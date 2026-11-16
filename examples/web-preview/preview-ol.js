"use strict";

(() => {
  const $ = (id) => document.getElementById(id);
  const defaults = [70, 15, 140, 55], world = 20037508.342789244;
  const state = { maps: [], selected: null, crs: "EPSG:3857", time: 0,
    upstream: "", views: {}, defaultMap: null, layer: null, requestURL: null, generation: 0, controller: null,
    loadingPackets: new Set(), features: [], featureStyles: new Map() };
  const viewConfig = () => state.views[state.selected?.id] || {};
  const initialExtent = () => state.views[state.selected?.id]?.bounds || defaults;
  function notice(text, error = false) {
    $("notice").textContent = text;
    $("notice").classList.toggle("error", error);
    $("notice").hidden = !text;
  }
  if (!window.ol) {
    notice("本地 OpenLayers 文件加载失败，请重新启动预览服务后刷新页面。", true);
    return;
  }
  const projection = () => state.crs === "EPSG:3857" ? "EPSG:3857" : "EPSG:4326";
  const map = new ol.Map({
    target: "ol-map", layers: [], maxTilesLoading: 8,
    controls: [new ol.control.Attribution()],
    view: new ol.View({ center: ol.proj.fromLonLat([105, 35]), zoom: 3, enableRotation: false }),
    keyboardEventTarget: $("viewport"),
  });
  function servicePath(link) {
    const base = new URL(state.upstream), url = new URL(link, base.href + "/");
    const prefix = base.pathname.replace(/\/$/, "");
    if (url.origin !== base.origin || !url.pathname.startsWith(prefix + "/")) throw new Error("服务返回的链接不属于当前地图服务");
    return "/service" + url.pathname.slice(prefix.length) + url.search;
  }
  async function json(path) {
    const response = await fetch(path, { signal: AbortSignal.timeout(15000) });
    if (!response.ok) throw new Error(`读取服务失败（HTTP ${response.status}）`);
    return response.json();
  }
  function supportedCRS(record) {
    return ["EPSG:3857", "OGC:CRS84"].filter((crs) => (record.crs || []).some((v) =>
      v === crs || (crs === "EPSG:3857" ? /\/EPSG\/0\/3857$/.test(v) : /\/CRS84$/.test(v))));
  }
  function gridId() { return state.crs === "EPSG:3857" ? "WebMercatorQuad" : "WorldCRS84Quad"; }
  function currentGeoExtent() {
    return ol.proj.transformExtent(map.getView().calculateExtent(map.getSize()), projection(), "EPSG:4326");
  }
  function fit(geo, duration = 0) {
    map.getView().fit(ol.proj.transformExtent(geo, "EPSG:4326", projection()), { padding: [20, 20, 20, 20], duration });
  }
  function updateForm() {
    const geo = currentGeoExtent();
    ["west", "south", "east", "north"].forEach((name, i) => {
      $("bounds-form").elements[name].value = geo[i].toFixed(5);
    });
    $("view-info").textContent = `${state.crs} · OpenLayers`;
  }
  function showRequest(path) {
    state.requestURL = state.upstream + path;
    $("request-link").href = "/service" + path;
    $("request-link").textContent = state.requestURL;
  }
  function clearLayer() {
    state.generation++;
    // Finish active loads while their queue listeners are still attached.
    for (const packet of state.loadingPackets) failPacket(packet);
    state.loadingPackets.clear();
    state.controller?.abort();
    state.features = [];
    $("feature-details").hidden = true;
    $("feature-summary").textContent = $("protocol").value === "wfs"
      ? "正在请求当前范围的 WFS 要素…" : "按 LXMC 分类绘制面边线。切换 WFS 可筛选类型并查看属性。";
    if (state.layer) {
      map.removeLayer(state.layer);
      state.layer.dispose();
      state.layer = null;
    }
    map.getTargetElement().setAttribute("aria-busy", "false");
  }
  function cards() {
    const list = $("map-list"); list.replaceChildren();
    $("count").textContent = state.maps.length;
    for (const record of state.maps) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "map-card" + (record.id === state.selected?.id ? " active" : "");
      button.setAttribute("aria-pressed", String(record.id === state.selected?.id));
      button.innerHTML = '<span class="map-symbol">◈</span><span><span class="map-name"></span><small></small></span>';
      button.querySelector(".map-name").textContent = record.title || record.id;
      button.querySelector("small").textContent = record.id;
      button.onclick = () => select(record);
      list.append(button);
    }
    list.querySelector(".active")?.scrollIntoView({ block: "nearest" });
    if (!state.maps.length) list.innerHTML = '<p class="muted">暂无已发布地图。发布后点击刷新。</p>';
  }
  // Fetch through the same-origin proxy so upstream problems remain visible.
  function failPacket(packet) {
    if (typeof packet.setState === "function") packet.setState(3);
    else { packet.state = 3; packet.changed(); }
  }
  function imageLoader(generation, signal) {
    return async (packet, url) => {
      state.loadingPackets.add(packet);
      let objectURL;
      try {
        const response = await fetch(url, { signal });
        if (!response.ok) {
          let detail = `地图请求失败（HTTP ${response.status}）`;
          if ((response.headers.get("content-type") || "").includes("json")) {
            const problem = await response.json();
            detail += "：" + (problem.detail || problem.title || "");
          }
          throw new Error(detail);
        }
        const blob = await response.blob();
        if (!blob.type.startsWith("image/")) throw new Error("服务没有返回地图图片");
        if (generation !== state.generation) throw new DOMException("Map source was replaced", "AbortError");
        objectURL = URL.createObjectURL(blob);
        const element = packet.getImage(), release = () => {
          state.loadingPackets.delete(packet);
          URL.revokeObjectURL(objectURL);
        };
        element.addEventListener("load", release, { once: true });
        element.addEventListener("error", release, { once: true });
        element.src = objectURL;
      } catch (error) {
        if (objectURL) URL.revokeObjectURL(objectURL);
        state.loadingPackets.delete(packet);
        // Tiles expose setState; ImageWrapper exposes state and changed().
        // Even cancelled tiles must finish, releasing OpenLayers' load queue.
        failPacket(packet);
        if (generation !== state.generation || error.name === "AbortError") return;
        notice(error.message, true);
      }
    };
  }
  function watchSource(source, generation, tiled) {
    let pending = 0, loaded = 0, failed = 0;
    const prefix = tiled ? "tileload" : "imageload";
    source.on(prefix + "start", () => {
      if (generation !== state.generation) return;
      pending++;
      map.getTargetElement().setAttribute("aria-busy", "true");
      $("render-status").textContent = tiled ? `正在加载 WMTS 瓦片 · ${loaded} 已完成` : "正在渲染地图…";
    });
    const finish = (error) => {
      if (generation !== state.generation) return;
      pending = Math.max(0, pending - 1);
      if (error) failed++; else loaded++;
      if (!failed) notice("");
      if (failed) {
        $("render-status").textContent = `地图加载失败 · ${failed} 个请求失败，可刷新重试`;
        if (!$("notice").classList.contains("error")) notice("地图图片加载失败，请检查服务状态后重试。", true);
      } else {
        $("render-status").textContent = tiled ? `● WMTS · ${gridId()} · ${loaded} 张瓦片` : "● 地图已渲染";
      }
      // A serial tile queue can briefly have no active request between tiles.
      // OpenLayers' rendercomplete event confirms that the entire view is ready.
      if (!tiled || failed) map.getTargetElement().setAttribute("aria-busy", String(pending > 0));
    };
    source.on(prefix + "end", () => finish(false));
    source.on(prefix + "error", () => finish(true));
  }
  function staticLayer(loader, mapId = state.selected.id, forceOGC = false) {
    const size = map.getSize(), extent = map.getView().calculateExtent(size);
    const scale = Math.min(1, 1600 / size[0], 1200 / size[1]);
    const width = Math.max(1, Math.round(size[0] * scale)), height = Math.max(1, Math.round(size[1] * scale));
    let path;
    if ($("protocol").value === "wms" && !forceOGC) {
      path = "/wms?" + new URLSearchParams({ SERVICE: "WMS", VERSION: "1.3.0", REQUEST: "GetMap",
        LAYERS: mapId, STYLES: "", CRS: state.crs === "OGC:CRS84" ? "CRS:84" : state.crs,
        BBOX: extent.join(","), WIDTH: width, HEIGHT: height, FORMAT: "image/png",
        TRANSPARENT: "TRUE", geello_time: state.time });
    } else {
      const link = mapId === state.selected.id && state.selected.links?.find((item) => item.rel === "map");
      const route = link ? servicePath(link.href).slice("/service".length) : `/ogc/maps/collections/${encodeURIComponent(mapId)}/map`;
      path = route + "?" + new URLSearchParams({ bbox: extent.join(","), crs: state.crs, width, height, geello_time: state.time });
    }
    showRequest(path);
    return new ol.layer.Image({ source: new ol.source.ImageStatic({ url: "/service" + path,
      imageExtent: extent, projection: projection(), interpolate: false, imageLoadFunction: loader }) });
  }
  function wmtsLayer(loader) {
    const mercator = state.crs === "EPSG:3857", id = gridId();
    const min = state.selected["x-geello-min-zoom"] ?? 0;
    const max = Math.min(30, state.selected["x-geello-max-zoom"] ?? 22);
    const origin = mercator ? [-world, world] : [-180, 90];
    const extent = mercator ? [-world, -world, world, world] : [-180, -90, 180, 90];
    const span = mercator ? world * 2 : 180;
    const resolutions = Array.from({ length: max + 1 }, (_, z) => span / (256 * 2 ** z));
    const sizes = resolutions.map((_, z) => [2 ** z * (mercator ? 1 : 2), 2 ** z]);
    const tileGrid = new ol.tilegrid.WMTS({ origin, extent, resolutions, sizes, minZoom: min,
      matrixIds: resolutions.map((_, z) => String(z)), tileSize: 256 });
    const source = new ol.source.WMTS({ url: "/service/wmts", layer: state.selected.id,
      matrixSet: id, style: "default", format: "image/png", version: "1.0.0",
      projection: projection(), tileGrid, wrapX: false, transition: 0, interpolate: false,
      dimensions: { geello_time: state.time }, tileLoadFunction: loader });
    // Native WMTS uses modern OpenLayers' top-left, downward-positive row index.
    const tileURL = source.getTileUrlFunction();
    source.setTileUrlFunction((coordinate, pixelRatio, proj) => {
      const url = tileURL(coordinate, pixelRatio, proj);
      if (url) showRequest(url.slice("/service".length));
      return url;
    });
    return new ol.layer.Tile({ source, preload: 0 });
  }
  function featureStyle(feature) {
    const type = String(feature.get(viewConfig().typeField || "LXMC") || "");
    if ($("feature-type").value && type !== $("feature-type").value) return undefined;
    const color = viewConfig().categories?.find((category) => category.value === type)?.color || "#00d9ff";
    if (!state.featureStyles.has(color)) state.featureStyles.set(color,
      new ol.style.Style({ stroke: new ol.style.Stroke({ color, width: 2 }) }));
    return state.featureStyles.get(color);
  }
  function featureSummary() {
    const type = $("feature-type").value;
    const visible = state.features.filter((feature) => !type || feature.get(viewConfig().typeField || "LXMC") === type);
    $("feature-summary").textContent = `WFS 已加载 ${state.features.length} 个面，显示 ${visible.length} 个。点击边线查看属性。`;
  }
  async function loadWFS(generation, signal) {
    const config = viewConfig(), bounds = currentGeoExtent();
    let offset = 0, matched = 0, lastPath;
    const features = [], seen = new Set();
    while (true) {
      const path = "/wfs?" + new URLSearchParams({ SERVICE: "WFS", VERSION: "2.0.0", REQUEST: "GetFeature",
        TYPENAMES: config.wfsDataset, OUTPUTFORMAT: "application/json", COUNT: 50,
        STARTINDEX: offset, BBOX: bounds.join(",") });
      showRequest(path);
      lastPath = path;
      const response = await fetch("/service" + path, { signal });
      if (!response.ok) throw new Error(`WFS 请求失败（HTTP ${response.status}）：${(await response.text()).slice(0, 250)}`);
      const page = await response.json();
      if (generation !== state.generation) return;
      if (page.type !== "FeatureCollection" || !Array.isArray(page.features)) throw new Error("WFS 未返回 GeoJSON 要素集");
      for (const feature of page.features) {
        const key = feature.id ?? JSON.stringify(feature);
        if (seen.has(key)) throw new Error("WFS 返回了重复分页要素，请刷新重试");
        seen.add(key);
      }
      features.push(...new ol.format.GeoJSON().readFeatures(page, { dataProjection: "EPSG:4326", featureProjection: projection() }));
      offset += page.features.length;
      matched = Number(page.numberMatched);
      if (Number.isFinite(matched) ? offset >= matched : page.features.length < 50) break;
      if (!page.features.length || offset > 100000) throw new Error("WFS 分页超过预览限制");
    }
    const source = new ol.source.Vector({ features, wrapX: false });
    const overlay = new ol.layer.Vector({ source, style: featureStyle });
    state.features = features;
    // The server renders imagery only; OpenLayers draws the WFS outlines.
    const background = staticLayer(imageLoader(generation, signal), config.baseMap, true);
    showRequest(lastPath);
    let imageReady = false;
    const status = () => {
      if (generation !== state.generation) return;
      $("render-status").textContent = `● WFS · ${features.length} 个要素${imageReady ? " · 影像已加载" : " · 影像加载中"}`;
    };
    background.getSource().on("imageloadend", () => { imageReady = true; status(); });
    background.getSource().on("imageloaderror", () => {
      if (generation === state.generation) $("render-status").textContent = "WFS 已加载，影像加载失败";
    });
    state.layer = new ol.layer.Group({ layers: [background, overlay] });
    map.addLayer(state.layer);
    featureSummary(); notice(""); status();
  }
  function replace() {
    if (!state.selected || !supportedCRS(state.selected).length) return;
    clearLayer();
    const tiled = $("protocol").value === "wmts";
    if (tiled && !(state.selected["x-geello-tile-matrix-sets"] || []).includes(gridId())) {
      notice(`此地图未发布 ${gridId()} 瓦片矩阵集，请选择其他坐标系或预览方式。`, true);
      $("render-status").textContent = "瓦片矩阵集未发布";
      return;
    }
    const generation = state.generation;
    state.controller = new AbortController();
    notice("正在加载地图…");
    $("render-status").textContent = "正在加载地图…";
    map.getTargetElement().setAttribute("aria-busy", "true");
    if ($("protocol").value === "wfs") {
      if (!viewConfig().wfsDataset) {
        notice("此地图未配置 WFS 数据集，请选择影像与矢量组合地图。", true);
        map.getTargetElement().setAttribute("aria-busy", "false");
        return;
      }
      const signal = state.controller.signal;
      loadWFS(generation, signal).catch((error) => {
        if (generation !== state.generation || error.name === "AbortError") return;
        notice(error.message, true); $("render-status").textContent = "WFS 加载失败";
        map.getTargetElement().setAttribute("aria-busy", "false");
      });
      return;
    }
    try {
      const loader = imageLoader(generation, state.controller.signal);
      state.layer = tiled ? wmtsLayer(loader) : staticLayer(loader);
      watchSource(state.layer.getSource(), generation, tiled);
      map.addLayer(state.layer);
    } catch (error) { notice(error.message, true); }
  }
  function setView(geo) {
    map.setView(new ol.View({ projection: projection(), enableRotation: false,
      center: ol.proj.transform([105, 35], "EPSG:4326", projection()), zoom: 3, maxZoom: 30 }));
    fit(geo); updateForm();
  }
  function select(record) {
    clearLayer(); state.selected = record; cards();
    const config = viewConfig();
    const wfs = $("protocol").querySelector('[value="wfs"]');
    wfs.disabled = !config.wfsDataset;
    if (wfs.disabled && $("protocol").value === "wfs") $("protocol").value = "ogc";
    $("feature-panel").hidden = !config.wfsDataset;
    $("feature-type").replaceChildren(new Option("全部类型", ""), ...(config.categories || []).map((category) => new Option(category.value, category.value)));
    $("feature-type").disabled = $("protocol").value !== "wfs";
    $("feature-legend").replaceChildren(...(config.categories || []).map((category) => {
      const entry = document.createElement("div"), swatch = document.createElement("span");
      swatch.style.backgroundColor = category.color; entry.append(swatch, category.value); return entry;
    }));
    $("map-title").textContent = record.title || record.id;
    const options = supportedCRS(record);
    $("crs").replaceChildren(...options.map((value) => new Option(
      value === "EPSG:3857" ? "Web Mercator · EPSG:3857" : "经纬度 · OGC:CRS84", value)));
    const self = record.links?.find((link) => link.rel === "self");
    $("metadata").hidden = !self;
    if (self) $("metadata").href = servicePath(self.href);
    if (!options.length) return notice("此地图未发布 EPSG:3857 或 CRS84。", true);
    state.crs = options.includes(state.crs) ? state.crs : options[0];
    $("crs").value = state.crs;
    setView(initialExtent()); replace();
  }
  async function discover() {
    $("refresh").disabled = true;
    try {
      const records = [], visited = new Set();
      let next = "/service/ogc/maps/collections";
      while (next) {
        if (visited.has(next)) throw new Error("服务返回了重复的地图分页链接");
        visited.add(next);
        const page = await json(next);
        records.push(...page.collections);
        const link = page.links?.find((item) => item.rel === "next");
        next = link ? servicePath(link.href) : null;
      }
      state.maps = Array.from(new Map(records.map((record) => [record.id, record])).values());
      const record = state.maps.find((item) => item.id === state.selected?.id)
        || state.maps.find((item) => item.id === (new URLSearchParams(location.search).get("map") || state.defaultMap))
        || state.maps.find((item) => item.id === "preview:combined") || state.maps[0];
      if (record) select(record);
      else {
        clearLayer(); state.selected = null; cards();
        $("map-title").textContent = "地图预览"; $("metadata").hidden = true;
        $("render-status").textContent = "暂无已发布地图";
        notice("暂无已发布地图，发布后点击刷新。");
      }
      $("connection").textContent = "● 服务已连接"; $("connection").className = "badge online";
      try {
        const health = await json("/service/health/ready");
        $("backend").textContent = `渲染后端 · ${health.backend.toUpperCase()}`;
      } catch { $("backend").textContent = "状态检查暂不可用"; }
    } catch (error) {
      notice(error.message, true);
      $("connection").textContent = "● 连接失败"; $("connection").className = "badge offline";
    } finally { $("refresh").disabled = false; }
  }
  map.on("rendercomplete", () => {
    if (state.layer) map.getTargetElement().setAttribute("aria-busy", "false");
  });
  map.on("moveend", () => {
    updateForm();
    if (state.upstream && state.selected && $("protocol").value !== "wmts") replace();
  });
  map.on("pointermove", (event) => {
    const point = ol.proj.transform(event.coordinate, projection(), "EPSG:4326");
    $("pointer").textContent = `${point[0].toFixed(4)}°, ${point[1].toFixed(4)}°`;
  });
  $("protocol").onchange = () => {
    $("feature-type").disabled = $("protocol").value !== "wfs";
    $("feature-type").value = ""; replace();
  };
  $("feature-type").onchange = () => {
    state.layer?.getLayers?.().item(1)?.changed(); featureSummary();
  };
  map.on("singleclick", (event) => {
    if ($("protocol").value !== "wfs") return;
    const feature = map.forEachFeatureAtPixel(event.pixel, (feature) => feature, { hitTolerance: 7 });
    $("feature-details").hidden = !feature;
    if (!feature) return;
    const table = document.createElement("table");
    for (const [key, value] of Object.entries(feature.getProperties())) {
      if (key === feature.getGeometryName()) continue;
      const row = table.insertRow(); row.insertCell().textContent = key;
      row.insertCell().textContent = value == null ? "—" : String(value);
    }
    $("feature-properties").replaceChildren(table); $("feature-details").open = true;
  });
  $("refresh").onclick = discover;
  $("crs").onchange = () => {
    const geo = currentGeoExtent(); clearLayer(); state.crs = $("crs").value;
    setView(geo); replace();
  };
  $("apply-time").onclick = () => {
    if (!$("time").reportValidity()) return;
    const time = Number($("time").value);
    if (!Number.isFinite(time) || time < 0) return;
    state.time = time; replace();
  };
  $("reset").onclick = () => fit(initialExtent(), 200);
  document.querySelectorAll("[data-bounds]").forEach((button) => {
    button.onclick = () => fit(button.dataset.bounds.split(",").map(Number), 200);
  });
  $("bounds-form").onsubmit = (event) => {
    event.preventDefault();
    const geo = ["west", "south", "east", "north"].map((name) => Number(event.target.elements[name].value));
    if (!geo.every(Number.isFinite) || geo[0] < -180 || geo[2] > 180 || geo[1] < -85 || geo[3] > 85 || geo[0] >= geo[2] || geo[1] >= geo[3]) {
      return notice("请输入有效范围：西 < 东、南 < 北，经度 −180～180°，纬度 −85～85°。", true);
    }
    fit(geo, 200);
  };
  $("zoom-in").onclick = () => map.getView().setZoom(map.getView().getZoom() + 1);
  $("zoom-out").onclick = () => map.getView().setZoom(map.getView().getZoom() - 1);
  $("copy").onclick = async () => {
    if (!state.requestURL) return;
    try {
      await navigator.clipboard.writeText(state.requestURL);
      $("copy").textContent = "已复制";
      setTimeout(() => { $("copy").textContent = "复制链接"; }, 1500);
    } catch { notice("复制失败，可手动复制当前地图请求链接。", true); }
  };
  $("viewport").addEventListener("keydown", (event) => {
    if (event.target !== $("viewport")) return;
    if (event.key === "+" || event.key === "=") { event.preventDefault(); $("zoom-in").click(); }
    else if (event.key === "-") { event.preventDefault(); $("zoom-out").click(); }
  });
  (async () => {
    try {
      const config = await json("/preview-config.json");
      state.views = config.maps || {}; state.defaultMap = config.defaultMap;
      const protocol = new URLSearchParams(location.search).get("protocol");
      if (["ogc", "wms", "wmts", "wfs"].includes(protocol)) $("protocol").value = protocol;
      state.upstream = config.upstream; $("server-url").textContent = config.upstream;
      await discover();
    } catch (error) { notice(error.message, true); }
  })();
})();
