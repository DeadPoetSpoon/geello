export class Dispatcher {
  constructor({ concurrency = 1, previewConcurrency = 1, capacity = 64 } = {}) {
    this.concurrency = concurrency;
    this.previewConcurrency = previewConcurrency;
    this.capacity = capacity;
    this.active = 0;
    this.previewActive = 0;
    this.queue = [];
    this.reads = new Map();
  }

  run(work, { priority = 1, signal, key } = {}) {
    if (signal?.aborted) return Promise.reject(new DOMException("请求已过期", "AbortError"));
    if (key && this.reads.has(key)) return this.reads.get(key);
    if (this.queue.length >= this.capacity) return Promise.reject(new Error("请求队列已满，请停止移动地图后重试"));
    const result = new Promise((resolve, reject) => {
      const job = { work, priority, signal, resolve, reject, started: false };
      const cancel = () => {
        if (job.started) return;
        this.queue = this.queue.filter((candidate) => candidate !== job);
        reject(new DOMException("请求已过期", "AbortError"));
      };
      job.cleanup = () => signal?.removeEventListener("abort", cancel);
      signal?.addEventListener("abort", cancel, { once: true });
      this.queue.push(job);
      this.pump();
    });
    if (key) {
      this.reads.set(key, result);
      result.finally(() => this.reads.delete(key)).catch(() => {});
    }
    return result;
  }

  pump() {
    while (this.active < this.concurrency) {
      const candidates = this.queue.filter((job) => job.priority !== 2 || this.previewActive < this.previewConcurrency);
      candidates.sort((first, second) => first.priority - second.priority);
      const job = candidates[0];
      if (!job) return;
      this.queue.splice(this.queue.indexOf(job), 1);
      job.started = true;
      this.active += 1;
      if (job.priority === 2) this.previewActive += 1;
      Promise.resolve().then(job.work).then((value) => {
        if (job.signal?.aborted) job.reject(new DOMException("请求已过期", "AbortError"));
        else job.resolve(value);
      }, (error) => job.reject(job.signal?.aborted ? new DOMException("请求已过期", "AbortError") : error)).finally(() => {
        job.cleanup();
        this.active -= 1;
        if (job.priority === 2) this.previewActive -= 1;
        this.pump();
      });
    }
  }
}

export const dispatcher = new Dispatcher();
export function latestRequest() {
  let sequence = 0;
  return () => {
    const current = ++sequence;
    return () => current === sequence;
  };
}
export const serviceBase = new URL("../../", import.meta.url);

export function serviceURL(path) {
  const target = new URL(path, serviceBase);
  if (target.origin !== serviceBase.origin || !target.pathname.startsWith(serviceBase.pathname)) {
    throw new Error("拒绝访问服务范围之外的地址");
  }
  return target.href;
}

export function errorMessage(status, text, requestID) {
  let detail = text.slice(0, 1600);
  try {
    const problem = JSON.parse(text);
    detail = problem.title || problem.detail || detail;
    if (problem.references?.length) detail += " · 引用：" + problem.references.map((key) => `${key.kind}/${key.id}`).join("，");
  } catch {
    if (/<(?:\w+:)?(?:ExceptionReport|ServiceExceptionReport)\b/i.test(text)) {
      detail = text.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ").trim().slice(0, 1600);
    } else if (/<html|<!doctype/i.test(text)) {
      detail = "服务器返回登录页或 HTML 错误页，请检查反向代理认证";
    }
  }
  return `HTTP ${status}：${detail || "请求失败"}${requestID ? `（请求 ID：${requestID}）` : ""}`;
}

async function readResponse(response, mode) {
  const requestID = response.headers.get("x-request-id");
  if (!response.ok) throw Object.assign(new Error(errorMessage(response.status, await response.text(), requestID)), { status: response.status });
  if (mode === "blob") {
    const data = await response.blob();
    if (!data.type.startsWith("image/")) throw new Error(errorMessage(response.status, await data.text(), requestID));
    return { data, headers: response.headers, status: response.status };
  }
  const text = await response.text();
  let data = null;
  if (text) {
    try { data = JSON.parse(text); }
    catch { throw new Error(errorMessage(response.status, text, requestID)); }
  }
  return { data, headers: response.headers, status: response.status };
}

export function request(path, { method = "GET", body, headers = {}, signal, preview = false, mode = "json" } = {}) {
  const url = serviceURL(path);
  const priority = method === "GET" ? (preview ? 2 : 1) : 0;
  return dispatcher.run(async () => {
    const response = await fetch(url, { method, body, headers, credentials: "same-origin", redirect: "error" });
    return readResponse(response, mode);
  }, { priority, signal, key: method === "GET" && !signal ? `${mode}:${url}` : undefined });
}

export function upload(path, file, headers, progress) {
  return dispatcher.run(() => new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open("PUT", serviceURL(path));
    xhr.withCredentials = true;
    for (const [name, value] of Object.entries(headers)) xhr.setRequestHeader(name, value);
    xhr.upload.onprogress = (event) => progress(event.lengthComputable ? event.loaded / event.total : null);
    xhr.upload.onload = () => progress(1);
    xhr.onerror = () => reject(new Error("上传连接中断，请到任务页核对服务器状态后重试"));
    xhr.onload = () => {
      if (xhr.status < 200 || xhr.status >= 300) return reject(Object.assign(new Error(errorMessage(xhr.status, xhr.responseText, xhr.getResponseHeader("x-request-id"))), { status: xhr.status }));
      try { resolve({ data: JSON.parse(xhr.responseText), status: xhr.status }); }
      catch { reject(new Error("上传返回非 JSON，请检查认证并查看任务列表")); }
    };
    xhr.send(file);
  }), { priority: 0 });
}

export function poll(work, interval = 2000) {
  let closed = false;
  let timer;
  let failures = 0;
  const tick = async () => {
    if (closed) return;
    if (!document.hidden) {
      try { await work(); failures = 0; }
      catch { failures += 1; }
    }
    if (!closed) timer = setTimeout(tick, failures ? Math.min(30000, 2000 * 2 ** (failures - 1)) : interval);
  };
  tick();
  return () => { closed = true; clearTimeout(timer); };
}
