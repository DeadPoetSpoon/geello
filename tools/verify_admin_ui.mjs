import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdir, rm, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

const base = new URL(process.env.GEELLO_ADMIN_BASE || "http://127.0.0.1:3000/");
const debugURL = process.env.CHROME_DEBUG_URL || "http://127.0.0.1:9223";
const output = resolve(process.env.GEELLO_ADMIN_RESULTS || "assets/test/admin-ui/results");
const deviceScaleFactor = Number(process.env.GEELLO_ADMIN_DEVICE_SCALE_FACTOR || 1);
assert.ok(Number.isFinite(deviceScaleFactor) && deviceScaleFactor > 0);
await mkdir(output, { recursive: true });
const tabs = await (await fetch(`${debugURL}/json`)).json();
const tab = tabs.find((candidate) => candidate.type === "page");
assert.ok(tab, "Start headless Chromium with --remote-debugging-port=9223");
const socket = new WebSocket(tab.webSocketDebuggerUrl);
await new Promise((done) => { socket.onopen = done; });
let sequence = 0;
const callbacks = new Map();
const errors = [];
const requests = [];
const active = new Map();
let maxActive = 0;
let maxPreview = 0;
socket.onmessage = (event) => {
  const message = JSON.parse(event.data);
  if (message.id) {
    const callback = callbacks.get(message.id);
    callbacks.delete(message.id);
    if (message.error) callback?.reject(new Error(JSON.stringify(message.error)));
    else callback?.resolve(message.result);
  } else if (message.method === "Runtime.exceptionThrown") {
    errors.push(message.params.exceptionDetails.exception?.description || message.params.exceptionDetails.text);
  } else if (message.method === "Network.requestWillBeSent") {
    const { requestId, request, type } = message.params;
    if (request.url.startsWith(base.href) && ["Fetch", "XHR"].includes(type)) {
      const preview = /\/wmts\?|\/wms\?|\/map\?/.test(request.url);
      active.set(requestId, preview);
      maxActive = Math.max(maxActive, active.size);
      maxPreview = Math.max(maxPreview, [...active.values()].filter(Boolean).length);
      requests.push({ method: request.method, url: request.url });
    } else if (/^https?:/.test(request.url) && new URL(request.url).origin !== base.origin) {
      errors.push(`Unexpected external request: ${request.url}`);
    }
  } else if (message.method === "Network.responseReceived") {
    if (active.has(message.params.requestId) && message.params.response.status >= 400) {
      const response = message.params.response;
      const error = `HTTP ${response.status}: ${response.url}`;
      errors.push(error);
    }
  } else if (["Network.loadingFinished", "Network.loadingFailed"].includes(message.method)) {
    active.delete(message.params.requestId);
  }
};
function send(method, params = {}) {
  const id = ++sequence;
  return new Promise((resolve, reject) => {
    callbacks.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params }));
  });
}
async function evaluate(action, argument) {
  const { result, exceptionDetails } = await send("Runtime.evaluate", { expression: `(${action.toString()})(${JSON.stringify(argument)})`, awaitPromise: true, returnByValue: true });
  if (exceptionDetails) throw new Error(exceptionDetails.exception?.description || exceptionDetails.text);
  return result.value;
}
async function until(action, argument, timeout = 20000) {
  const started = Date.now();
  while (Date.now() - started < timeout) {
    if (await evaluate(action, argument)) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  throw new Error(`Browser wait timed out: ${action}\n${await evaluate(() => document.body.innerText)}`);
}
async function navigate(hash) {
  await evaluate(async (hash) => {
    location.hash = hash;
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  }, hash);
  await until(() => document.querySelector("main h1"));
}
async function setValue(selector, value) {
  await evaluate(({ selector, value }) => {
    const control = document.querySelector(selector);
    if (!control) throw new Error(`Missing control: ${selector}`);
    control.value = value;
    control.dispatchEvent(new Event("input", { bubbles: true }));
    control.dispatchEvent(new Event("change", { bubbles: true }));
  }, { selector, value });
}
async function click(text) {
  await evaluate((text) => {
    const control = [...document.querySelectorAll("main button")].find((candidate) => candidate.textContent === text);
    if (!control || control.disabled) throw new Error(`Missing or disabled button: ${text}`);
    control.click();
  }, text);
}
async function screenshot(name) {
  const { data } = await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: true });
  await writeFile(resolve(output, name), Buffer.from(data, "base64"));
}
async function accepted() {
  await until(() => document.querySelector("main")?.innerText.includes("加载已发布版本"));
  await click("加载已发布版本");
  await until(() => document.querySelector("main .editor-status")?.innerText.startsWith("编辑基线"));
}
const suffix = Date.now();
const ids = { dataset: `uitest:points-${suffix}`, style: `uitest:style-${suffix}`, map: `uitest:map-${suffix}`, animation: `uitest:motion-${suffix}`, font: `uitest:font-${suffix}` };
const created = [];
const checks = [];
try {
  await send("Page.enable");
  await send("Runtime.enable");
  await send("Network.enable");
  await send("Emulation.setDeviceMetricsOverride", { width: 1440, height: 1000, deviceScaleFactor, mobile: false });
  await send("Page.navigate", { url: new URL("admin/", base).href });
  await until(() => document.querySelector("main h1")?.textContent === "服务概览" && document.querySelector(".metrics"));
  const readiness = await (await fetch(new URL("health/ready", base))).json();
  checks.push(`Overview and embedded assets: ${readiness.backend}`);
  await screenshot("overview.png");

  const dataFile = resolve(output, "points.geojson");
  await writeFile(dataFile, JSON.stringify({ type: "FeatureCollection", features: [{ type: "Feature", id: "point", properties: { name: "Open data browser test", hostile: "<img src=x onerror=alert(1)>" }, geometry: { type: "Point", coordinates: [0, 0] } }] }));
  for (const kind of ["dataset", "font"]) {
    await navigate(`#/resources/${kind}/new`);
    await until(() => document.querySelector('main input[type="file"]'));
    await setValue("main input[pattern]", ids[kind]);
    const { root } = await send("DOM.getDocument");
    const { nodeId } = await send("DOM.querySelector", { nodeId: root.nodeId, selector: 'main input[type="file"]' });
    await send("DOM.setFileInputFiles", { nodeId, files: [kind === "dataset" ? dataFile : resolve("tests/fixtures/fonts/latin.ttf")] });
    await click("上传并发布");
    await accepted();
    created.push(kind);
    checks.push(`${kind} raw upload and asynchronous publication`);
  }
  const texts = {
    style: `schema_version=1\nid='${ids.style}'\n[[rules]]\nid='base'\n[[rules.symbols]]\nid='marker'\nkind='point'\nshape='circle'\nsize={value=12,unit='px'}\nfill='#087f83'\n`,
    animation: `schema_version=1\nid='${ids.animation}'\nduration=2.0\n[[geometry_tracks]]\nid='position'\nsource_crs={definition='OGC:CRS84',space='GeographicXy'}\ninterpolation='linear'\ntrajectory={anchor=[0.0,0.0],points=[[0.0,0.0],[1.0,1.0]],timing='distance'}\n`,
    map: `schema_version=1\nid='${ids.map}'\ntitle='Browser test map'\noutput_crs=['OGC:CRS84','EPSG:3857']\ntiles={matrix_sets=['WebMercatorQuad','WorldCRS84Quad'],min_zoom=0,max_zoom=8}\n[[items]]\nid='points'\ndataset='${ids.dataset}'\nstyle='${ids.style}'\n[[items.geometry_bindings]]\nid='move'\nanimation='${ids.animation}'\ntrack='position'\nfeature_ids=['point']\nmax_features=1\nstart=0.0\nspeed=1.0\nrepeat='once'\n`,
  };
  for (const kind of ["style", "animation", "map"]) {
    await navigate(`#/resources/${kind}/new`);
    await until(() => document.querySelector("main textarea"));
    await setValue("main input[pattern]", ids[kind]);
    await setValue("main textarea", texts[kind]);
    await click("检查并载入表单");
    await until(() => document.querySelector("main")?.innerText.includes("配置语法与模型检查通过"));
    if (kind === "map") {
      assert.ok(await evaluate(() => document.querySelector("main")?.innerText.includes("几何绑定 1")));
      await click("应用表单到 TOML");
      await until(() => document.querySelector("dialog[open]"));
      await evaluate(() => [...document.querySelectorAll("dialog button")].find((button) => button.textContent === "确认操作").click());
      await until(() => document.querySelector("main")?.innerText.includes("表单已转换"));
      assert.match(await evaluate(() => document.querySelector("textarea").value), /geometry_bindings/);
      await screenshot("map-editor.png");
    }
    await click("保存并发布");
    await accepted();
    created.push(kind);
    checks.push(`${kind} TOML validation, full-tree form conversion and publication`);
  }

  await navigate("#/resources/map");
  await until(() => document.querySelector('input[aria-label="资源 ID 关键字"]'));
  await setValue('input[aria-label="资源 ID 关键字"]', ids.map);
  await evaluate(() => document.querySelector("main form").requestSubmit());
  await until((id) => document.querySelector("tbody")?.textContent.includes(id) && document.querySelectorAll("tbody tr").length === 1, ids.map);
  checks.push("Resource UI filtering and bounded result table");
  await screenshot("resources.png");
  await navigate("#/operations");
  await until(() => document.querySelector('input[aria-label="任务资源 ID"]'));
  await setValue('input[aria-label="任务资源 ID"]', ids.map);
  await evaluate(() => document.querySelector("main form").requestSubmit());
  await until((id) => document.querySelector("tbody")?.textContent.includes(id) && document.querySelectorAll("tbody tr").length === 1, ids.map);
  checks.push("Task UI filtering and real terminal state");
  await navigate("#/service");
  await until(() => document.querySelector("main pre")?.textContent.includes("actual-backend"));
  checks.push("Read-only service capabilities UI");

  await navigate(`#/preview/${encodeURIComponent(ids.map)}`);
  await until(() => document.querySelector(".map canvas"));
  await until(() => document.querySelector(".map-note")?.textContent.includes("正式服务图像已加载"), null, 40000);
  await screenshot("wmts-preview.png");
  checks.push("Published map WMTS preview on actual server backend");
  await evaluate(() => {
    const control = document.querySelectorAll(".preview-toolbar select")[0];
    control.value = "ogc"; control.dispatchEvent(new Event("change"));
  });
  await until(() => document.querySelector(".map-note")?.textContent.includes("正式服务图像已加载"));
  checks.push("OGC Maps image preview");
  const markerPixels = () => {
    const canvas = document.querySelector(".map canvas");
    if (!canvas) return;
    const pixels = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
    let minimum = canvas.width;
    let maximum = -1;
    for (let offset = 0; offset < pixels.length; offset += 4) {
      if (Math.abs(pixels[offset] - 8) > 5 || Math.abs(pixels[offset + 1] - 127) > 5 || Math.abs(pixels[offset + 2] - 131) > 5 || pixels[offset + 3] < 200) continue;
      const column = offset / 4 % canvas.width;
      minimum = Math.min(minimum, column);
      maximum = Math.max(maximum, column);
    }
    return maximum >= minimum ? maximum - minimum + 1 : undefined;
  };
  await until(markerPixels);
  const markerWidth = await evaluate(markerPixels);
  const expectedMarkerWidth = 12 * deviceScaleFactor;
  assert.ok(Math.abs(markerWidth - expectedMarkerWidth) <= 3 * deviceScaleFactor, `Image placement at DPR=${deviceScaleFactor}: marker width ${markerWidth}, expected ${expectedMarkerWidth}`);
  const imageRequest = new URL(requests.findLast((entry) => entry.url.includes("/map?")).url);
  assert.equal(Number(imageRequest.searchParams.get("geello_pixel_ratio")), Math.min(2, deviceScaleFactor));
  checks.push(`OGC Maps DPR=${deviceScaleFactor}: native marker width ${markerWidth}px without placement stretching`);
  await evaluate(() => {
    const controls = document.querySelectorAll(".preview-toolbar select");
    controls[0].value = "wms"; controls[1].value = "OGC:CRS84";
    controls[0].dispatchEvent(new Event("change"));
  });
  await until(() => document.querySelector(".map-note")?.textContent.includes("正式服务图像已加载"));
  await setValue('.preview-toolbar input[type="number"]', "1");
  await click("应用时间");
  await until(() => document.querySelector(".map-note")?.textContent.includes("正式服务图像已加载"));
  checks.push("WMS CRS84 XY axis handling and explicit animation time");
  await until(() => document.querySelector('select[aria-label="属性检查数据集"]')?.options.length > 1);
  await setValue('select[aria-label="属性检查数据集"]', ids.dataset);
  await click("查询当前范围");
  await until(() => document.querySelector(".feature-result")?.textContent.includes("1 个要素"));
  assert.equal(await evaluate(() => document.querySelectorAll("main img[src='x']").length), 0);
  checks.push("Bounded Features highlight with hostile property treated as text");
  await screenshot("wms-features.png");

  for (let index = 0; index < 12; index += 1) {
    await evaluate((index) => {
      const control = document.querySelectorAll(".preview-toolbar select")[0];
      control.value = index % 2 ? "wmts" : "ogc";
      control.dispatchEvent(new Event("change"));
    }, index);
  }
  await until(() => document.querySelector(".map-note")?.textContent.includes("正式服务图像已加载"), null, 40000);
  assert.ok(maxActive <= 2, `HTTP peak ${maxActive}`);
  assert.ok(maxPreview <= 1, `Preview peak ${maxPreview}`);
  checks.push(`Continuous protocol switching: active HTTP peak ${maxActive}, preview peak ${maxPreview}`);

  for (const recovery of ["定位范围", "全图"]) {
    await evaluate(() => {
      const originalFetch = window.fetch;
      window.__geelloRestoreFetch = () => { window.fetch = originalFetch; };
      window.__geelloInjectedFailures = 0;
      window.fetch = function (url, options) {
        if (/\/map\?/.test(String(url))) {
          window.__geelloInjectedFailures += 1;
          return Promise.resolve(new Response(JSON.stringify({ detail: "Injected preview failure" }), { status: 422, headers: { "Content-Type": "application/problem+json" } }));
        }
        return originalFetch.call(this, url, options);
      };
      const protocol = document.querySelectorAll(".preview-toolbar select")[0];
      protocol.value = "ogc";
      protocol.dispatchEvent(new Event("change"));
    });
    await until(() => document.querySelector("main")?.textContent.includes("Injected preview failure"));
    const injectedFailures = await evaluate(() => window.__geelloInjectedFailures);
    await new Promise((done) => setTimeout(done, 600));
    assert.ok(await evaluate(() => document.querySelector(".map-note")?.textContent.includes("预览请求失败")), "Failed preview must not automatically retry");
    assert.equal(await evaluate(() => window.__geelloInjectedFailures), injectedFailures);
    await evaluate(() => { window.__geelloRestoreFetch(); delete window.__geelloRestoreFetch; delete window.__geelloInjectedFailures; });
    if (recovery === "定位范围") {
      await evaluate(() => {
        const extent = [...document.querySelectorAll("main input")].find((candidate) => candidate.placeholder === "minX,minY,maxX,maxY");
        extent.value = "-1000000,-1000000,1000000,1000000";
      });
    }
    await click(recovery);
    await until(() => document.querySelector(".map-note")?.textContent.includes("正式服务图像已加载"), null, 40000);
    checks.push(`Failed preview stays stopped; explicit ${recovery} recovers without a page reload`);
  }

  const openMaps = (process.env.GEELLO_OPEN_DATA_MAPS || "").split(",").filter(Boolean);
  for (const mapID of openMaps) {
    await navigate(`#/preview/${encodeURIComponent(mapID)}`);
    await until(() => document.querySelector(".preview-toolbar"));
    await until(() => document.querySelector(".map-note")?.textContent.includes("正式服务图像已加载"), null, 60000);
    await screenshot(`open-${mapID.replace(/[^a-z0-9-]/gi, "-")}-world.png`);
    checks.push(`Open-data initial world WMTS preview without HTTP errors: ${mapID}`);
    if (mapID === "open:landsat") {
      for (const protocol of ["ogc", "wms"]) {
        await evaluate((protocol) => {
          const control = document.querySelectorAll(".preview-toolbar select")[0];
          control.value = protocol;
          control.dispatchEvent(new Event("change"));
        }, protocol);
        await until(() => document.querySelector(".map-note")?.textContent.includes("正式服务图像已加载"), null, 60000);
      }
      checks.push("Local UTM imagery: OGC Maps and WMS world views without HTTP errors");
      await evaluate(() => {
        const controls = document.querySelectorAll(".preview-toolbar select");
        controls[0].value = "ogc";
        controls[1].value = "OGC:CRS84";
        controls[0].dispatchEvent(new Event("change"));
        const extent = [...document.querySelectorAll("main input")].find((candidate) => candidate.placeholder === "minX,minY,maxX,maxY");
        extent.value = "-79.1,23.4,-76.4,25.7";
        [...document.querySelectorAll("main button")].find((candidate) => candidate.textContent === "定位范围").click();
      });
    }
    await until(() => document.querySelector(".map-note")?.textContent.includes("正式服务图像已加载"), null, 60000);
    await screenshot(`open-${mapID.replace(/[^a-z0-9-]/gi, "-")}.png`);
    if (mapID === "open:landsat") checks.push("Local UTM imagery: visible OGC Maps preview at manual imagery extent");
  }

  await send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 1, mobile: true });
  await screenshot("mobile-preview.png");
  assert.ok(await evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), "Mobile page overflows horizontally");
  checks.push("390px responsive layout without horizontal page overflow");
  assert.deepEqual(errors, []);
  const gitRevision = execFileSync("git", ["rev-parse", "HEAD"], { encoding: "utf8" }).trim();
  const worktreeDirty = Boolean(execFileSync("git", ["status", "--porcelain"], { encoding: "utf8" }).trim());
  const { "x-geello-enabled": capabilities } = await (await fetch(new URL("api", base))).json();
  const report = { timestamp: new Date().toISOString(), gitRevision, worktreeDirty, serverConfiguration: process.env.GEELLO_SERVER_CONFIG || null, capabilities, base: base.href, backend: readiness.backend, browser: await send("Browser.getVersion"), checks, maxActive, maxPreview, errors, requests };
  await writeFile(resolve(output, "report.json"), JSON.stringify(report, null, 2));
  await Promise.all(["failure.json", "failure.png"].map((name) => rm(resolve(output, name), { force: true })));
  console.log(JSON.stringify({ backend: report.backend, checks, maxActive, maxPreview }, null, 2));
} catch (error) {
  await screenshot("failure.png").catch(() => {});
  await writeFile(resolve(output, "failure.json"), JSON.stringify({ message: error.stack, errors, checks, requests }, null, 2));
  throw error;
} finally {
  await send("Page.navigate", { url: "about:blank" }).catch(() => {});
  for (const kind of created.reverse()) {
    const path = new URL(`admin/v1/resources/${kind}/${encodeURIComponent(ids[kind])}`, base);
    const current = await fetch(path);
    if (current.ok) {
      await current.arrayBuffer();
      const removed = await fetch(`${path}/content`, { method: "DELETE", headers: { "If-Match": current.headers.get("etag") } });
      await removed.arrayBuffer();
      if (!removed.ok) console.error(`Cleanup failed for ${kind}/${ids[kind]}: ${removed.status}`);
    }
  }
  socket.close();
}
