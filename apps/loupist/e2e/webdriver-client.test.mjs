import assert from "node:assert/strict";
import { createServer } from "node:http";
import { once } from "node:events";
import test from "node:test";
import { remote } from "webdriverio";

// Exercise the pinned client's HTTP contract without opening a native window,
// downloading a browser, or executing a model request.
test("the Tauri client keeps main/child handles and structured denial evidence", { timeout: 10_000 }, async (t) => {
  const child = "browser-11111111-1111-4111-8111-111111111111";
  const handles = ["main", child];
  let selected = "main";
  let requestedCapabilities;
  const server = createServer(async (request, response) => {
    let input = "";
    for await (const chunk of request) input += chunk;
    const body = input ? JSON.parse(input) : {};
    let value;
    if (request.method === "POST" && request.url === "/session") {
      requestedCapabilities = body.capabilities.alwaysMatch;
      value = { sessionId: "client-contract", capabilities: { browserName: "tauri" } };
    } else if (request.method === "GET" && request.url === "/session/client-contract/window/handles") {
      value = handles;
    } else if (request.method === "GET" && request.url === "/session/client-contract/window") {
      value = selected;
    } else if (request.method === "POST" && request.url === "/session/client-contract/window") {
      assert.ok(handles.includes(body.handle));
      selected = body.handle;
      value = null;
    } else if (request.method === "GET" && request.url === "/session/client-contract/url") {
      value = selected === "main" ? "http://localhost:1420/" : "https://example.com/";
    } else if (request.method === "POST" && request.url === "/session/client-contract/execute/sync") {
      value = { apiPresent: true, allowed: false, denialMessage: "loupist_list not allowed. Plugin not found" };
    } else if (request.method === "DELETE" && request.url === "/session/client-contract") {
      value = null;
    } else {
      response.writeHead(404, { "content-type": "application/json" });
      response.end(JSON.stringify({ value: { error: "unknown command", message: request.url } }));
      return;
    }
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify({ value }));
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  let browser;
  t.after(async () => {
    try {
      await browser?.deleteSession();
    } finally {
      server.closeAllConnections();
      server.close();
    }
  });
  browser = await remote({
    protocol: "http",
    hostname: "127.0.0.1",
    port: server.address().port,
    path: "/",
    capabilities: { browserName: "tauri", "wdio:tauriServiceOptions": { windowLabel: "main" } },
    connectionRetryCount: 0,
    connectionRetryTimeout: 2_000,
    logLevel: "silent",
  });
  assert.equal(requestedCapabilities.browserName, "tauri");
  assert.deepEqual(requestedCapabilities["wdio:tauriServiceOptions"], { windowLabel: "main" });
  assert.deepEqual(await browser.getWindowHandles(), handles);
  await browser.switchToWindow(child);
  assert.equal(await browser.getUrl(), "https://example.com/");
  assert.deepEqual(await browser.execute(() => null), {
    apiPresent: true, allowed: false, denialMessage: "loupist_list not allowed. Plugin not found",
  });
  await browser.switchToWindow("main");
  assert.equal(await browser.getUrl(), "http://localhost:1420/");
});
