import assert from "node:assert/strict";
import test from "node:test";
import { runInNewContext } from "node:vm";
import { measureMapDrawProbe, measureMapProbe, openComposerModelMenu, openMapLibrary, openCustomConnections, openLoupistSwitcher, openMainTab, readVisibleMapStats, selectLoupistTemplate, switcherLoupists, waitForNoLayers, waitForRenderedButton } from "./native-shell.mjs";

test("reopening the Loupist switcher activates its trigger during menu exit", async () => {
  let expanded = false;
  let clicks = 0;
  const trigger = "[data-testid=loupist-header-identity]";
  const browser = {
    execute: async (callback, selector) => {
      if (selector === trigger) return "ready";
      assert.equal(selector, '.loupist-switcher-menu[data-state="open"]');
      return runInNewContext(`(${callback})(selector)`, {
        selector,
        document: { querySelector: () => expanded ? { getClientRects: () => [{}] } : null },
        getComputedStyle: () => ({ visibility: "visible" }),
      });
    },
    $: selector => selector === trigger ? {
      getAttribute: async name => name === "aria-haspopup" ? "menu" : String(expanded),
      isDisplayed: async () => true,
      click: async () => { clicks++; expanded = true; },
    } : {
      // The WebView renders the portal even when WebDriver says it is hidden.
      waitForDisplayed: async () => assert.fail("The driver's visibility does not describe this menu"),
    },
    waitUntil: async condition => {
      assert(await condition());
      return true;
    },
  };
  await openLoupistSwitcher(browser);
  await openLoupistSwitcher(browser);
  assert.equal(clicks, 1);
});

test("reading the switcher waits for menu focus and verifies Escape dismissed it", async () => {
  const trigger = { getAttribute: async () => "true" };
  let activeElement = trigger;
  let open = true;
  let escapeEvents = 0;
  const option = {
    getAttribute: name => name === "data-loupist-option" ? "folder-id" : null,
    querySelector: () => ({ textContent: "Folder" }),
    dispatchEvent: event => {
      assert.deepEqual([event.type, event.key, event.bubbles, event.cancelable], ["keydown", "Escape", true, true]);
      escapeEvents++;
      if (escapeEvents === 1) return true;
      open = false;
      return false;
    },
  };
  const menu = {
    getClientRects: () => [{}],
    contains: element => element === option,
  };
  class KeyboardEvent {
    constructor(type, props) { Object.assign(this, props, { type }); }
  }
  const browser = {
    execute: async (callback, selector) => runInNewContext(`(${callback})(selector)`, {
      selector,
      document: {
        activeElement,
        querySelector: () => open ? menu : null,
        querySelectorAll: () => open ? [option] : [],
      },
      getComputedStyle: () => ({ visibility: "visible" }),
      KeyboardEvent,
    }),
    $: selector => selector === "[data-testid=loupist-header-identity]" ? trigger : {
      waitForExist: async ({ reverse }) => {
        assert.ok(reverse);
        assert.equal(open, false);
      },
    },
    waitUntil: async condition => {
      if (await condition()) return;
      assert.equal(escapeEvents, 0, "Escape must wait for focus to enter the menu");
      activeElement = option;
      assert.equal(await condition(), false, "A key event alone does not prove the menu closed");
      assert.equal(await condition(), false, "Wait for the closed state after Escape is handled");
      assert.ok(await condition());
    },
    keys: async () => assert.fail("Escape must be dispatched from the focused menu element in the same WebView call"),
  };
  const [listed] = await switcherLoupists(browser);
  assert.equal(listed.id, "folder-id");
  assert.equal(listed.name, "Folder");
  assert.equal(escapeEvents, 2);
});

