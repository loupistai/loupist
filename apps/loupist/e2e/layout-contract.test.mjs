import assert from "node:assert/strict";
import { test } from "node:test";
import vm from "node:vm";
import { JSDOM } from "jsdom";
import { assertShellLayout, measureShellLayout } from "./shell-layout.mjs";
import { assertChatLayout, measureChatLayout } from "./native-layout.mjs";

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
    <div class="app-titlebar" data-rect="0,0,700,40"></div>
    <div class="app-body" data-rect="0,40,700,780"><div class="app-frame" data-rect="64,40,636,780"></div>
      <div data-collapsible="icon"><nav data-slot="sidebar-container" data-rect="0,40,64,780">
        ${places.map((id, index) => `<div data-sidebar-place="${id}"><button data-rect="${id === broken ? x : 12},${100 + 48 * index},40,40"></button></div>`).join("")}
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
