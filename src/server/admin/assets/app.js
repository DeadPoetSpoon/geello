import { dispatcher, latestRequest, poll, request, serviceURL, upload } from "./requests.js";
import { mountPreview } from "./preview.js";

const names = { dataset: "数据集", font: "字体", style: "样式", map: "地图", animation: "动画" };
const statuses = { staging: "接收中", preparing: "准备中", ready: "已完成", failed: "失败", unpublished: "已下线" };
const main = document.querySelector("main");
let capabilities = {};
let pageLimit = 20;
let dirty = false;
let lastHash = "";
let scope = new AbortController();
let cleanups = [];
let messages;

export function element(tag, attributes = {}, ...children) {
  const node = document.createElement(tag);
  for (const [name, value] of Object.entries(attributes)) {
    if (value === undefined || value === null || value === false) continue;
    if (name.startsWith("on")) node.addEventListener(name.slice(2), value);
    else if (name === "className") node.className = value;
    else if (name in node && !name.startsWith("aria")) node[name] = value;
    else node.setAttribute(name, String(value));
  }
  for (const child of children.flat(Infinity)) {
    if (child !== null && child !== undefined && child !== false) node.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return node;
}
const h = element;
const link = (text, hash, attributes = {}) => h("a", { href: hash, ...attributes }, text);
const button = (text, action, className = "", attributes = {}) => h("button", { type: "button", className, onclick: action, ...attributes }, text);
const badge = (status) => h("span", { className: `badge ${status}` }, statuses[status] || status);
const shortVersion = (value) => String(value || "—").slice(0, 16);
const resourcePath = (kind, id) => `admin/v1/resources/${kind}/${encodeURIComponent(id)}`;
const resourceHash = (kind, id) => `#/resources/${kind}/${encodeURIComponent(id)}`;
const jsonOptions = (document) => ({ method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(document) });

function field(text, input, help) {
  return h("label", { className: "field" }, h("span", {}, text), input, help && h("small", {}, help));
}
function input(value = "", attributes = {}) { return h("input", { value, ...attributes }); }
function select(options, value, attributes = {}) {
  const node = h("select", attributes, options.map(([id, title]) => h("option", { value: id }, title)));
  node.value = value;
  return node;
}
function notify(text, type = "", target = messages) {
  target.replaceChildren(h("div", { className: `message ${type}`, role: type === "error" ? "alert" : "status" }, text));
}
function report(error) { if (error.name !== "AbortError") notify(error.message || String(error), "error"); }
function guarded(action) {
  const signal = scope.signal;
  return async () => {
    try { await action(); }
    catch (error) { if (!signal.aborted) report(error); }
  };
}
function head(title, description, ...actions) {
  return h("div", { className: "page-head" }, h("div", {}, h("h1", {}, title), h("p", { className: "muted" }, description)), h("div", { className: "actions" }, actions));
}
function table(headers, rows) {
  return h("div", { className: "table-wrap" }, h("table", {}, h("thead", {}, h("tr", {}, headers.map((title) => h("th", { scope: "col" }, title)))), h("tbody", {}, rows)));
}
function pagination(offset, total, change) {
  return h("div", { className: "pagination" }, h("span", { className: "muted" }, total ? `${offset + 1}–${Math.min(offset + pageLimit, total)} / ${total}` : "共 0 项"), h("div", { className: "actions" }, button("上一页", guarded(() => change(Math.max(0, offset - pageLimit))), "", { disabled: offset === 0 }), button("下一页", guarded(() => change(offset + pageLimit)), "", { disabled: offset + pageLimit >= total })));
}

async function confirmAction(title, description, exactID) {
  const trigger = document.activeElement;
  const dialog = h("dialog", { "aria-label": title });
  const confirmation = input("", { autocomplete: "off", placeholder: exactID || "" });
  const accepted = button("确认操作", () => dialog.close("yes"), "danger", { disabled: Boolean(exactID) });
  if (exactID) confirmation.addEventListener("input", () => { accepted.disabled = confirmation.value !== exactID; });
  dialog.append(h("h2", {}, title), h("p", {}, description), exactID && field("输入完整资源 ID 确认", confirmation), h("div", { className: "actions" }, button("取消", () => dialog.close("no")), accepted));
  document.body.append(dialog);
  dialog.showModal();
  return new Promise((resolve) => dialog.addEventListener("close", () => {
    const result = dialog.returnValue === "yes";
    dialog.remove();
    trigger?.focus();
    resolve(result);
  }, { once: true }));
}

async function overview() {
  const container = h("div");
  main.append(head("服务概览", "查看服务就绪状态与发布资源。统计来自当前进程，不等于 RSS 或显存。", link("API 文档 ↗", serviceURL("api.html"), { className: "button", target: "_blank", rel: "noopener" })), container);
  const signal = scope.signal;
  const refresh = async () => {
    try {
      const { data } = await request("health/ready", { signal });
      const { data: resources } = await request("admin/v1/resources?limit=1", { signal });
      const metrics = [
        ["已注册资源", resources.total], ["实际渲染后端", data.backend],
        ["已核算内存", `${(data.live_accounted_bytes / 1048576).toFixed(1)} MiB`], ["目录代数", resources.generation],
      ];
      container.replaceChildren(h("div", { className: "metrics" }, metrics.map(([title, value]) => h("div", { className: "metric" }, h("span", {}, title), h("strong", {}, value)))), h("div", { className: "grid" }, h("section", { className: "panel" }, h("h2", {}, "发布工作流"), h("p", {}, "上传数据集 → 创建样式 → 编排地图 → 正式服务预览。保存会立即准备并发布，并非保存服务器草稿。"), h("div", { className: "actions" }, link("上传数据集", "#/resources/dataset/new", { className: "button primary" }), link("创建地图", "#/resources/map/new", { className: "button" }))), h("section", { className: "panel" }, h("h2", {}, "运行诊断"), h("pre", {}, JSON.stringify(data, null, 2)))));
    } catch (error) {
      if (!signal.aborted) { notify(`连接已断开，以下为上次结果。${error.message}`, "warning"); throw error; }
    }
  };
  cleanups.push(poll(refresh, 10000));
}

async function resources(kind) {
  if (!names[kind]) throw new Error("未知资源类型");
  const search = input("", { placeholder: "按资源 ID 搜索", "aria-label": "资源 ID 关键字", maxLength: 256 });
  const published = select([["", "全部状态"], ["true", "已发布"], ["false", "已下线"]], "", { "aria-label": "发布状态" });
  const content = h("div");
  let generation;
  const beginLoad = latestRequest();
  const signal = scope.signal;
  const load = async (offset = 0) => {
    const isLatest = beginLoad();
    const query = new URLSearchParams({ kind, q: search.value, offset, limit: pageLimit });
    if (published.value) query.set("published", published.value);
    let result;
    try { result = await request(`admin/v1/resources?${query}`, { signal }); }
    catch (error) { if (isLatest() && !signal.aborted) throw error; return; }
    const { data } = result;
    if (!isLatest() || signal.aborted) return;
    if (generation !== undefined && generation !== data.generation && offset) {
      generation = data.generation;
      notify("目录已更新，已返回第一页。", "warning");
      return load(0);
    }
    generation = data.generation;
    content.replaceChildren(data.resources.length ? table(["资源标识", "所有权", "发布状态", "版本", "操作"], data.resources.map((record) => h("tr", {}, h("td", {}, link(record.key.id, resourceHash(kind, record.key.id))), h("td", {}, record.ownership === "managed" ? "托管" : "外部"), h("td", {}, badge(record.published ? "ready" : "unpublished")), h("td", {}, h("code", {}, shortVersion(record.version))), h("td", {}, link("查看 / 编辑", resourceHash(kind, record.key.id)), kind === "map" && record.published && h("span", {}, " · ", link("OL 预览", `#/preview/${encodeURIComponent(record.key.id)}`)))))) : h("div", { className: "panel empty" }, search.value || published.value ? "没有匹配的资源。调整筛选条件后重试。" : `还没有${names[kind]}。使用右上角按钮开始发布。`), pagination(offset, data.total, load));
  };
  const filters = h("form", { className: "toolbar", onsubmit: (event) => { event.preventDefault(); guarded(() => load())(); } }, search, published, h("button", { type: "submit" }, "筛选"), button("刷新", guarded(() => load())));
  main.append(head(names[kind], "按 ID 筛选，按目录快照分页；版本冲突由服务器最终校验。", link(`新建${names[kind]}`, resourceHash(kind, "new"), { className: "button primary" })), filters, content);
  await load();
}

async function operationPage() {
  const beginLoad = latestRequest();
  let offset = 0;
  const status = select([["", "全部状态"], ...Object.entries(statuses)], "", { "aria-label": "任务状态" });
  const kind = select([["", "全部类型"], ...Object.entries(names)], "", { "aria-label": "资源类型" });
  const id = input("", { placeholder: "完整资源 ID（精确匹配）", "aria-label": "任务资源 ID" });
  const content = h("div");
  const signal = scope.signal;
  const load = async (next = offset) => {
    const isLatest = beginLoad();
    const query = new URLSearchParams({ offset: next, limit: pageLimit });
    for (const [key, control] of [["status", status], ["kind", kind], ["resource_id", id]]) if (control.value) query.set(key, control.value);
    try {
      const { data } = await request(`admin/v1/operations?${query}`, { signal });
      if (!isLatest() || signal.aborted) return;
      offset = next;
      content.replaceChildren(data.operations.length ? table(["任务 ID（倒序）", "资源", "状态", "诊断 / 版本"], data.operations.map((operation) => h("tr", {}, h("td", {}, h("code", {}, operation.operation_id)), h("td", {}, link(`${names[operation.resource.kind]} · ${operation.resource.id}`, resourceHash(operation.resource.kind, operation.resource.id))), h("td", {}, badge(operation.status)), h("td", {}, operation.diagnostics.length ? operation.diagnostics.join("\n") : shortVersion(operation.version))))) : h("div", { className: "panel empty" }, "没有匹配的任务。"), pagination(offset, data.total, load));
      messages.replaceChildren();
    } catch (error) { if (isLatest() && !signal.aborted) { notify(`连接已断开，以下为上次结果。${error.message}`, "warning"); throw error; } }
  };
  main.append(head("异步任务", "每 2 秒更新，隐藏标签页暂停；不伪造进度，也不提供服务端未支持的取消操作。"), h("form", { className: "toolbar", onsubmit: (event) => { event.preventDefault(); guarded(() => load(0))(); } }, status, kind, id, h("button", { type: "submit" }, "筛选"), button("刷新", guarded(() => load()))), content);
  cleanups.push(poll(load));
}

async function servicePage() {
  const { data } = await request("api", { signal: scope.signal });
  main.append(head("服务信息", "只读配置；不提供重启、修改预算、清缓存或文件系统浏览。"), h("div", { className: "grid" }, h("section", { className: "panel" }, h("h2", {}, "服务端能力"), h("pre", {}, JSON.stringify(data["x-geello-enabled"], null, 2))), h("section", { className: "panel" }, h("h2", {}, "服务入口"), h("p", {}, `配置公开地址：${data.servers?.[0]?.url || "未提供"}`), h("div", { className: "actions" }, ["api.html", "api", "ogc/maps/collections", "ogc/features/collections", "health/ready", "health/live"].map((path) => link(path, serviceURL(path), { className: "button", target: "_blank", rel: "noopener" }))), h("p", { className: "muted" }, "公开地址仅展示；管理台请求始终使用当前页面推导的同源服务前缀。"))));
}

async function choices(kind) {
  let offset = 0;
  let generation;
  const records = [];
  do {
    const { data } = await request(`admin/v1/resources?${new URLSearchParams({ kind, published: true, offset, limit: pageLimit })}`, { signal: scope.signal });
    if (generation !== undefined && generation !== data.generation) throw new Error("目录在读取资源选择器期间变化，请重新加载页面");
    generation = data.generation;
    records.push(...data.resources);
    offset += data.resources.length;
    if (offset >= data.total) return records;
    if (!data.resources.length || offset >= 1000) throw new Error("资源选择器超过 1000 项，请使用 TOML 填写完整资源 ID");
  } while (true);
}

function template(kind, id, type = "area") {
  const quotedID = JSON.stringify(id);
  if (kind === "map") return `schema_version = 1\nid = ${quotedID}\ntitle = "新地图"\noutput_crs = ["OGC:CRS84", "EPSG:3857"]\nbackground = "#eaf2f5"\ntiles = { matrix_sets = ["WebMercatorQuad", "WorldCRS84Quad"], min_zoom = 0, max_zoom = 8 }\nitems = []\n`;
  if (kind === "animation") return `schema_version = 1\nid = ${quotedID}\nduration = 2.0\n[[geometry_tracks]]\nid = "position"\nsource_crs = { definition = "OGC:CRS84", space = "GeographicXy" }\ninterpolation = "linear"\ntrajectory = { anchor = [0.0, 0.0], points = [[0.0, 0.0], [1.0, 1.0]], timing = "distance" }\n`;
  const symbols = {
    area: 'kind = "area"\nfill = "#89b8a4"',
    line: 'kind = "line"\ncolor = "#287a86"\nwidth = { value = 2.0, unit = "px" }',
    point: 'kind = "point"\nshape = "circle"\nsize = { value = 8.0, unit = "px" }\nfill = "#087f83"',
    raster: 'kind = "raster"\nmode = "rgb"\nbands = [1, 2, 3]\nrange = { kind = "explicit", min = 0, max = 255 }\nresampling = { enlarge = "nearest", reduce = "nearest" }',
    label: 'kind = "label"\nfield = "/name"\nfonts = [{ resource = "user:font", face_index = 0 }]\nsize = { value = 14.0, unit = "px" }\nplacement = "point"\ncolor = "#243647"',
  };
  return `schema_version = 1\nid = ${quotedID}\n[[rules]]\nid = "base"\n[[rules.symbols]]\nid = "symbol"\n${symbols[type]}\n`;
}

async function detail(kind, id) {
  if (!names[kind]) throw new Error("未知资源类型");
  const creating = id === "new";
  const signal = scope.signal;
  let record;
  let baseline;
  if (!creating) {
    const response = await request(resourcePath(kind, id), { signal });
    record = response.data;
    baseline = response.headers.get("etag");
    if (!baseline) throw new Error("资源没有强 ETag，禁止编辑");
  }
  const identity = input(creating ? `user:new-${kind}` : id, { readOnly: !creating, required: true, pattern: "[A-Za-z0-9_.-]+:[A-Za-z0-9_.-]+" });
  const status = h("div", { className: "editor-status", role: "status" }, creating ? "尚未发布" : `编辑基线：${baseline}`);
  const taskBox = h("div");
  const content = h("div");
  let busy = false;
  const condition = () => creating ? { "If-None-Match": "*" } : { "If-Match": baseline };
  const currentPath = () => {
    if (!/^[A-Za-z_][A-Za-z0-9_.-]*:[A-Za-z_][A-Za-z0-9_.-]*$/.test(identity.value) || identity.value.length > 256) throw new Error("资源 ID 必须为 namespace:local 格式");
    return resourcePath(kind, identity.value);
  };
  const conflict = async (error) => {
    if (error.status === 409 && !creating) {
      const { data: current, headers } = await request(resourcePath(kind, id), { signal });
      notify(`${error.message}\n本地内容和原始基线 ${baseline} 已保留。当前版本：${headers.get("etag")}；发布状态：${current.published ? "已发布" : "已下线"}。请比较内容后明确选择重新加载，不会自动改用新 ETag 重试。`, "error");
    } else report(error);
  };
  const runWrite = (action) => guarded(async () => {
    if (busy) return;
    busy = true;
    const controls = Array.from(content.querySelectorAll("input,select,textarea,button")).map((control) => [control, control.disabled]);
    for (const [control] of controls) control.disabled = true;
    main.setAttribute("aria-busy", "true");
    try {
      const response = await action();
      if (signal.aborted || !response) return;
      if (response.status === 202) {
        dirty = false;
        const operation = response.data;
        status.textContent = "服务器已接收，正在准备；发布成功前旧版本继续有效。";
        taskBox.replaceChildren(h("div", { className: "message" }, `任务 ${operation.operation_id} · 准备中`, link(" 查看任务列表", "#/operations")));
        await watchOperation(operation.operation_id, taskBox, async () => {
          status.textContent = "已发布。请重新加载以获取新的编辑基线。";
          taskBox.append(button("加载已发布版本", () => reloadDetail()));
        });
        if (taskBox.textContent.includes("· 失败")) {
          dirty = true;
          status.textContent = "准备失败；本地输入已保留，可修改后重新提交。原编辑基线没有变化。";
        }
      } else {
        dirty = false;
        if (response.status === 204) return;
        notify("操作成功。正在刷新资源状态。", "");
        await reloadDetail();
      }
    } catch (error) { if (!signal.aborted) await conflict(error); }
    finally {
      busy = false;
      for (const [control, disabled] of controls) control.disabled = disabled;
      if (!signal.aborted) main.removeAttribute("aria-busy");
    }
  });
  const reloadDetail = async () => {
    if (dirty && !await confirmAction("放弃本地修改？", "重新加载会替换当前页面中的未保存文本和文件选择。")) return;
    dirty = false;
    const next = resourceHash(kind, identity.value);
    if (location.hash !== next) location.hash = next;
    else await route(true);
  };
  main.append(head(creating ? `新建${names[kind]}` : id, "保存即准备并发布。浏览器不持久化草稿或文件，刷新会丢失未保存输入。", link("返回列表", `#/resources/${kind}`, { className: "button" }), !creating && button("重新加载", guarded(reloadDetail)), record?.published && kind === "map" && link("OL 预览", `#/preview/${encodeURIComponent(id)}`, { className: "button primary" })), status, taskBox, content);
  const identityField = field("资源 ID（创建后不可改名）", identity);
  content.append(h("section", { className: "panel" }, identityField));
  identity.addEventListener("input", () => { dirty = true; });
  if (record) {
    const dependencies = record.dependencies.map((key) => link(`${names[key.kind]} · ${key.id}`, resourceHash(key.kind, key.id)));
    const details = h("dl", { className: "detail-grid" });
    for (const [title, value] of [["状态", badge(record.published ? "ready" : "unpublished")], ["所有权", record.ownership], ["源格式", record.source.format], ["源路径", record.source.path], ["有效 CRS", record.effective_crs?.definition || "—"], ["资源版本", record.version], ["数据版本", record.data_version], ["派生版本", record.derived_version || "—"], ["依赖", dependencies.length ? h("div", {}, dependencies) : "无"]]) details.append(h("dt", {}, title), h("dd", {}, value));
    const references = h("div");
    content.append(h("details", { className: "panel" }, h("summary", {}, "资源详情与依赖"), details, button("检查反向引用", guarded(async () => {
      references.replaceChildren(h("p", {}, "正在按有界分页扫描目录…"));
      let offset = 0;
      let generation;
      const found = [];
      do {
        const { data } = await request(`admin/v1/resources?offset=${offset}&limit=${pageLimit}`, { signal });
        if (generation !== undefined && generation !== data.generation) throw new Error("目录代数已变化，引用扫描无效，请重试");
        generation = data.generation;
        found.push(...data.resources.filter((candidate) => candidate.dependencies.some((key) => key.kind === kind && key.id === id)));
        offset += data.resources.length;
        if (offset >= data.total) break;
        if (!data.resources.length || offset >= 10000) throw new Error("引用扫描达到上限，不能判定没有引用；请缩小目录后重试");
      } while (true);
      references.replaceChildren(found.length ? h("div", {}, found.map((candidate) => h("p", {}, link(`${candidate.key.kind} · ${candidate.key.id}`, resourceHash(candidate.key.kind, candidate.key.id))))) : h("p", {}, "当前快照中没有反向引用。写操作仍由服务器再次检查。"));
    })), references));
  }
  if (["style", "map", "animation"].includes(kind)) {
    await configurationEditor({ kind, identity, record, content, signal, runWrite, currentPath, condition, status });
  }
  const fileInput = input("", { type: "file", accept: kind === "font" ? ".ttf,.otf,.ttc,.otc" : kind === "dataset" ? ".geojson,.json,.zip,.tif,.tiff,.toml" : ".toml" });
  const formatOptions = kind === "font" ? [["application/octet-stream", "字体 TTF / OTF / 集合"]] : kind === "dataset" ? [["application/geo+json", "GeoJSON"], ["application/zip", "Shapefile ZIP"], ["image/tiff", "GeoTIFF"], ["application/toml", "数据集 TOML"]] : [["application/toml", "TOML"]];
  const media = select(formatOptions, formatOptions[0][0]);
  const encoding = input(record?.source.encoding || "", { placeholder: "例如 UTF-8 / GBK" });
  const crs = input(record?.source.crs || "", { placeholder: "例如 EPSG:4326" });
  const sampleKind = select([["", "使用文件定义"], ["continuous", "连续"], ["categorical", "分类"]], record?.source.sample_kind || "");
  const progress = h("progress", { className: "progress", max: 1, value: 0, "aria-label": "文件上传进度" });
  fileInput.addEventListener("change", () => {
    dirty = true;
    const name = fileInput.files[0]?.name.toLowerCase() || "";
    if (kind === "dataset") media.value = name.endsWith(".zip") ? "application/zip" : /\.tiff?$/.test(name) ? "image/tiff" : name.endsWith(".toml") ? "application/toml" : "application/geo+json";
  });
  content.append(h("details", { className: "panel", open: kind === "dataset" || kind === "font" }, h("summary", {}, creating ? "上传原始文件并发布" : "用文件替换当前资源"), h("div", { className: "form-grid" }, field("原始文件", fileInput, `最大 ${(capabilities["max-upload-bytes"] / 1048576 || 0).toFixed(1)} MiB；文件不会持久化在浏览器。`), field("内容格式", media), kind === "dataset" && [field("编码（可选）", encoding), field("源 CRS（可选）", crs), field("栅格样本解释", sampleKind)]), progress, button("上传并发布", runWrite(async () => {
    const file = fileInput.files[0];
    if (!file) throw new Error("请选择要上传的文件");
    if (file.size > capabilities["max-upload-bytes"]) throw new Error("文件超过服务端上传上限");
    const headers = { ...condition(), "Content-Type": media.value };
    for (const [name, control] of [["Encoding", encoding], ["CRS", crs], ["Sample-Kind", sampleKind]]) if (kind === "dataset" && control.value) headers[`X-Geello-${name}`] = control.value;
    status.textContent = "正在上传原始文件…";
    return upload(`${currentPath()}/content`, file, headers, (fraction) => {
      if (signal.aborted) return;
      if (fraction === null) progress.removeAttribute("value");
      else progress.value = fraction;
      status.textContent = fraction === 1 ? "上传完成，等待服务器接收并建立准备任务…" : `正在上传${fraction === null ? "" : ` ${Math.round(fraction * 100)}%`}…`;
    });
  }), "primary")));
  const external = record?.ownership === "external" ? record.source : {};
  const path = input(external.path || "", { placeholder: "/absolute/path/on/server" });
  const sourceFormat = select((kind === "dataset" ? ["geojson", "shapefile", "geotiff", "toml"] : kind === "font" ? ["font"] : ["toml"]).map((name) => [name, name]), external.format || (kind === "dataset" ? "geojson" : kind === "font" ? "font" : "toml"));
  const sourceEncoding = input(external.encoding || "");
  const sourceCRS = input(external.crs || "");
  const sourceSample = select([["", "未指定"], ["continuous", "连续"], ["categorical", "分类"]], external.sample_kind || "");
  const sourcePanel = h("details", { className: "panel" }, h("summary", {}, "注册服务器外部源（不上传文件）"), h("p", { className: "muted" }, "仅允许已配置 external_roots 内的绝对路径，不接受 URL，不提供磁盘浏览。替换为外部源会改变所有权。"), h("div", { className: "form-grid" }, field("服务器路径", path), field("格式", sourceFormat), kind === "dataset" && [field("编码", sourceEncoding), field("源 CRS", sourceCRS), field("样本解释", sourceSample)]), button("注册并发布", runWrite(async () => {
    if (!path.value.startsWith("/")) throw new Error("请输入服务器上的绝对文件路径");
    return request(`${currentPath()}/source`, { method: "PUT", headers: { ...condition(), "Content-Type": "application/json" }, body: JSON.stringify({ path: path.value, format: sourceFormat.value, encoding: sourceEncoding.value || null, crs: sourceCRS.value || null, sample_kind: sourceSample.value || null }) });
  }), "primary"));
  sourcePanel.addEventListener("input", () => { dirty = true; });
  content.append(sourcePanel);
  if (record) {
    content.append(h("section", { className: "panel danger-zone" }, h("h2", {}, "资源生命周期"), h("p", { className: "muted" }, "没有级联操作；有引用、版本变化或正在使用时服务器可能拒绝。"), h("div", { className: "actions" }, button(record.published ? "从已注册源刷新发布" : "重新发布", runWrite(async () => {
      if (!await confirmAction("从已注册源发布？", "重新读取已登记源，并在准备成功后原子发布。所有权保持不变。")) return;
      return request(`${currentPath()}:publish`, { method: "POST", headers: condition() });
    })), button("下线资源", runWrite(async () => {
      if (!await confirmAction("下线资源？", "停止公开服务，但保留记录和原始内容。引用方必须先处理。")) return;
      return request(`${currentPath()}:unpublish`, { method: "POST", headers: condition() });
    }), "danger", { disabled: !record.published }), button("删除注册记录", runWrite(async () => {
      if (!await confirmAction("删除注册记录？", "移除目录记录，保留托管原始内容；不会自动删除引用方。")) return;
      const response = await request(currentPath(), { method: "DELETE", headers: condition() });
      dirty = false;
      location.hash = `#/resources/${kind}`;
      return response;
    }), "danger"), button("删除托管内容", runWrite(async () => {
      if (!await confirmAction("永久删除托管内容？", "这会移除资源记录并永久删除托管原始文件，无法撤销。", id)) return;
      const response = await request(`${currentPath()}/content`, { method: "DELETE", headers: condition() });
      dirty = false;
      location.hash = `#/resources/${kind}`;
      return response;
    }), "danger", { disabled: record.ownership !== "managed" }))));
  }
}

async function watchOperation(id, container, ready) {
  const signal = scope.signal;
  let finish;
  const stopped = new Promise((resolve) => { finish = resolve; });
  const cancel = () => { stop(); finish(); };
  const stop = poll(async () => {
    try {
      const { data } = await request(`admin/v1/operations/${encodeURIComponent(id)}`, { signal });
      container.replaceChildren(h("div", { className: `message ${data.status === "failed" ? "error" : ""}` }, `任务 ${id} · ${statuses[data.status]}${data.diagnostics.length ? `\n${data.diagnostics.join("\n")}` : ""}`, link(" 查看任务列表", "#/operations")));
      if (["ready", "failed", "unpublished"].includes(data.status)) {
        stop();
        if (data.status === "ready") await ready();
        finish();
      }
    } catch (error) {
      if (!signal.aborted) {
        container.replaceChildren(h("div", { className: "message warning" }, `任务状态未知，未判定成功。${error.message}`));
        if (error.status === 404 || error.status === 401 || error.status === 403) cancel();
        else throw error;
      }
    }
  });
  cleanups.push(cancel);
  return stopped;
}

async function configurationEditor({ kind, identity, record, content, signal, runWrite, currentPath, condition, status }) {
  const initial = record?.definition || template(kind, identity.value);
  const textarea = h("textarea", { value: initial, spellcheck: false, "aria-label": `${names[kind]} TOML` });
  let generatedTemplate = initial;
  identity.addEventListener("input", () => {
    if (!record && textarea.value === generatedTemplate) {
      generatedTemplate = template(kind, identity.value);
      textarea.value = generatedTemplate;
    }
  });
  let tree;
  let parsedText;
  let treeDirty = false;
  let revision = 0;
  let originalText;
  let conversionConfirmed = false;
  const auxiliary = h("div");
  const editorState = h("div", { className: "editor-status" }, "原始 TOML 按原样提交。配置检查不等于依赖验证或渲染成功。");
  let datasets = [];
  let styles = [];
  if (kind === "map") {
    try { datasets = await choices("dataset"); styles = await choices("style"); }
    catch (error) { if (signal.aborted) throw error; editorState.textContent = `资源选择器不可用，可使用 TOML。${error.message}`; }
  }
  const markTree = () => { revision += 1; treeDirty = true; dirty = true; editorState.textContent = "辅助表单已修改。请先应用到 TOML，再保存发布。"; };
  const bound = (object, key, title, type = "text") => {
    const control = input(object[key] ?? "", { type });
    control.addEventListener("input", () => {
      if (type === "number") object[key] = Number(control.value);
      else if (control.value) object[key] = control.value;
      else delete object[key];
      markTree();
    });
    return field(title, control);
  };
  const renderForm = () => {
    auxiliary.replaceChildren(h("h2", {}, "配置辅助"));
    if (!tree || parsedText !== textarea.value) {
      auxiliary.append(h("p", { className: "muted" }, "先点击“检查并载入表单”，成功后才能编辑辅助字段。文本仍可直接提交服务器。"));
      return;
    }
    if (kind === "map") {
      auxiliary.append(bound(tree, "title", "地图标题"), bound(tree, "language", "语言（可选）"), bound(tree, "background", "背景颜色（可选）"));
      const output = input(tree.output_crs.join(", "));
      output.addEventListener("change", () => { tree.output_crs = output.value.split(",").map((value) => value.trim()).filter(Boolean); markTree(); });
      auxiliary.append(field("发布 CRS（逗号分隔）", output));
      const tiles = tree.tiles || { matrix_sets: [], min_zoom: 0, max_zoom: 8 };
      const grids = input(tiles.matrix_sets.join(", "));
      grids.addEventListener("change", () => {
        const value = grids.value.split(",").map((value) => value.trim()).filter(Boolean);
        if (value.length) { tree.tiles = tiles; tiles.matrix_sets = value; }
        else delete tree.tiles;
        markTree();
      });
      auxiliary.append(field("矩阵集（留空禁用瓦片）", grids, "WebMercatorQuad, WorldCRS84Quad"));
      if (tree.tiles) auxiliary.append(h("div", { className: "form-grid" }, bound(tiles, "min_zoom", "最小缩放", "number"), bound(tiles, "max_zoom", "最大缩放", "number")));
      const items = h("div");
      const renderItems = () => {
        items.replaceChildren(h("h3", {}, "地图图层 · 从底到顶"));
        tree.items.forEach((item, index) => {
          const dataset = select([["", "选择已发布数据集"], ...datasets.map((record) => [record.key.id, record.key.id])], item.dataset);
          const style = select([["", "选择已发布样式"], ...styles.map((record) => [record.key.id, record.key.id])], item.style);
          for (const [control, key] of [[dataset, "dataset"], [style, "style"]]) {
            if (!Array.from(control.options).some((option) => option.value === item[key])) control.append(h("option", { value: item[key], selected: true }, `${item[key]}（未发布 / 不在选择器中）`));
            control.addEventListener("change", () => { item[key] = control.value; markTree(); });
          }
          const row = h("div", { className: "layer-row", draggable: true }, bound(item, "id", "图层 ID"), field("数据集", dataset), field("样式", style), h("div", { className: "layer-actions" }, button("↑", () => { [tree.items[index - 1], tree.items[index]] = [tree.items[index], tree.items[index - 1]]; markTree(); renderItems(); }, "", { disabled: index === 0, "aria-label": "上移图层" }), button("↓", () => { [tree.items[index + 1], tree.items[index]] = [tree.items[index], tree.items[index + 1]]; markTree(); renderItems(); }, "", { disabled: index === tree.items.length - 1, "aria-label": "下移图层" }), button("×", () => { tree.items.splice(index, 1); markTree(); renderItems(); }, "danger", { "aria-label": "删除图层" })), h("small", { className: "full" }, `动画绑定 ${(item.animation_bindings || []).length} · 几何绑定 ${(item.geometry_bindings || []).length}（完整对象保留，复杂绑定请编辑 TOML）`));
          row.addEventListener("dragstart", (event) => event.dataTransfer.setData("text/plain", String(index)));
          row.addEventListener("dragover", (event) => event.preventDefault());
          row.addEventListener("drop", (event) => {
            event.preventDefault();
            const from = Number(event.dataTransfer.getData("text/plain"));
            if (!Number.isInteger(from) || from < 0 || from >= tree.items.length) return;
            const [moved] = tree.items.splice(from, 1);
            tree.items.splice(index, 0, moved);
            markTree(); renderItems();
          });
          items.append(row);
        });
      };
      renderItems();
      auxiliary.append(items, button("添加图层", () => {
        tree.items.push({ id: `layer-${tree.items.length + 1}`, dataset: datasets[0]?.key.id || "user:dataset", style: styles[0]?.key.id || "user:style" });
        markTree(); renderItems();
      }));
    } else if (kind === "style") {
      for (const rule of tree.rules) {
        auxiliary.append(h("h3", {}, `规则 ${rule.id}`));
        for (const symbol of rule.symbols) {
          auxiliary.append(h("p", { className: "muted" }, `${symbol.id} · ${symbol.kind}`));
          for (const key of ["fill", "color", "field"]) if (typeof symbol[key] === "string") auxiliary.append(bound(symbol, key, key));
          for (const key of ["size", "width"]) if (symbol[key]?.value !== undefined) auxiliary.append(bound(symbol[key], "value", `${key} (${symbol[key].unit || "模型默认单位"})`, "number"));
        }
      }
    } else auxiliary.append(bound(tree, "duration", "动画时长（秒）", "number"), h("p", { className: "muted" }, `属性轨道 ${(tree.tracks || []).length} · 几何轨道 ${(tree.geometry_tracks || []).length}。关键帧与绑定请在 TOML 中编辑。`));
    auxiliary.append(button("应用表单到 TOML", guarded(async () => {
      if (!conversionConfirmed && !await confirmAction("转换配置格式？", "注释、空白与键顺序可能重排。完整语义树会保留。原文本可在当前页面恢复。")) return;
      const expectedRevision = revision;
      const expectedText = textarea.value;
      const response = await request(`admin/v1/configurations/${kind}/serialize`, { ...jsonOptions({ document: tree }), signal });
      if (revision !== expectedRevision || textarea.value !== expectedText) throw new Error("转换期间输入已变化，已保留较新的本地输入；请重新应用表单。");
      if (originalText === undefined) originalText = textarea.value;
      conversionConfirmed = true;
      textarea.value = response.data.text;
      tree = response.data.document;
      parsedText = textarea.value;
      treeDirty = false;
      dirty = true;
      editorState.textContent = "表单已转换为 TOML；请检查生成内容后保存。";
      renderForm();
    }), "primary"));
  };
  textarea.addEventListener("input", () => { revision += 1; dirty = true; tree = undefined; treeDirty = false; renderForm(); });
  const parse = guarded(async () => {
    if (treeDirty && !await confirmAction("重新载入表单？", "尚未应用的表单修改会丢弃；TOML 文本保持不变。")) return;
    const text = textarea.value;
    const expectedRevision = revision;
    const response = await request(`admin/v1/configurations/${kind}/parse`, { ...jsonOptions({ text }), signal });
    if (textarea.value !== text || expectedRevision !== revision) return;
    tree = response.data.document;
    parsedText = text;
    treeDirty = false;
    editorState.textContent = "配置语法与模型检查通过；目录依赖及渲染仍由最终发布验证。";
    renderForm();
  });
  const type = select([["area", "面"], ["line", "线"], ["point", "点"], ["raster", "栅格 RGB"], ["label", "注记（需字体资源）"]], "area", { "aria-label": "样式模板" });
  const editor = h("section", { className: "panel" }, h("h2", {}, "TOML 编辑器"), h("div", { className: "toolbar" }, kind === "style" && type, button("载入模板", guarded(async () => {
    if (!await confirmAction("用模板替换当前文本？", "只更改当前页面，不会创建任何临时资源。")) return;
    textarea.value = template(kind, identity.value, type.value);
    dirty = true; tree = undefined; treeDirty = false; renderForm();
  })), button("检查并载入表单", parse), button("恢复转换前文本", () => {
    if (originalText === undefined) { notify("尚未执行表单转换。"); return; }
    textarea.value = originalText; dirty = true; tree = undefined; treeDirty = false; renderForm();
  })), textarea, editorState, button("保存并发布", runWrite(async () => {
    if (treeDirty) throw new Error("辅助表单有未应用的修改，请先应用到 TOML");
    status.textContent = "正在提交配置，等待准备任务…";
    return request(`${currentPath()}/content`, { method: "PUT", headers: { ...condition(), "Content-Type": "application/toml" }, body: textarea.value });
  }), "primary"));
  content.append(h("div", { className: "editor-layout" }, editor, h("section", { className: "panel" }, auxiliary)));
  renderForm();
}

async function route(force = false) {
  const hash = location.hash || "#/overview";
  if (!force && dirty && hash !== lastHash && !window.confirm("当前页面有未保存修改，确定离开？")) {
    history.replaceState(null, "", lastHash);
    return;
  }
  dirty = false;
  lastHash = hash;
  scope.abort();
  for (const cleanup of cleanups.splice(0)) cleanup();
  scope = new AbortController();
  const signal = scope.signal;
  messages = h("div", { "aria-live": "polite" });
  main.removeAttribute("aria-busy");
  main.replaceChildren(messages);
  for (const anchor of document.querySelectorAll("nav a")) {
    const active = hash === anchor.hash || hash.startsWith(anchor.hash + "/");
    anchor.classList.toggle("active", active);
    if (active) anchor.setAttribute("aria-current", "page"); else anchor.removeAttribute("aria-current");
  }
  let parts;
  try { parts = hash.slice(2).split("/").map(decodeURIComponent); }
  catch { notify("页面地址编码无效，请从左侧导航重新进入。", "error"); return; }
  document.querySelector("#breadcrumb").textContent = `管理台 / ${names[parts[1]] || ({ overview: "服务概览", operations: "异步任务", service: "服务信息", preview: "地图预览" })[parts[0]] || "资源"}`;
  try {
    if (parts[0] === "overview") await overview();
    else if (parts[0] === "resources" && parts.length === 2) await resources(parts[1]);
    else if (parts[0] === "resources" && parts.length === 3) await detail(parts[1], parts[2]);
    else if (parts[0] === "operations") await operationPage();
    else if (parts[0] === "service") await servicePage();
    else if (parts[0] === "preview" && parts[1]) {
      const cleanup = await mountPreview({ id: parts[1], container: main, signal, h, field, input, select, button, head, notify, pageLimit, maxDimension: capabilities["max-dimension"] || 1600 });
      if (signal.aborted) cleanup(); else cleanups.push(cleanup);
    } else main.append(head("页面不存在", "请选择左侧导航中的页面。"));
  } catch (error) { if (!signal.aborted) report(error); }
  if (!signal.aborted) { main.focus({ preventScroll: true }); document.title = `${document.querySelector("#breadcrumb").textContent} · Geello`; }
}

window.addEventListener("hashchange", () => route());
window.addEventListener("beforeunload", (event) => { if (dirty) { event.preventDefault(); event.returnValue = ""; } });

async function start() {
  try {
    const { data } = await request("api");
    capabilities = data["x-geello-enabled"] || {};
    pageLimit = Math.max(1, Math.min(20, capabilities["max-list-items"] || 20));
    dispatcher.concurrency = Math.max(1, Math.min(2, capabilities["max-active-http"] || 1));
  } catch (error) {
    document.querySelector("#health").textContent = `能力发现失败：${error.message}`;
  }
  await route();
  poll(async () => {
    try {
      const { data } = await request("health/ready");
      document.querySelector("#health").textContent = `● 就绪 · ${data.backend}`;
    } catch (error) {
      document.querySelector("#health").textContent = "○ 连接中断 / 未就绪";
      throw error;
    }
  }, 10000);
}
start();