test("an Open button must be rendered and enabled in the requested card", async () => {
  let scrolled = 0;
  const target = {
    textContent: " Open ",
    disabled: false,
    getClientRects: () => [{}],
    scrollIntoView: () => { scrolled++; },
  };
  const buttons = [{ ...target, disabled: true }, { ...target, textContent: "Stop" }];
  const browser = {
    execute: async (callback, selector, name) => runInNewContext(`(${callback})(selector, name)`, {
      selector,
      name,
      document: { querySelector: root => {
        assert.equal(root, "#stopped-card");
        return { closest: () => null, querySelectorAll: () => buttons };
      } },
      getComputedStyle: () => ({ visibility: "visible" }),
    }),
    waitUntil: async condition => {
      assert.equal(await condition(), false);
      buttons.push(target);
      assert.equal(await condition(), true);
    },
  };
  await waitForRenderedButton(browser, "#stopped-card", "Open");
  assert.equal(scrolled, 1);
});

test("a lingering tooltip does not hold the composer after a menu closes", async () => {
  const overlay = (role, tooltip = false) => ({
    getClientRects: () => [{}],
    closest: () => null,
    matches: () => role === null,
    querySelector: selector => tooltip && selector === '[data-slot="tooltip-content"]' ? {} : null,
    getAttribute: () => role,
  });
  const tooltip = overlay(null, true);
  let layers = [tooltip, overlay("menu")];
  const browser = {
    execute: async callback => runInNewContext(`(${callback})()`, {
      document: { querySelectorAll: () => layers },
    }),
    waitUntil: async condition => {
      assert.equal(await condition(), false);
      layers = [tooltip];
      assert.equal(await condition(), true);
    },
  };
  await waitForNoLayers(browser);
});

test("the model picker opens by keyboard when WebKit delivers a click without pointerdown", async () => {
  const selector = '[data-testid="composer-model-menu"]';
  let focused = false;
  let expanded = false;
  let clicks = 0;
  class Trigger {
    closest() { return null; }
    matches() { return false; }
    focus() { focused = true; }
    async click() { clicks++; }
    async isDisplayed() { return true; }
    async waitForDisplayed() {}
    async waitForEnabled() {}
    async getAttribute(name) {
      return name === "aria-haspopup" ? "dialog" : String(expanded);
    }
  }
  const trigger = new Trigger();
  const browser = {
    $: () => trigger,
    execute: async (script, sel) => runInNewContext(`(${script})(selector)`, {
      selector: sel,
      document: { querySelector: () => trigger },
      HTMLElement: Trigger,
    }),
    keys: async key => {
      assert.equal(key, "Enter");
      assert.ok(focused);
      expanded = true;
    },
    waitUntil: async condition => {
      assert(await condition());
      return true;
    },
  };
  await openComposerModelMenu(browser, selector);
  assert.ok(expanded);
  await openComposerModelMenu(browser, selector);
  assert.equal(clicks, 1, "An open picker must stay open");
});

test("map counts come from the visible map after routing, never a retained hidden view", async () => {
  const span = (textContent, { hidden = false, width = 200, visibility = "visible" } = {}) => ({
    textContent, visibility,
    closest: () => hidden ? {} : null,
    getClientRects: () => width ? [{}] : [],
  });
  let spans = [span("99 names · 88 relationships", { hidden: true }),
    span("77 names · 66 relationships", { width: 0 }),
    span("55 names · 44 relationships", { visibility: "hidden" }),
    span("1 new name this week"), span("35 names · 38 relationships")];
  const browser = { execute: async script => runInNewContext(`(${script})()`, {
    document: { querySelectorAll: selector => {
      assert.equal(selector, "[data-testid=map-view] [data-testid=map-stats] > span");
      return spans;
    } },
    getComputedStyle: element => ({ visibility: element.visibility }),
  }) };
  assert.equal(await readVisibleMapStats(browser), "35 names · 38 relationships");

  for (const [status, expected] of [
    ["1 name · 1 relationship · 1 new name this week", "1 name · 1 relationship"],
    ["1,375 names · 1,588 relationships · 12 new names this week · 3 disagreements", "1,375 names · 1,588 relationships"],
    ["Showing 991 of 1,375 names · 1,588 relationships · 3 disagreements", "Showing 991 of 1,375 names · 1,588 relationships"],
    ["5 of 35 in World around Sealed Hub", ""],
    ["Loading", ""],
  ]) {
    spans = [span("99 names · 88 relationships", { hidden: true }), span(status)];
    assert.equal(await readVisibleMapStats(browser), expected, status);
  }
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
      const value = runInNewContext(`(${script})(...args)`, {
        args,
        location: { href: frame.url ?? requestedUrl },
        document: {
          documentElement: { dataset: { previewReady: frame.ready ?? "true", previewError: frame.error } },
          querySelector: (selector) => selector === '#main-panel-map [data-testid=map-view] [data-testid=map-canvas]' ? host : hidden,
        },
      });
      // The native driver treats a top-level error field as a script error.
      if (value?.error) throw new Error("WebDriver script error");
      return value;
    },
    waitUntil: async (condition, { timeoutMsg }) => {
      for (let attempt = 0; attempt < frames.length + 1; attempt++) if (await condition()) return;
      throw new Error(timeoutMsg);
    },
  };
  return { browser, reads: () => reads, url: () => requestedUrl };
}

