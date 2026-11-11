import assert from "node:assert/strict";
import test from "node:test";
import { runInNewContext } from "node:vm";
import { measureMapDrawProbe, measureMapProbe, openMapLibrary, readVisibleMapStats, selectLoupistTemplate } from "./native-shell.mjs";

test("map counts come from the visible map after routing, never a retained hidden view", async () => {
  const span = (textContent, { hidden = false, width = 200, visibility = "visible" } = {}) => ({
    textContent, visibility,
    closest: () => hidden ? {} : null,
    getClientRects: () => width ? [{}] : [],
  });
  let spans = [span("99 on the map · 88 relationships", { hidden: true }),
    span("77 on the map · 66 relationships", { width: 0 }),
    span("55 on the map · 44 relationships", { visibility: "hidden" }),
    span(" · 1 new this week"), span("34 on the map · 34 relationships")];
  const browser = { execute: async script => runInNewContext(`(${script})()`, {
    document: { querySelectorAll: selector => {
      assert.equal(selector, "[data-testid=map-view] [data-testid=map-stats] > span");
      return spans;
    } },
    getComputedStyle: element => ({ visibility: element.visibility }),
  }) };
  assert.equal(await readVisibleMapStats(browser), "34 on the map · 34 relationships");
  spans = [span("99 on the map · 88 relationships", { hidden: true }), span("Loading")];
  assert.equal(await readVisibleMapStats(browser), "");
});

const origin = "http://127.0.0.1:1430";
const frameSummary = "150 nodes · median 17.00 ms · p95 19.00 ms";

function mapHost({ text = frameSummary, width = 800, firstPaintMs = "12.00" } = {}) {
  return {
    querySelector: (selector) => selector === "canvas"
      ? { getBoundingClientRect: () => ({ width, height: 600 }) }
      : selector === "[data-testid=map-probe]" ? { textContent: text } : null,
    getAttribute: () => firstPaintMs,
  };
}

// Run the real browser callback against changing documents, as the native
// driver can acknowledge navigation before it replaces the current page.
function browserFixture(frames) {
  let requestedUrl;
  let reads = 0;
  const browser = {
    url: async (url) => { requestedUrl = url; },
    switchToWindow: async () => {},
    execute: async (script, ...args) => {
      const frame = frames[Math.min(reads++, frames.length - 1)];
      const host = mapHost(frame);
      const hidden = mapHost({ text: "1000 nodes · median 90.00 ms · p95 99.00 ms", width: 0 });
      return runInNewContext(`(${script})(...args)`, {
        args,
        location: { href: frame.url ?? requestedUrl },
        document: {
          documentElement: { dataset: { previewReady: frame.ready ?? "true", previewError: frame.error } },
          querySelector: (selector) => selector === '#workspace-panel[data-panel-kind="world"] [data-testid=map-view] [data-testid=map-canvas]' ? host : hidden,
        },
      });
    },
    waitUntil: async (condition, { timeoutMsg }) => {
      for (let attempt = 0; attempt < frames.length + 1; attempt++) if (await condition()) return;
      throw new Error(timeoutMsg);
    },
  };
  return { browser, reads: () => reads, url: () => requestedUrl };
}

test("frame probe waits through old document and unfinished render, then samples the visible Map together", async () => {
  const source = browserFixture([
    { url: `${origin}/preview.html?scenario=map-1000&probe=1`, text: "1000 nodes · median 21.00 ms · p95 26.00 ms" },
    { ready: "loading" },
    { text: "Measuring frame times…" },
    { width: 0 },
    { firstPaintMs: "18.25" },
  ]);
  const result = await measureMapProbe(source.browser, origin, { scenario: "map-local-2hops-bounded" });
  assert.equal(source.reads(), 5);
  assert.equal(result.nodes, 150);
  assert.equal(result.p95Ms, 19);
  assert.equal(result.canvas, true);
  assert.equal(result.firstPaintMs, "18.25");
  assert.equal(new URL(result.url).searchParams.get("scenario"), "map-local-2hops-bounded");
});

test("repeated same-scenario probes cannot accept the previous completed document", async () => {
  const first = browserFixture([{}]);
  const before = await measureMapProbe(first.browser, origin);
  const second = browserFixture([{ url: before.url, text: "1000 nodes · median 70.00 ms · p95 80.00 ms" }, {}]);
  const after = await measureMapProbe(second.browser, origin);
  assert.notEqual(after.url, before.url);
  assert.equal(second.reads(), 2);
  assert.equal(after.p95Ms, 19);
});

