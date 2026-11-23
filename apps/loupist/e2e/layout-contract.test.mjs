import assert from "node:assert/strict";
import { test } from "node:test";
import vm from "node:vm";
import { JSDOM } from "jsdom";
import { assertShellLayout, measureShellLayout } from "./shell-layout.mjs";
import { assertChatLayout, measureChatLayout } from "./native-layout.mjs";
import { assertMapOverlays, measureMapOverlays } from "./map-layout.mjs";

function measure(callback, html) {
  const { window } = new JSDOM(html);
  window.HTMLElement.prototype.getBoundingClientRect = function () {
    const [x, y, width, height] = this.getAttribute("data-rect")?.split(",").map(Number) ?? [0, 0, 100, 40];
    return { x, y, width, height, right: x + width, bottom: y + height };
  };
  window.HTMLCanvasElement.prototype.getContext = () => ({ measureText: () => ({ width: 10 }) });
  try {
    return vm.runInNewContext(`(${callback})()`, {
      document: window.document, getComputedStyle: window.getComputedStyle.bind(window),
      innerWidth: 700, innerHeight: 820, location: { href: "http://localhost/preview.html" },
    });
  } finally { window.close(); }
}

test("collapsed shell measures all eight rail actions once", () => {
  const layout = measure(measureShellLayout, `<main data-testid="app-shell">
    <div data-collapsible="icon"><nav data-slot="sidebar-container">
      <button class="navigation-search"></button>
      <div data-testid="loupist-chat-row" data-home><button data-testid="loupist-chat-button"><svg class="loupist-home-icon"></svg></button></div>
      ${["following", "map", "lens", "schedule"].map(id => `<div data-sidebar-place="${id}"><button><svg class="loupist-home-icon"></svg></button></div>`).join("")}
      <button data-testid="navigation-new"></button><button id="main-tab-settings"></button>
    </nav></div><header class="workspace-heading"><div class="workspace-chat-title"><div class="thread-statuses"></div></div></header>
  </main>`);
  assert.equal(layout.navigation[0].railButtons.length, 8);
  assert.equal(layout.headingItems.length, 1);
});

test("collapsed shell rejects an off-center or clipped destination in each new sidebar row", () => {
  const places = ["following", "map", "lens", "schedule"];
  const html = (broken, x) => `<main data-testid="app-shell" data-rect="0,0,700,820">
    <div class="app-body" data-rect="0,0,700,820"><div class="app-frame" data-rect="76,0,624,820"><header class="app-header" data-rect="76,0,624,56"><button data-rect="88,12,32,32"></button></header></div>
      <div data-collapsible="icon"><nav data-slot="sidebar-container" data-rect="0,0,76,820">
        ${places.map((id, index) => `<div data-sidebar-place="${id}"><button data-rect="${id === broken ? x : 18},${100 + 48 * index},40,40"></button></div>`).join("")}
      </nav></div>
    </div></main>`;
  assert.doesNotThrow(() => assertShellLayout(measure(measureShellLayout, html())));
  for (const place of places) for (const x of [0, 30]) {
    assert.throws(() => assertShellLayout(measure(measureShellLayout, html(place, x))), /collapsed sidebar control is off-center/, `${place} at ${x}`);
  }
});

test("chart toolbar measures native range selects and its More button", () => {
  const layout = measure(measureChatLayout, `<article class="chat-artifact-block">
    <div class="chart-local-toolbar"><span>NVDA</span><select><option>Start</option></select><select><option>End</option></select><button class="chart-more-trigger">…</button></div>
  </article>`);
  assert.equal(layout.cards[0].toolbars[0].controls.length, 3);
});

function chatLayout({ overlay = true, panelWidth = 700, chatWidth = 700 } = {}) {
  const box = (x, width) => ({ x, right: x + width, width, y: 0, bottom: 820, height: 820, center: x + width / 2, overflow: 0 });
  return {
    viewport: box(0, 700), stage: box(0, 700), documentOverflow: 0,
    chat: box(0, chatWidth), reading: box(12, chatWidth - 24), composer: box(12, chatWidth - 24), readingPane: box(0, chatWidth),
    panel: box(700 - panelWidth, panelWidth), panelPlacement: "side", panelOverlay: overlay,
    cards: [], receipts: [], attachmentIcons: [],
  };
}