test("frame probe waits through old document and unfinished render, then samples the visible World together", async () => {
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

for (const initiallyOpen of [false, true]) {
  test(`World Library ${initiallyOpen ? "keeps its open state" : "opens from the persistent toolbar button"}`, async () => {
    let libraryOpen = initiallyOpen;
    const clicks = [];
    const librarySelector = "[data-testid=map-view] aside[data-testid=map-library]";
    const buttonSelector = "[data-testid=map-view] button[data-map-library]";
    const elements = new Map([librarySelector, buttonSelector].map(selector => [selector, {
      isDisplayed: async () => selector === buttonSelector || libraryOpen,
      waitForDisplayed: async () => assert(selector === buttonSelector || libraryOpen),
      click: async () => { clicks.push(selector); libraryOpen = true; },
    }]));
    const browser = { $: async selector => {
      assert(elements.has(selector), `Unexpected selector: ${selector}`);
      return elements.get(selector);
    } };
    assert.equal(await openMapLibrary(browser), elements.get(librarySelector));
    assert.deepEqual(clicks, initiallyOpen ? [] : [buttonSelector]);
  });

  test(`Custom connections ${initiallyOpen ? "stays open" : "opens its disclosure"}`, async () => {
    let open = initiallyOpen;
    let clicks = 0;
    const browser = { $: async selector => {
      assert.equal(selector, "[data-custom-connections]");
      return {
        getAttribute: async name => { assert.equal(name, "open"); return open ? "" : null; },
        $: async child => {
          assert.equal(child, ":scope > summary");
          return { click: async () => { open = !open; clicks++; } };
        },
      };
    } };
    await openCustomConnections(browser);
    assert(open);
    assert.equal(clicks, initiallyOpen ? 0 : 1);
  });
}

for (const place of ["following", "map", "lens", "schedule"]) {
  test(`sidebar navigation opens ${place} through its destination row`, async () => {
    let opened = false;
    const selected = `[data-sidebar-place="${place}"] button`;
    const destination = place === "map" ? "[data-testid=map-view]" : place === "lens" ? "[data-testid=memory-page]" : `#main-panel-${place}`;
    const clicks = [];
    const browser = {
      execute: async (_callback, selector) => { assert.equal(selector, selected); return "ready"; },
      $: selector => ({
        isDisplayed: async () => selector === selected || (selector === destination && opened),
        getAttribute: async () => null,
        click: async () => { clicks.push(selector); opened = true; },
        waitForDisplayed: async () => { assert.equal(selector, destination); assert(opened); },
      }),
    };
    await openMainTab(browser, place);
    assert.deepEqual(clicks, [selected]);
    if (place === "map" || place === "lens") {
      await openMainTab(browser, place);
      assert.deepEqual(clicks, [selected], "Opening the current panel must not toggle it closed");
    }
  });
}

test("sidebar navigation rejects an unknown destination before interacting", async () => {
  await assert.rejects(openMainTab({}, "world"), /Unknown main tab/);
});
