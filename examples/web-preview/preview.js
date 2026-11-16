"use strict";

const $ = (id) => document.getElementById(id);
const viewport = $("viewport");
const image = $("map-image");
const radius = 6378137;
const limit = 20037508.342789244;
const defaults = [70, 15, 140, 55];
const state = { maps: [], selected: null, bounds: defaults.slice(), crs: "EPSG:3857", time: 0,
  upstream: "", imageURL: null, controller: null, sequence: 0, timer: null, drag: null, requestURL: null };

function notice(text, error = false) {
  $("notice").textContent = text;
  $("notice").classList.toggle("error", error);
  $("notice").hidden = !text;
}
function project(x, y) {
  if (state.crs === "OGC:CRS84") return [x, y];
  const latitude = Math.max(-85.05112878, Math.min(85.05112878, y));
  return [radius * x * Math.PI / 180, radius * Math.log(Math.tan(Math.PI / 4 + latitude * Math.PI / 360))];
}
function unproject(x, y) {
  return state.crs === "OGC:CRS84" ? [x, y] : [x / radius * 180 / Math.PI, (2 * Math.atan(Math.exp(y / radius)) - Math.PI / 2) * 180 / Math.PI];
}
function fittedBounds(geo) {
  const a = project(geo[0], geo[1]), b = project(geo[2], geo[3]);
  const ratio = viewport.clientWidth / viewport.clientHeight;
  const width = Math.max(b[0] - a[0], (b[1] - a[1]) * ratio);
  const height = width / ratio;
  const x = (a[0] + b[0]) / 2, y = (a[1] + b[1]) / 2;
  return [x - width / 2, y - height / 2, x + width / 2, y + height / 2];
}
function fit(geo) {
  state.bounds = fittedBounds(geo);
  schedule();
}
function geoBounds() {
  return [...unproject(state.bounds[0], state.bounds[1]), ...unproject(state.bounds[2], state.bounds[3])];
}
function servicePath(link) {
  const base = new URL(state.upstream);
  const url = new URL(link, base.href + "/");
  const prefix = base.pathname.replace(/\/$/, "");
  if (url.origin !== base.origin || !url.pathname.startsWith(prefix + "/")) throw new Error("服务返回的链接不属于当前地图服务");
  return "/service" + url.pathname.slice(prefix.length) + url.search;
}
async function fetchJSON(path) {
  const response = await fetch(path, { signal: AbortSignal.timeout(15000) });
  if (!response.ok) throw new Error(`读取服务失败（HTTP ${response.status}）`);
  return response.json();
}
function supportedCRS(map) {
  const values = map.crs || [];
  return ["EPSG:3857", "OGC:CRS84"].filter((crs) => values.some((v) =>
    v === crs || (crs === "EPSG:3857" ? /\/EPSG\/0\/3857$/.test(v) : /\/CRS84$/.test(v))));
}
function renderCards() {
  const list = $("map-list");
  list.replaceChildren();
  $("count").textContent = state.maps.length;
  for (const map of state.maps) {
    const card = document.createElement("button");
    card.type = "button";
    card.className = "map-card" + (map.id === state.selected?.id ? " active" : "");
    card.setAttribute("aria-pressed", String(map.id === state.selected?.id));
    const icon = document.createElement("span");
    icon.className = "map-symbol";
    icon.textContent = "◈";
    const text = document.createElement("span");
    const name = document.createElement("span");
    name.className = "map-name";
    name.textContent = map.title || map.id;
    const id = document.createElement("small");
    id.textContent = map.id;
    text.append(name, id);
    card.append(icon, text);
    card.onclick = () => selectMap(map);
    list.append(card);
  }
  if (!state.maps.length) {
    const message = document.createElement("p");
    message.className = "muted";
    message.textContent = "暂无已发布地图。发布地图后，点击刷新即可查看。";
    list.append(message);
  }
}
function invalidate(clearImage = false) {
  clearTimeout(state.timer);
  state.sequence++;
  state.controller?.abort();
  if (clearImage) {
    image.hidden = true;
    if (state.imageURL) URL.revokeObjectURL(state.imageURL);
    state.imageURL = null;
    image.removeAttribute("src");
  }
}
function selectMap(map) {
  const oldCRS = state.crs, geo = geoBounds();
  invalidate(true);
  state.selected = map;
  renderCards();
  $("map-title").textContent = map.title || map.id;
  const options = supportedCRS(map);
  $("crs").replaceChildren(...options.map((value) => new Option(value === "EPSG:3857" ? "Web Mercator · EPSG:3857" : "经纬度 · OGC:CRS84", value)));
  const self = map.links?.find((link) => link.rel === "self");
  $("metadata").hidden = !self;
  if (self) $("metadata").href = servicePath(self.href);
  if (!options.length) {
    notice("此地图未发布预览页支持的 EPSG:3857 或 CRS84 坐标系。", true);
    $("render-status").textContent = "坐标系不支持";
    return;
  }
  state.crs = options.includes(oldCRS) ? oldCRS : options[0];
  $("crs").value = state.crs;
  state.bounds = fittedBounds(geo);
  schedule(0);
}
async function discover() {
  $("refresh").disabled = true;
  try {
    const maps = [], visited = new Set();
    let next = "/service/ogc/maps/collections";
    while (next) {
      if (visited.has(next)) throw new Error("服务返回了重复的地图分页链接");
      visited.add(next);
      const page = await fetchJSON(next);
      maps.push(...page.collections);
      const link = page.links?.find((item) => item.rel === "next");
      next = link ? servicePath(link.href) : null;
    }
    state.maps = Array.from(new Map(maps.map((map) => [map.id, map])).values());
    // Prefer the small protocol-demo map when it is present. Older M3 preview
    // datasets may have derived manifests from a previous binary and return
    // 422 until they are republished; the protocol demo remains a valid,
    // immediately viewable map for a fresh server.
    const selected = state.maps.find((map) => map.id === state.selected?.id) || state.maps.find((map) => map.id === "demo:labels") || state.maps.find((map) => map.id === "preview:combined") || state.maps[0];
    if (selected) selectMap(selected);
    else {
      invalidate(true);
      state.selected = null;
      renderCards();
      $("map-title").textContent = "地图预览";
      $("metadata").hidden = true;
      $("render-status").textContent = "暂无已发布地图";
      notice("服务已连接，暂无已发布地图。发布后点击刷新。");
    }
    $("connection").textContent = "● 服务已连接";
    $("connection").className = "badge online";
    try {
      const health = await fetchJSON("/service/health/ready");
      $("backend").textContent = `渲染后端 · ${health.backend.toUpperCase()}`;
    } catch { $("backend").textContent = "状态检查暂不可用"; }
  } catch (error) {
    $("connection").textContent = "● 连接失败";
    $("connection").className = "badge offline";
    $("render-status").textContent = "读取地图列表失败，可点击刷新重试";
    notice(error.message, true);
  } finally { $("refresh").disabled = false; }
}
function imageRequest(width, height) {
  const q = new URLSearchParams({ bbox: state.bounds.join(","), width, height, geello_time: state.time });
  if ($("protocol").value === "wms") {
    const crs = state.crs === "OGC:CRS84" ? "CRS:84" : state.crs;
    for (const [key, value] of Object.entries({ SERVICE: "WMS", VERSION: "1.3.0", REQUEST: "GetMap", LAYERS: state.selected.id, STYLES: "", CRS: crs, BBOX: q.get("bbox"), WIDTH: width, HEIGHT: height, FORMAT: "image/png", TRANSPARENT: "TRUE" })) q.set(key, value);
    for (const key of ["bbox", "width", "height"]) q.delete(key);
    return "/wms?" + q;
  }
  q.set("crs", state.crs);
  const link = state.selected.links?.find((item) => item.rel === "map");
  const path = link ? servicePath(link.href).slice("/service".length) : `/ogc/maps/collections/${encodeURIComponent(state.selected.id)}/map`;
  return path + (path.includes("?") ? "&" : "?") + q;
}
function schedule(delay = 180) {
  invalidate();
  drawGrid();
  syncBounds();
  state.timer = setTimeout(render, delay);
}
function syncBounds() {
  const geo = geoBounds();
  ["west", "south", "east", "north"].forEach((name, i) => {
    $("bounds-form").elements[name].value = geo[i].toFixed(5);
  });
  $("view-info").textContent = state.crs;
}
async function render() {
  if (!state.selected || !supportedCRS(state.selected).length) return;
  const sequence = ++state.sequence;
  const controller = new AbortController();
  state.controller = controller;
  const started = performance.now();
  const scale = Math.min(1, 1600 / viewport.clientWidth, 1200 / viewport.clientHeight);
  const width = Math.max(1, Math.round(viewport.clientWidth * scale)), height = Math.max(1, Math.round(viewport.clientHeight * scale));
  const path = imageRequest(width, height);
  state.requestURL = state.upstream + path;
  $("request-link").href = "/service" + path;
  $("request-link").textContent = state.requestURL;
  $("render-status").textContent = "正在渲染地图…";
  viewport.setAttribute("aria-busy", "true");
  if (image.hidden) notice("正在渲染地图…");
  let nextURL;
  try {
    const response = await fetch("/service" + path, { signal: controller.signal });
    if (!response.ok) {
      let detail = `地图请求失败（HTTP ${response.status}）`;
      if ((response.headers.get("content-type") || "").includes("json")) {
        const problem = await response.json();
        detail += "：" + (problem.detail || problem.title || "请刷新地图列表后重试");
      }
      throw new Error(detail);
    }
    const blob = await response.blob();
    if (!blob.type.startsWith("image/")) throw new Error("服务未返回地图图片，请检查发布配置。");
    nextURL = URL.createObjectURL(blob);
    const pending = new Image();
    pending.src = nextURL;
    await pending.decode();
    if (sequence !== state.sequence) { URL.revokeObjectURL(nextURL); return; }
    const previous = state.imageURL;
    state.imageURL = nextURL;
    image.src = nextURL;
    image.alt = `${state.selected.title || state.selected.id}，${state.crs} 地图`;
    image.style.transform = "";
    image.hidden = false;
    if (previous) URL.revokeObjectURL(previous);
    notice("");
    $("render-status").textContent = `● 已渲染 · ${width} × ${height} · ${Math.round(performance.now() - started)} ms`;
  } catch (error) {
    if (nextURL) URL.revokeObjectURL(nextURL);
    if (sequence !== state.sequence || error.name === "AbortError") return;
    image.hidden = true;
    // A server restart can leave an older derived-store manifest published.
    // Keep the preview useful by returning to the known-good protocol-demo map
    // when that specific resource compatibility error is encountered.
    if (/incompatible vector manifest/i.test(error.message) && state.selected?.id !== "demo:labels") {
      const fallback = state.maps.find((map) => map.id === "demo:labels");
      if (fallback) {
        notice("当前地图的派生存储与服务版本不兼容，已切换到可用的 demo:labels。", true);
        selectMap(fallback);
        return;
      }
    }
    notice(error.message, true);
    $("render-status").textContent = "渲染失败，调整范围或刷新重试";
  } finally {
    if (sequence === state.sequence) viewport.setAttribute("aria-busy", "false");
  }
}
function drawGrid() {
  const canvas = $("grid"), ctx = canvas.getContext("2d");
  const width = viewport.clientWidth, height = viewport.clientHeight, dpr = window.devicePixelRatio || 1;
  canvas.width = width * dpr; canvas.height = height * dpr;
  ctx.scale(dpr, dpr);
  ctx.strokeStyle = "#e1eae4"; ctx.fillStyle = "#90a597"; ctx.font = "10px Segoe UI";
  const geo = geoBounds(), span = geo[2] - geo[0];
  const step = [0.0001, 0.001, 0.01, 0.1, 0.5, 1, 2, 5, 10, 20, 30, 60].find((n) => span / n <= 10) || 90;
  const [a, b, c, d] = state.bounds;
  for (let lon = Math.ceil(geo[0] / step) * step, count = 0; lon <= geo[2] && count < 100; lon += step, count++) {
    const x = (project(lon, 0)[0] - a) / (c - a) * width;
    ctx.beginPath(); ctx.moveTo(x, 0); ctx.lineTo(x, height); ctx.stroke();
    if (x > 10 && x < width - 40) ctx.fillText(`${+lon.toFixed(4)}°`, x + 5, 16);
  }
  for (let lat = Math.ceil(Math.max(-85, geo[1]) / step) * step, count = 0; lat <= Math.min(85, geo[3]) && count < 100; lat += step, count++) {
    const y = (d - project(0, lat)[1]) / (d - b) * height;
    ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(width, y); ctx.stroke();
    if (y > 27 && y < height - 45) ctx.fillText(`${+lat.toFixed(4)}°`, 8, y - 5);
  }
}
function zoom(factor, px = .5, py = .5) {
  const [a, b, c, d] = state.bounds;
  const w = c - a, h = d - b;
  const min = state.crs === "EPSG:3857" ? 1 : .00001;
  const max = state.crs === "EPSG:3857" ? limit * 8 : 1440;
  if (w * factor < min || w * factor > max) return;
  const x = a + w * px, y = d - h * py;
  state.bounds = [x - w * factor * px, y - h * factor * (1 - py), x + w * factor * (1 - px), y + h * factor * py];
  schedule();
}
$("zoom-in").onclick = () => zoom(.7);
$("zoom-out").onclick = () => zoom(1 / .7);
$("reset").onclick = () => fit(defaults);
$("refresh").onclick = discover;
document.querySelectorAll("[data-bounds]").forEach((button) => button.onclick = () => fit(button.dataset.bounds.split(",").map(Number)));
$("protocol").onchange = () => { invalidate(true); schedule(0); };
$("crs").onchange = () => {
  const geo = geoBounds();
  invalidate(true);
  state.crs = $("crs").value;
  fit(geo);
};
$("apply-time").onclick = () => {
  if (!$("time").reportValidity()) return;
  const value = Number($("time").value);
  if (!Number.isFinite(value) || value < 0) return;
  state.time = value;
  schedule(0);
};
$("bounds-form").onsubmit = (event) => {
  event.preventDefault();
  const values = ["west", "south", "east", "north"].map((name) => Number(event.target.elements[name].value));
  const [a, b, c, d] = values;
  if (!values.every(Number.isFinite) || a < -180 || c > 180 || b < -85 || d > 85 || a >= c || b >= d) {
    notice("请输入有效范围：西 < 东、南 < 北，经度 −180～180°，纬度 −85～85°。", true);
    return;
  }
  fit(values);
};
$("copy").onclick = async () => {
  if (!state.requestURL) return;
  try {
    await navigator.clipboard.writeText(state.requestURL);
    $("copy").textContent = "已复制";
    setTimeout(() => { $("copy").textContent = "复制链接"; }, 1500);
  } catch { notice("复制失败，可展开当前地图请求并手动复制链接。", true); }
};
viewport.addEventListener("wheel", (event) => {
  event.preventDefault();
  const rect = viewport.getBoundingClientRect();
  zoom(event.deltaY > 0 ? 1.2 : 1 / 1.2, (event.clientX - rect.left) / rect.width, (event.clientY - rect.top) / rect.height);
}, { passive: false });
viewport.addEventListener("pointerdown", (event) => {
  if (event.target.closest("button") || event.button !== 0 || state.drag) return;
  invalidate();
  state.drag = { id: event.pointerId, x: event.clientX, y: event.clientY, bounds: state.bounds.slice() };
  viewport.setPointerCapture(event.pointerId);
  viewport.classList.add("dragging");
});
viewport.addEventListener("pointermove", (event) => {
  const rect = viewport.getBoundingClientRect(), [a, b, c, d] = state.bounds;
  const p = unproject(a + (event.clientX - rect.left) / rect.width * (c - a), d - (event.clientY - rect.top) / rect.height * (d - b));
  $("pointer").textContent = `${p[0].toFixed(4)}°, ${p[1].toFixed(4)}°`;
  if (!state.drag || state.drag.id !== event.pointerId) return;
  const dx = event.clientX - state.drag.x, dy = event.clientY - state.drag.y;
  const source = state.drag.bounds;
  const x = -dx / rect.width * (source[2] - source[0]), y = dy / rect.height * (source[3] - source[1]);
  state.bounds = [source[0] + x, source[1] + y, source[2] + x, source[3] + y];
  image.style.transform = `translate(${dx}px, ${dy}px)`;
  drawGrid();
});
function endDrag(event) {
  if (!state.drag || state.drag.id !== event.pointerId) return;
  state.drag = null;
  viewport.classList.remove("dragging");
  schedule(0);
}
viewport.addEventListener("pointerup", endDrag);
viewport.addEventListener("pointercancel", endDrag);
viewport.addEventListener("lostpointercapture", endDrag);
viewport.addEventListener("keydown", (event) => {
  if (event.target !== viewport) return;
  const delta = { ArrowLeft: [-.2, 0], ArrowRight: [.2, 0], ArrowUp: [0, .2], ArrowDown: [0, -.2] }[event.key];
  if (delta) {
    event.preventDefault();
    const [a, b, c, d] = state.bounds, dx = (c - a) * delta[0], dy = (d - b) * delta[1];
    state.bounds = [a + dx, b + dy, c + dx, d + dy];
    schedule();
  } else if (event.key === "+" || event.key === "=") { event.preventDefault(); zoom(.7); }
  else if (event.key === "-") { event.preventDefault(); zoom(1 / .7); }
});
let previousSize;
new ResizeObserver(() => {
  const width = viewport.clientWidth, height = viewport.clientHeight;
  if (previousSize) {
    const [a, b, c, d] = state.bounds;
    const x = (a + c) / 2, half = (d - b) * width / height / 2;
    state.bounds = [x - half, b, x + half, d];
    schedule(250);
  }
  previousSize = [width, height];
}).observe(viewport);
(async () => {
  state.bounds = fittedBounds(defaults);
  drawGrid();
  try {
    const config = await fetchJSON("/preview-config.json");
    state.upstream = config.upstream;
    $("server-url").textContent = config.upstream;
    await discover();
  } catch (error) { notice(error.message, true); $("connection").textContent = "配置读取失败"; }
})();