test("draw probe also waits for its own ready document and rendered canvas", async () => {
  const text = "1000 nodes · draw median 8.00 ms · draw p95 10.00 ms";
  const source = browserFixture([
    { url: `${origin}/preview.html?scenario=map-1000&probe=draw`, text },
    { ready: "loading", text },
    { text, width: 0 },
    { text },
  ]);
  const result = await measureMapDrawProbe(source.browser, origin);
  assert.equal(source.reads(), 4);
  assert.equal(result.kind, "draw-cost");
  assert.equal(result.p95Ms, 10);
  assert.equal(result.canvas, true);
});

test("a missing rendered canvas times out and a failed preview reports its reason", async () => {
  await assert.rejects(measureMapProbe(browserFixture([{ width: 0 }]).browser, origin), /rendered canvas/);
  await assert.rejects(measureMapProbe(browserFixture([{ ready: "error", error: "Missing landmark" }]).browser, origin), /Missing landmark/);
});

test("template selection waits for entry visibility and confirms selection after the click", async () => {
  let poll = 0;
  let clicks = 0;
  const browser = {
    $$: async () => [{
      isDisplayed: async () => poll > 1,
      isEnabled: async () => poll > 2,
      getText: async () => "Semiconductors\nChips, memory, foundries and equipment",
      getAttribute: async () => clicks && poll > 4 ? "true" : "false",
      click: async () => { clicks++; },
    }],
    waitUntil: async (condition, { timeoutMsg }) => {
      for (poll = 1; poll <= 6; poll++) if (await condition()) return;
      throw new Error(timeoutMsg);
    },
  };
  await selectLoupistTemplate(browser, "Semiconductors");
  assert.equal(clicks, 1);
  assert.equal(poll, 5);
});

test("an unselected template after a delivered click still fails", async () => {
  let clicks = 0;
  const browser = {
    $$: async () => [{
      isDisplayed: async () => true,
      isEnabled: async () => true,
      getText: async () => "Dividend income",
      getAttribute: async () => "false",
      click: async () => { clicks++; },
    }],
    waitUntil: async (condition, { timeoutMsg }) => {
      for (let poll = 0; poll < 3; poll++) if (await condition()) return;
      throw new Error(timeoutMsg);
    },
  };
  await assert.rejects(selectLoupistTemplate(browser, "Dividend income"), /did not become selected/);
  assert.equal(clicks, 1);
});

const { dismissModelMenu } = await import("./agent-driver.mjs");
test("a stuck model menu moves focus to the current Chat before retrying dismissal", async () => {
  let open = true;
  const clicked = [];
  const browser = {
    $$: async () => [{ isDisplayed: async () => open }],
    $: (selector) => ({
      isDisplayed: async () => selector !== "#main-tab-chat",
      isExisting: async () => selector !== "#main-tab-chat",
      getAttribute: async () => "true",
      click: async () => { clicked.push(selector); if (selector === "#main-panel-chat") open = false; },
    }),
    keys: async () => {},
    waitUntil: async (condition) => { if (!await condition()) throw new Error("Menu retained focus"); },
  };
  await dismissModelMenu(browser);
  assert.equal(open, false);
  assert(clicked.includes("#main-panel-chat"));
});

for (const mode of ["folded", "direct", "open"]) {
  test(`Map Library uses the visible ${mode} toolbar state`, async () => {
    let libraryOpen = mode === "open";
    let menuOpen = false;
    const clicks = [];
    const librarySelector = "[data-testid=map-view] aside[data-testid=map-library]";
    const directSelector = "[data-testid=map-view] button[data-map-library]:not([inert])";
    const moreSelector = "[data-testid=map-view] [data-map-more]";
    const itemSelector = '[role="menuitemcheckbox"][data-map-library]';
    const displayed = (selector) => selector === librarySelector ? libraryOpen
      : selector === directSelector ? mode === "direct"
        : selector === itemSelector ? menuOpen : selector === moreSelector;
    const elements = new Map([librarySelector, directSelector, moreSelector, itemSelector].map((selector) => [selector, {
      isDisplayed: async () => displayed(selector),
      getAttribute: async (name) => name === "aria-haspopup" && selector === moreSelector ? "menu"
        : name === "aria-expanded" && menuOpen ? "true" : null,
      waitForDisplayed: async () => assert(displayed(selector), `Still hidden: ${selector}`),
      click: async () => {
        assert(displayed(selector), `Clicked a hidden duplicate: ${selector}`);
        clicks.push(selector);
        if (selector === moreSelector) menuOpen = true;
        else libraryOpen = true;
      },
    }]));
    const browser = {
      $: async (selector) => {
        assert(elements.has(selector), `Unexpected selector: ${selector}`);
        return elements.get(selector);
      },
      execute: async () => "ready",
      waitUntil: async (condition) => { assert(await condition()); return true; },
    };
    assert.equal(await openMapLibrary(browser), elements.get(librarySelector));
    assert.equal(libraryOpen, true);
    assert.deepEqual(clicks, mode === "open" ? [] : mode === "direct" ? [directSelector] : [moreSelector, itemSelector]);
  });
}
