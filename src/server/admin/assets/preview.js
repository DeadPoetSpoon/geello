import { request, serviceURL } from "./requests.js";

export function imageRequestPlan(bounds, worldExtent, size, maxDimension, deviceRatio = 1) {
  if (![...bounds, ...worldExtent, ...size, maxDimension, deviceRatio].every(Number.isFinite) || !size.every((value) => value > 0) || maxDimension < 1 || bounds[2] <= bounds[0] || bounds[3] <= bounds[1]) return;
  const extent = [Math.max(bounds[0], worldExtent[0]), Math.max(bounds[1], worldExtent[1]), Math.min(bounds[2], worldExtent[2]), Math.min(bounds[3], worldExtent[3])];
  if (extent[0] >= extent[2] || extent[1] >= extent[3]) return;
  const cssWidth = size[0] * (extent[2] - extent[0]) / (bounds[2] - bounds[0]);
  const cssHeight = size[1] * (extent[3] - extent[1]) / (bounds[3] - bounds[1]);
  const pixelRatio = Math.min(Math.max(1, deviceRatio), 2, maxDimension / cssWidth, maxDimension / cssHeight);
  const width = Math.max(1, Math.floor(cssWidth * pixelRatio));
  const height = Math.max(1, Math.floor(cssHeight * pixelRatio));
  return { extent, width, height, pixelRatio };
}

