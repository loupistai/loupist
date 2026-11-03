import assert from "node:assert/strict";
import { createServer } from "node:http";

// A truthy top-level `error` on an execute/sync result is a WebDriver protocol
// failure (`[object Object]`). Nest the probe and unwrap it in Node.
export function browserProbeEnvelope() {
  const probe = window.__nativeBrowserProbe;
  return probe ? { probe: { pending: probe.pending, result: probe.result, error: probe.error, events: probe.events } } : null;
}

async function readProbe(browser) {
  const envelope = await browser.execute(browserProbeEnvelope);
  return envelope ? envelope.probe : null;
}

async function settledProbe(browser) {
  let state;
  await browser.waitUntil(async () => {
    state = await readProbe(browser);
    return state && !state.pending;
  }, { timeout: 20_000, interval: 100, timeoutMsg: "Native browser command did not settle" });
  return state;
}

async function command(browser, method, ...args) {
  await browser.execute((operation, values) => {
    const probe = window.__nativeBrowserProbe;
    probe.pending = true;
    probe.result = null;
    probe.error = null;
    probe.bridge[operation](...values)
      .then((result) => { probe.result = result ?? null; })
      .catch((cause) => { probe.error = { code: cause.code ?? "unknown", message: cause.message ?? String(cause) }; })
      .finally(() => { probe.pending = false; });
  }, method, args);
  return settledProbe(browser);
}

/** Exercise the real child WebView and Rust policy without credentials or model calls. */
export async function verifyNativeBrowserNavigation(browser) {
  const requests = new Set();
  const server = createServer((request, response) => {
    const path = new URL(request.url, "http://fixture.test").pathname;
    requests.add(path);
    if (path === "/start") {
      response.writeHead(302, { location: "/article" });
      response.end();
      return;
    }
    response.setHeader("Content-Type", "text/html; charset=utf-8");
    response.setHeader("Cache-Control", "no-store");
    if (path === "/article") {
      response.end(`<!doctype html><title>Main research article</title><h1>Research article</h1>
        <script>const placeholder = document.createElement('iframe'); placeholder.src = 'about:blank';
          placeholder.title = 'Analytics placeholder'; placeholder.hidden = true; document.body.append(placeholder);</script>
        <iframe title="Embedded research" src="/embedded" onload="fetch('/frames-ready')"></iframe>`);
    } else if (path === "/embedded") {
      response.end("<!doctype html><title>Embedded document</title><p>HTTP subframe</p>");
    } else {
      response.end("<!doctype html><title>Second research article</title><h1>Second article</h1>");
    }
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const origin = `http://127.0.0.1:${server.address().port}`;
  let tabId;
  try {
    await browser.execute((url) => {
      const probe = window.__nativeBrowserProbe = { pending: true, result: null, error: null, events: [] };
      import("/src/bridge/tauriBridge.ts")
        .then(({ createLoupistBridge }) => {
          probe.bridge = createLoupistBridge();
          return probe.bridge.browserTabOpen({ url, visible: false, bounds: { x: 0, y: 0, width: 640, height: 400 } },
            (event) => { probe.events.push(event); });
        })
        .then((result) => { probe.result = result; })
        .catch((cause) => { probe.error = { code: cause.code ?? "unknown", message: cause.message ?? String(cause) }; })
        .finally(() => { probe.pending = false; });
    }, `${origin}/start`);
    const opened = await settledProbe(browser);
    assert.equal(opened.error, null, JSON.stringify(opened.error));
    tabId = opened.result.tab_id;
    let state;
    await browser.waitUntil(async () => {
      state = await readProbe(browser);
      return requests.has("/frames-ready") && state.events.some((event) => event.type === "load_finished" && event.url === `${origin}/article`);
    }, { timeout: 20_000, interval: 100, timeoutMsg: "Article, redirect and HTTP subframe did not finish" });
    assert.equal(state.events.some((event) => event.type === "navigation_blocked"), false,
      `An internal blank frame was reported as a page failure: ${JSON.stringify(state.events)}`);
    const mainLoads = state.events.filter((event) => event.type === "load_started" || event.type === "load_finished");
    assert.ok(mainLoads.every((event) => event.url === `${origin}/article`), `A subframe changed the main address: ${JSON.stringify(mainLoads)}`);
    assert.ok(state.events.some((event) => event.type === "title_changed" && event.title === "Main research article"));
    assert.equal(state.events.some((event) => event.type === "title_changed" && event.title === "Embedded document"), false);

    const denied = [];
    for (const [url, code] of [
      ["about:blank", "browser_url_unsupported_protocol"],
      ["about:srcdoc", "browser_url_unsupported_protocol"],
      ["file:///tmp/loupist-browser-policy-test", "browser_url_unsupported_protocol"],
      ["javascript:alert(1)", "browser_url_unsupported_protocol"],
      ["data:text/html,test", "browser_url_unsupported_protocol"],
      ["https://user:secret@example.com/report", "browser_url_credentials"],
    ]) {
      const result = await command(browser, "browserTabNavigate", tabId, url);
      assert.equal(result.error?.code, code, `Unsafe navigation was not rejected: ${url}`);
      denied.push({ scheme: new URL(url).protocol, code });
    }
    const finishedBefore = state.events.filter((event) => event.type === "load_finished").length;
    assert.equal((await command(browser, "browserTabReload", tabId)).error, null);
    await browser.waitUntil(async () => {
      state = await readProbe(browser);
      return state.events.filter((event) => event.type === "load_finished").length > finishedBefore;
    }, { timeout: 20_000, interval: 100, timeoutMsg: "The valid article did not reload after denied navigation" });
    assert.equal(state.events.filter((event) => event.type === "load_finished").at(-1).url, `${origin}/article`);
    assert.equal((await command(browser, "browserTabNavigate", tabId, `${origin}/second`)).error, null);
    await browser.waitUntil(async () => {
      state = await readProbe(browser);
      return state.events.some((event) => event.type === "load_finished" && event.url === `${origin}/second`);
    }, { timeout: 20_000, interval: 100, timeoutMsg: "The next valid article did not load" });
    assert.equal(state.events.some((event) => event.type === "navigation_blocked"), false);
    return { redirect: true, blankFrameQuietlyDenied: true, httpSubframeKeptMainAddress: true, denied, reload: true, subsequentNavigation: true };
  } finally {
    try {
      if (tabId) {
        const closed = await command(browser, "browserTabClose", tabId);
        assert.equal(closed.error, null, "The native browser fixture did not close");
      }
    } finally {
      try {
        await browser.execute(() => { delete window.__nativeBrowserProbe; });
      } finally {
        server.closeAllConnections();
        await new Promise((resolve) => server.close(resolve));
      }
    }
  }
}
