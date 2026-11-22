import assert from "node:assert/strict";
import { test } from "node:test";
import { Dispatcher, errorMessage, serviceURL } from "../src/server/admin/assets/requests.js";

const defer = () => {
  let resolve;
  const promise = new Promise((done) => { resolve = done; });
  return { promise, resolve };
};
const turn = () => new Promise((resolve) => setImmediate(resolve));

test("writes overtake queued reads and preview, without abort releasing a running slot", async () => {
  const dispatcher = new Dispatcher({ concurrency: 1 });
  const first = defer();
  const controller = new AbortController();
  const order = [];
  const active = dispatcher.run(async () => { order.push("active"); await first.promise; }, { priority: 2, signal: controller.signal });
  const rejection = assert.rejects(active, { name: "AbortError" });
  const preview = dispatcher.run(() => order.push("preview"), { priority: 2 });
  const read = dispatcher.run(() => order.push("read"));
  const write = dispatcher.run(() => order.push("write"), { priority: 0 });
  await turn();
  controller.abort();
  await turn();
  assert.deepEqual(order, ["active"]);
  assert.equal(dispatcher.active, 1);
  first.resolve();
  await Promise.all([rejection, preview, read, write]);
  assert.deepEqual(order, ["active", "write", "read", "preview"]);
});

test("preview concurrency remains one under a two-slot total", async () => {
  const dispatcher = new Dispatcher({ concurrency: 2 });
  const first = defer();
  const order = [];
  const preview = dispatcher.run(async () => { order.push("preview1"); await first.promise; }, { priority: 2 });
  const next = dispatcher.run(() => order.push("preview2"), { priority: 2 });
  const read = dispatcher.run(() => order.push("read"));
  await read;
  assert.deepEqual(order, ["preview1", "read"]);
  assert.equal(dispatcher.previewActive, 1);
  first.resolve();
  await Promise.all([preview, next]);
  assert.deepEqual(order, ["preview1", "read", "preview2"]);
});

test("bounded queues discard obsolete pending work and coalesce identical reads", async () => {
  const dispatcher = new Dispatcher({ capacity: 1 });
  const first = defer();
  const active = dispatcher.run(() => first.promise, { key: "first" });
  assert.equal(dispatcher.run(() => { throw new Error("duplicate"); }, { key: "first" }), active);
  const controller = new AbortController();
  let started = false;
  const pending = dispatcher.run(() => { started = true; }, { signal: controller.signal });
  const rejection = assert.rejects(pending, { name: "AbortError" });
  await assert.rejects(dispatcher.run(() => {}), /队列已满/);
  controller.abort();
  assert.equal(dispatcher.queue.length, 0);
  first.resolve();
  await Promise.all([active, rejection]);
  assert.equal(started, false);
});

test("errors handle JSON, XML, login HTML and preserve request IDs", () => {
  assert.match(errorMessage(503, '{"title":"capacity exhausted"}', "req-1"), /capacity exhausted.*req-1/);
  assert.match(errorMessage(503, '<ExceptionReport><ExceptionText>Busy</ExceptionText></ExceptionReport>'), /Busy/);
  assert.match(errorMessage(200, '<!doctype html><html>Login</html>'), /认证/);
  assert.throws(() => serviceURL("https://example.test/other"), /拒绝/);
});