export async function mountPreview({ id, container, signal, h, field, input, select, button, head, notify, pageLimit, maxDimension }) {
  if (!globalThis.ol) throw new Error("本地 OpenLayers 加载失败，请检查管理静态资源认证及 CSP");
  const { data: record } = await request(`admin/v1/resources/map/${encodeURIComponent(id)}`, { signal });
  if (!record.published) throw new Error("仅支持已发布地图。请先保存并发布，再进入预览。");
  const { data: collection } = await request(`ogc/maps/collections/${encodeURIComponent(id)}`, { signal });
  const publishedCRS = collection.crs.map((value) => value.includes("3857") ? "EPSG:3857" : /CRS84/.test(value) ? "OGC:CRS84" : value);
  const supportedCRS = publishedCRS.filter((value) => ["EPSG:3857", "OGC:CRS84"].includes(value));
  container.append(head(collection.title || id, "OpenLayers 仅显示正式服务渲染。棋盘格为透明背景，不加载外部底图。"));
  if (!supportedCRS.length) {
    container.append(h("div", { className: "message warning" }, `该地图发布的 CRS 暂不支持内置预览：${publishedCRS.join(", ")}`, h("a", { href: serviceURL(`ogc/maps/collections/${encodeURIComponent(id)}`), target: "_blank", rel: "noopener" }, "查看正式服务")));
    return () => {};
  }
  const messages = h("div", { "aria-live": "polite" });
  const tell = (message, type = "") => notify(message, type, messages);
  const crs = select(supportedCRS.map((value) => [value, value]), supportedCRS.includes("EPSG:3857") ? "EPSG:3857" : supportedCRS[0]);
  const protocols = select([["wmts", "WMTS 原生瓦片"], ["ogc", "OGC Maps 单图"], ["wms", "WMS 1.3.0 单图"]], "wmts");
  const time = input("0", { type: "number", step: "any" });
  const extentInput = input("", { placeholder: "minX,minY,maxX,maxY" });
  const canvas = h("div", { className: "map", tabIndex: 0, role: "region", "aria-label": "地图预览，使用方向键平移，加减键缩放" });
  const rendered = h("div", { className: "map-note", role: "status" }, "等待请求…");
  const endpoint = h("code");
  const properties = h("div", { className: "feature-result" });
  const featureChoice = select([["", "关闭属性检查"]], "", { "aria-label": "属性检查数据集" });
  const world = 20037508.342789244;
  const map = new ol.Map({ target: canvas, layers: [], controls: ol.control.defaults.defaults({ attribution: false }), maxTilesLoading: 1 });
  const observer = new ResizeObserver(() => map.updateSize());
  observer.observe(canvas);
  let generation = 0;
  let current = new AbortController();
  let tileGeneration = new AbortController();
  let featureGeneration = new AbortController();
  let layer;
  let overlay;
  let timer;
  let configured = false;
  let failed = false;
  let appliedTime = 0;
  let imageGeneration = 0;
  const featureSource = new ol.source.Vector({ wrapX: false });
  const featureLayer = new ol.layer.Vector({
    source: featureSource,
    style: new ol.style.Style({
      stroke: new ol.style.Stroke({ color: "#ed8b24", width: 3 }),
      fill: new ol.style.Fill({ color: "rgba(237,139,36,0.16)" }),
      image: new ol.style.Circle({
        radius: 7,
        stroke: new ol.style.Stroke({ color: "#ed8b24", width: 2 }),
        fill: new ol.style.Fill({ color: "rgba(237,139,36,0.25)" }),
      }),
    }),
  });
  const projection = () => crs.value === "OGC:CRS84" ? "EPSG:4326" : "EPSG:3857";
  const validExtent = () => crs.value === "EPSG:3857" ? [-world, -world, world, world] : [-180, -90, 180, 90];
  const matrix = () => crs.value === "EPSG:3857" ? "WebMercatorQuad" : "WorldCRS84Quad";
  const imageError = (error) => {
    if (error.name === "AbortError" || signal.aborted) return;
    failed = true;
    tell(`地图请求失败：${error.message}\n已停止本轮预览请求；不会自动重试。检查服务状态后点击“重试预览”。`, "error");
    rendered.textContent = "预览请求失败，当前可能保留上次图像。";
    tileGeneration.abort();
  };
  const showImage = async (image, url, requestSignal, expectedGeneration, expectedImage) => {
    if (failed) throw new DOMException("预览已停止", "AbortError");
    endpoint.textContent = url;
    rendered.textContent = "正在请求服务端图像…";
    const { data } = await request(url, { mode: "blob", preview: true, signal: requestSignal });
    if (signal.aborted || expectedGeneration !== generation || (expectedImage !== undefined && expectedImage !== imageGeneration)) throw new DOMException("图像已过期", "AbortError");
    const objectURL = URL.createObjectURL(data);
    await new Promise((resolve, reject) => {
      image.onload = () => { URL.revokeObjectURL(objectURL); resolve(); };
      image.onerror = () => { URL.revokeObjectURL(objectURL); reject(new Error("服务器返回的图像无法解码")); };
      image.src = objectURL;
    });
    if (requestSignal.aborted || signal.aborted || expectedGeneration !== generation || (expectedImage !== undefined && expectedImage !== imageGeneration)) throw new DOMException("图像已过期", "AbortError");
    rendered.textContent = "正式服务图像已加载。透明区域可能无数据；不是加载失败。";
  };
  const installImage = () => {
    if (failed || signal.aborted || protocols.value === "wmts" || !map.getSize()?.every((value) => value > 0)) return;
    imageGeneration += 1;
    tileGeneration.abort();
    tileGeneration = new AbortController();
    const requestSignal = tileGeneration.signal;
    const expected = generation;
    const expectedImage = imageGeneration;
    const size = map.getSize();
    const bounds = map.getView().calculateExtent(size);
    const worldExtent = validExtent();
    const plan = imageRequestPlan(bounds, worldExtent, size, maxDimension, globalThis.devicePixelRatio || 1);
    if (!plan) return;
    const { extent, width, height, pixelRatio } = plan;
    const query = protocols.value === "wms" ? new URLSearchParams({ SERVICE: "WMS", VERSION: "1.3.0", REQUEST: "GetMap", LAYERS: id, STYLES: "", CRS: crs.value === "OGC:CRS84" ? "CRS:84" : crs.value, BBOX: extent.join(","), WIDTH: width, HEIGHT: height, FORMAT: "image/png", TRANSPARENT: "TRUE", geello_time: appliedTime }) : new URLSearchParams({ bbox: extent.join(","), crs: crs.value, width, height, geello_time: appliedTime });
    query.set("geello_pixel_ratio", pixelRatio);
    const imageURL = protocols.value === "wms" ? `wms?${query}` : `ogc/maps/collections/${encodeURIComponent(id)}/map?${query}`;
    const source = new ol.source.Image({ projection: projection(), interpolate: false, loader: async () => {
      const image = new Image();
      try { await showImage(image, imageURL, requestSignal, expected, expectedImage); }
      catch (error) {
        if (error.name === "AbortError") return { image: document.createElement("canvas"), extent, resolution: 1, pixelRatio: 1 };
        if (expected === generation && !requestSignal.aborted) imageError(error);
        throw error;
      }
      return { image, extent, resolution: [(extent[2] - extent[0]) * pixelRatio / width, (extent[3] - extent[1]) * pixelRatio / height], pixelRatio };
    } });
    const next = new ol.layer.Image({ source, extent });
    if (overlay) map.removeLayer(overlay);
    overlay = next;
    map.getLayers().insertAt(0, next);
  };
  const refresh = (resetView = false) => {
    generation += 1;
    current.abort();
    tileGeneration.abort();
    featureGeneration.abort();
    current = new AbortController();
    tileGeneration = new AbortController();
    featureSource.clear();
    properties.replaceChildren();
    messages.replaceChildren();
    rendered.textContent = "等待请求…";
    failed = false;
    const grids = collection["x-geello-tile-matrix-sets"] || [];
    protocols.querySelector('option[value="wmts"]').disabled = !grids.includes(matrix());
    if (protocols.value === "wmts" && !grids.includes(matrix())) protocols.value = "ogc";
    map.getLayers().clear();
    overlay = undefined;
    if (resetView || !configured) {
      const extent = validExtent();
      map.setView(new ol.View({ projection: projection(), center: [0, 0], zoom: 1, extent, showFullExtent: true, multiWorld: false, constrainResolution: protocols.value === "wmts", maxResolution: (crs.value === "EPSG:3857" ? world * 2 : 180) / 256, minZoom: protocols.value === "wmts" ? collection["x-geello-min-zoom"] || 0 : 0, maxZoom: collection["x-geello-max-zoom"] ?? 22 }));
      map.getView().fit(extent, { size: map.getSize(), padding: [12, 12, 12, 12], maxZoom: 2 });
      configured = true;
    }
    if (protocols.value === "wmts") {
      const mercator = crs.value === "EPSG:3857";
      const max = Math.min(30, collection["x-geello-max-zoom"] ?? 22);
      const min = collection["x-geello-min-zoom"] || 0;
      const resolutions = Array.from({ length: max + 1 }, (_, zoom) => (mercator ? world * 2 : 180) / (256 * 2 ** zoom));
      const grid = new ol.tilegrid.WMTS({ origin: mercator ? [-world, world] : [-180, 90], extent: validExtent(), resolutions, matrixIds: resolutions.map((_, zoom) => String(zoom)), minZoom: min, sizes: resolutions.map((_, zoom) => [2 ** zoom * (mercator ? 1 : 2), 2 ** zoom]), tileSize: 256 });
      const expected = generation;
      const source = new ol.source.WMTS({ url: serviceURL("wmts"), layer: id, matrixSet: matrix(), style: "default", format: "image/png", projection: projection(), tileGrid: grid, wrapX: false, transition: 0, interpolate: false, dimensions: { geello_time: appliedTime }, tileLoadFunction: (tile, url) => {
        const requestSignal = tileGeneration.signal;
        if (failed || requestSignal.aborted) { tile.setState(3); return; }
        showImage(tile.getImage(), url, requestSignal, expected).catch((error) => {
          tile.setState(3);
          if (expected === generation && !requestSignal.aborted) imageError(error);
        });
      } });
      layer = new ol.layer.Tile({ source, preload: 0 });
      map.addLayer(layer);
    } else installImage();
    map.addLayer(featureLayer);
    extentInput.value = map.getView().calculateExtent(map.getSize()).map((value) => Number(value.toFixed(5))).join(",");
  };
  const applyTime = () => {
    const value = Number(time.value);
    if (!Number.isFinite(value) || value < 0) { tell("时间必须为非负有限秒数", "error"); return; }
    appliedTime = value;
    refresh();
  };
  const checkFeatures = async () => {
    featureGeneration.abort();
    featureGeneration = new AbortController();
    const featureSignal = featureGeneration.signal;
    const selected = featureChoice.value;
    featureSource.clear(); properties.replaceChildren();
    if (!selected) return;
    try {
      const bounds = ol.proj.transformExtent(map.getView().calculateExtent(map.getSize()), projection(), "EPSG:4326");
      const bbox = [Math.max(-180, bounds[0]), Math.max(-90, bounds[1]), Math.min(180, bounds[2]), Math.min(90, bounds[3])];
      const limit = Math.min(100, pageLimit);
      const features = [];
      let cursor;
      let more = false;
      do {
        const query = cursor ? new URLSearchParams({ cursor }) : new URLSearchParams({ limit, bbox: bbox.join(",") });
        const { data } = await request(`ogc/features/collections/${encodeURIComponent(selected)}/items?${query}`, { signal: featureSignal, preview: true });
        features.push(...data.features.slice(0, 1000 - features.length));
        const next = data.links?.find((link) => link.rel === "next");
        cursor = next ? new URL(next.href, serviceURL(".")).searchParams.get("cursor") : undefined;
        more = Boolean(cursor);
      } while (cursor && features.length < 1000);
      if (featureSignal.aborted || signal.aborted) return;
      featureSource.addFeatures(new ol.format.GeoJSON().readFeatures({ type: "FeatureCollection", features }, { dataProjection: "EPSG:4326", featureProjection: projection() }));
      properties.replaceChildren(h("p", {}, `${features.length} 个要素${more ? "（已达 1000 上限，结果不完整）" : ""}。点击高亮要素查看属性；不会合并到服务端地图。`));
    } catch (error) { if (error.name !== "AbortError") tell(`属性检查失败：${error.message}`, "error"); }
  };
  const previewTools = h("section", { className: "panel" }, h("div", { className: "preview-toolbar" }, field("协议", protocols), field("发布 CRS", crs), field("geello_time（秒）", time), h("div", { className: "actions" }, button("应用时间", applyTime), button("+1 秒", () => { time.value = Number(time.value) + 1; applyTime(); }))), h("div", { className: "toolbar" }, field("视图范围（当前 CRS，XY）", extentInput), button("定位范围", () => {
    const values = extentInput.value.split(",").map(Number);
    if (values.length !== 4 || !values.every(Number.isFinite) || values[0] >= values[2] || values[1] >= values[3]) { tell("范围需为四个有序有限数字", "error"); return; }
    map.getView().fit(values, { size: map.getSize(), padding: [10, 10, 10, 10] });
    if (failed) refresh();
  }), button("全图", () => {
    map.getView().fit(validExtent(), { size: map.getSize(), padding: [12, 12, 12, 12] });
    if (failed) refresh();
  }), button("重试预览", () => refresh())), h("p", { className: "muted" }, `已发布 CRS：${publishedCRS.join("，")}。不支持的 CRS 可通过正式服务单独调用。`));
  container.append(previewTools, messages, h("div", { className: "preview-layout" }, h("section", { className: "panel" }, canvas, rendered, h("details", {}, h("summary", {}, "最近的正式服务请求"), endpoint)), h("section", { className: "panel" }, h("h2", {}, "属性检查（可选）"), field("已公开的矢量数据集", featureChoice), button("查询当前范围", checkFeatures), properties)));
  protocols.addEventListener("change", () => refresh(true));
  crs.addEventListener("change", () => refresh(true));
  map.on("movestart", () => {
    featureGeneration.abort();
    if (protocols.value !== "wmts") { tileGeneration.abort(); imageGeneration += 1; }
  });
  map.on("moveend", () => {
    clearTimeout(timer);
    extentInput.value = map.getView().calculateExtent(map.getSize()).map((value) => Number(value.toFixed(5))).join(",");
    if (protocols.value !== "wmts") timer = setTimeout(installImage, 200);
  });
  map.on("singleclick", (event) => {
    const feature = map.forEachFeatureAtPixel(event.pixel, (candidate) => candidate, { layerFilter: (candidate) => candidate === featureLayer });
    if (feature) {
      const attributes = { ...feature.getProperties() };
      delete attributes[feature.getGeometryName()];
      properties.replaceChildren(h("h3", {}, `要素 ${feature.getId() ?? "无 ID"}`), h("pre", {}, JSON.stringify(attributes, null, 2)));
    }
  });
  map.updateSize();
  refresh(true);
  try {
    const datasets = record.dependencies.filter((key) => key.kind === "dataset");
    for (const key of datasets) {
      try {
        const { data: dataset } = await request(`admin/v1/resources/dataset/${encodeURIComponent(key.id)}`, { signal });
        if (!dataset.published || dataset.class !== "vector") continue;
        await request(`ogc/features/collections/${encodeURIComponent(key.id)}`, { signal });
        featureChoice.append(h("option", { value: key.id }, key.id));
      } catch (error) { if (error.name === "AbortError") throw error; }
    }
  } catch (error) { if (error.name !== "AbortError") tell(error.message, "error"); }
  const cleanup = () => { clearTimeout(timer); current.abort(); tileGeneration.abort(); featureGeneration.abort(); observer.disconnect(); map.setTarget(undefined); map.dispose(); };
  signal.addEventListener("abort", cleanup, { once: true });
  return cleanup;
}