test("a narrow side sheet covers the stage while the chat stays mounted beneath it", () => {
  assert.doesNotThrow(() => assertChatLayout(chatLayout()));
  assert.throws(() => assertChatLayout(chatLayout({ panelWidth: 480 })), /cover the full stage/);
});

test("a docked panel still reserves the minimum readable chat width", () => {
  assert.throws(() => assertChatLayout(chatLayout({ overlay: false, panelWidth: 400, chatWidth: 300 })), /less than 420/);
});

test("a resized sidebar keeps its handle on its right edge", () => {
  const html = (handleX) => `<main data-testid="app-shell" data-rect="0,0,700,820">
    <div class="app-body" data-rect="0,0,700,820"><div class="app-frame" data-rect="232,0,468,820"></div>
      <nav data-slot="sidebar-container" data-rect="0,0,232,820"><div data-sidebar-resize data-rect="${handleX},0,8,820"></div></nav>
    </div></main>`;
  assert.doesNotThrow(() => assertShellLayout(measure(measureShellLayout, html(224))));
  assert.throws(() => assertShellLayout(measure(measureShellLayout, html(180))), /resize handle is not on the sidebar's right edge/);
});

test("a long row title stops before its time or badge, in the activity sheet and the sidebar", () => {
  const html = (titleWidth, sessionWidth = 140) => `<main data-testid="app-shell" data-rect="0,0,700,820">
    <div class="app-body" data-rect="0,0,700,820"><div class="app-frame" data-rect="0,0,700,820"></div></div>
    <ol class="activity-earlier"><li data-rect="328,600,340,44"><i></i><button data-rect="352,600,${titleWidth},44"><span class="activity-step-label" data-rect="352,600,${titleWidth},20">A long question</span><small></small></button><time data-rect="600,600,60,20">8:50 AM</time></li></ol>
    <ul><li class="loupist-chat-row" data-rect="8,300,232,32"><button><span class="loupist-chat-title" data-rect="16,306,${sessionWidth},20">A long session title</span><span class="loupist-status-dot" data-state="finished" data-rect="220,312,8,8"></span></button></li></ul>
  </main>`;
  const layout = measure(measureShellLayout, html(240));
  assert.deepEqual(Array.from(layout.titledRows, row => row.list), [".activity-earlier > li", ".loupist-chat-row"]);
  assert.doesNotThrow(() => assertShellLayout(layout));
  assert.throws(() => assertShellLayout(measure(measureShellLayout, html(316))), /row title runs under its time or badge/);
  assert.throws(() => assertShellLayout(measure(measureShellLayout, html(240, 220))), /row title runs under its time or badge/);
});

test("World controls sit beside What's new, the Library or a sheet, never under them", () => {
  const map = ({ panel = "", stats = "8,8,300,30", legend = "8,660,800,32" } = {}) => `<div data-testid="map-view" data-rect="0,0,900,700">
    <div class="map-body" data-rect="0,0,900,700"><div class="map-stage" data-rect="0,0,900,700">
      <div class="map-stats" data-rect="${stats}"></div><details class="map-legend" open data-rect="${legend}"></details><button class="map-fit" data-rect="860,660,28,28"></button>
    </div>${panel}</div></div>`;
  const whatsNew = `<aside class="map-library map-whats-new" style="position:absolute" data-rect="0,0,360,700"></aside>`;
  assert.doesNotThrow(() => assertMapOverlays(measure(measureMapOverlays, map())));
  // The pill and the key strip moved with the panel only as far as it was mid-slide: 24 px short.
  assert.throws(() => assertMapOverlays(measure(measureMapOverlays, map({ panel: whatsNew, stats: "344,8,300,30", legend: "344,660,470,32" }))), /div\.map-stats is not inside the free part/);
  assert.doesNotThrow(() => assertMapOverlays(measure(measureMapOverlays, map({ panel: whatsNew, stats: "368,8,300,30", legend: "368,660,446,32" }))));
  // A bottom sheet leaves the strip above it; a panel over the whole canvas leaves nothing to place.
  const sheet = `<aside class="map-inspector" style="position:absolute" data-rect="0,400,900,300"></aside>`;
  assert.throws(() => assertMapOverlays(measure(measureMapOverlays, map({ panel: sheet }))), /map-legend is not inside/);
  assert.doesNotThrow(() => assertMapOverlays(measure(measureMapOverlays, map({ panel: `<aside class="map-library" style="position:absolute" data-rect="0,0,900,700"></aside>` }))));
});
