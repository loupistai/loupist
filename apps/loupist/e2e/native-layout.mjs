import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { readWindowGeometry, setWindowSize } from "./window-geometry.mjs";
import { assertShellLayout, measureShellLayout } from "./shell-layout.mjs";

export const layoutWidths = [700, 800, 899, 900, 1061, 1280, 1680, 2560];
export const layoutScenarios = [
  "shell-titlebar-grown", "chat-header-resuming",
  "chat-rail-files", "chat-rail-compact", "chat-rail-bottom", "chat-chart-inline", "chat-chart-failed", "chat-grid-card", "chat-step-paused",
  "chat-all-chats", "chat-status-pill", "chat-status-pill-idle",
];

/** Page callback shared by WKWebView and headless Chrome; no driver-specific globals. */
export function measureChatLayout() {
  const box = (element) => {
    if (!element) return null;
    const r = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    if (!r.width || !r.height || style.display === "none" || style.visibility === "hidden") return null;
    return { x: r.x, y: r.y, width: r.width, height: r.height, right: r.right, bottom: r.bottom,
      center: r.x + r.width / 2, position: style.position, overflow: element.scrollWidth - element.clientWidth };
  };
  const rect = (selector) => box(document.querySelector(selector));
  const content = document.querySelector(".conversation-content");
  const contentBox = box(content);
  const contentStyle = content && getComputedStyle(content);
  const left = contentStyle ? parseFloat(contentStyle.paddingLeft) : 0;
  const right = contentStyle ? parseFloat(contentStyle.paddingRight) : 0;
  const reading = contentBox && { ...contentBox, x: contentBox.x + left, right: contentBox.right - right,
    width: contentBox.width - left - right, center: contentBox.center + (left - right) / 2 };
  const panel = document.querySelector(".workspace-panel");
  const scroll = document.querySelector(".message-scroll");
  const textContext = document.createElement("canvas").getContext("2d");
  const receipts = [...document.querySelectorAll(".chat-receipt-item")].flatMap((item) => {
    const label = item.querySelector(".chat-receipt-segment, .chat-receipt-text");
    const bounds = box(label);
    if (!bounds || label.querySelector("svg")) return [];
    const style = getComputedStyle(label);
    textContext.font = `${style.fontStyle} ${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
    const text = label.textContent.trim();
    const textWidth = textContext.measureText(text).width
      + (parseFloat(style.letterSpacing) || 0) * Math.max(0, [...text].length - 1);
    const available = item.closest(".chat-receipt").clientWidth - item.querySelector(".chat-receipt-separator").getBoundingClientRect().width;
    return [{ text, bounds, requiredWidth: Math.min(textWidth, available) }];
  });
  const attachmentIcons = [...document.querySelectorAll(".message-attachment > svg")].filter(box).map((icon) => {
    const style = getComputedStyle(icon);
    return { foreground: style.color, background: style.backgroundColor };
  });
  const cards = [...document.querySelectorAll(".chat-artifact-block, .grid-card")].flatMap((card) => {
    const cardBox = box(card);
    if (!cardBox) return [];
    const toolbars = [...card.querySelectorAll(".chat-artifact-block-header, .chart-local-toolbar, .grid-card-header")]
      .flatMap((toolbar) => {
        const bounds = box(toolbar);
        return bounds ? [{ bounds, controls: [...toolbar.querySelectorAll("button, select, [role='combobox']")].map(box).filter(Boolean) }] : [];
      });
    return [{ bounds: cardBox, toolbars, failedBody: card.querySelector(".chat-artifact-block-state[role='alert']")
      ? box(card.querySelector(".chat-artifact-block-body")) : null }];
  });
  return {
    url: location.href, platform: document.documentElement.dataset.desktopPlatform,
    viewport: { width: innerWidth, height: innerHeight }, documentOverflow: document.documentElement.scrollWidth - innerWidth,
    stage: rect(".app-stage"), main: rect(".chat-tab-panel"), chat: rect(".chat-workspace"), reading,
    composer: rect(".composer-shell"), heading: rect(".workspace-heading"), rail: rect(".chat-rail"),
    chatBody: rect(".chat-body"), readingPane: rect(".chat-reading-pane"), messageViewport: rect(".message-scroll"),
    railMode: document.querySelector(".chat-workspace")?.dataset.rail,
    statusPill: rect(".chat-status-pill"), scroll: scroll && { top: scroll.scrollTop, height: scroll.clientHeight, total: scroll.scrollHeight },
    panel: box(panel), panelPlacement: panel?.dataset.placement, panelOverlay: panel?.getAttribute("aria-modal") === "true",
    identity: rect("[data-testid=loupist-header-identity]"), sidebarTrigger: rect(".app-header-sidebar-trigger"),
    header: rect(".app-header"), cards, receipts, attachmentIcons,
  };
}

/** Assertions compare rendered boundaries, not implementation tokens or stylesheet text. */
export function assertChatLayout(layout, { scenario = "", label = scenario, tolerance = 1.5 } = {}) {
  const check = (condition, reason, evidence) => assert.ok(condition,
    `${label}: ${reason}${evidence ? ` ${JSON.stringify(evidence)}` : ""}`);
  const near = (a, b) => Math.abs(a - b) <= tolerance;
  const within = (child, parent) => child.x >= parent.x - tolerance && child.right <= parent.right + tolerance
    && child.y >= parent.y - tolerance && child.bottom <= parent.bottom + tolerance;
  check(layout.documentOverflow <= tolerance, "document overflows horizontally", layout.documentOverflow);
  check(layout.chat && layout.reading && layout.composer, "reading column and composer must be rendered");
  for (const edge of ["x", "right", "center"]) {
    check(near(layout.reading[edge], layout.composer[edge]), `reading/composer ${edge} is misaligned`,
      { reading: layout.reading, composer: layout.composer });
  }
  check(near(layout.reading.center, layout.readingPane.center), "reading column is off its reserved space's center",
    { reading: layout.reading, pane: layout.readingPane, rail: layout.rail });
  check(layout.chat.overflow <= tolerance, "conversation overflows internally", layout.chat);
  if (layout.panel && layout.panelPlacement === "side" && layout.panelOverlay && layout.stage.width < 840) {
    check(near(layout.panel.x, layout.stage.x) && near(layout.panel.right, layout.stage.right),
      "narrow panel sheet does not cover the full stage", { panel: layout.panel, stage: layout.stage });
  }
  if (layout.panel && layout.panelPlacement === "side" && !layout.panelOverlay) {
    check(layout.chat.width >= 420 - tolerance, "docked panel leaves less than 420 px for chat", layout.chat);
  }
  if (["chat-rail-files", "chat-rail-compact", "chat-rail-bottom"].includes(scenario)) check(layout.rail, "chat context is missing");
  if (layout.rail) {
    check(within(layout.rail, layout.chatBody), "context rail escapes the chat body", { rail: layout.rail, body: layout.chatBody });
    check(near(layout.rail.width, Math.min(300, layout.chat.width - 24)), "context rail is squeezed", layout.rail);
    check(layout.chat.right - layout.rail.right <= 20 + tolerance && layout.rail.y - layout.chatBody.y >= 8 - tolerance && layout.rail.y - layout.chatBody.y <= 8 + tolerance,
      "context rail is not inset at the top right of the chat body", { rail: layout.rail, body: layout.chatBody });
    if (layout.heading) check(layout.rail.y >= layout.heading.bottom + 8 - tolerance,
      "context rail overlaps the chat header", { rail: layout.rail, heading: layout.heading });
    if (layout.railMode === "rail") {
      check(layout.rail.x >= layout.reading.right + 12 - tolerance && layout.rail.x >= layout.composer.right + 12 - tolerance,
        "context rail covers the conversation or composer", { rail: layout.rail, reading: layout.reading, composer: layout.composer });
    } else {
      check(layout.rail.bottom + 8 <= layout.readingPane.y + tolerance,
        "compact context covers the answer or composer", { rail: layout.rail, pane: layout.readingPane });
      check(layout.messageViewport.height > 0, "compact context left no room for the transcript", layout);
    }
  }
  if (layout.platform === "macos" && layout.viewport.width >= 768 && layout.viewport.width < 900) {
    // Native traffic lights occupy the first 80 CSS px; the visible drawer toggle starts after them.
    check(layout.sidebarTrigger && layout.sidebarTrigger.x >= 80 - tolerance,
      "sidebar toggle overlaps native window controls or is hidden", layout.sidebarTrigger);
    check(layout.identity && layout.identity.x >= layout.sidebarTrigger.right - tolerance,
      "Loupist identity overlaps the titlebar toggle", { identity: layout.identity, trigger: layout.sidebarTrigger });
  }
  for (const card of layout.cards) {
    check(card.bounds.x >= layout.reading.x - tolerance && card.bounds.right <= layout.reading.right + tolerance,
      "answer card escapes the reading column", card.bounds);
    for (const toolbar of card.toolbars) {
      check(within(toolbar.bounds, card.bounds), "card toolbar escapes its card", toolbar.bounds);
      // Compact card actions may extend into the card's own padding above the
      // heading. The card's clipping edge, rather than its text row, is the boundary.
      for (const control of toolbar.controls) check(within(control, card.bounds), "toolbar control is clipped", { control, card: card.bounds });
    }
    if (card.failedBody) check(card.failedBody.height < 160, "failed chart keeps an empty plot reservation", card.failedBody);
  }
  for (const receipt of layout.receipts) check(receipt.bounds.width + tolerance >= receipt.requiredWidth,
    "receipt label wraps despite sufficient row space", receipt);
  for (const icon of layout.attachmentIcons) check(icon.foreground !== icon.background,
    "attachment icon has the same color as its background", icon);
  if (scenario === "chat-chart-inline") check(layout.cards.some((card) => card.toolbars.length), "chart card is missing");
  if (scenario === "chat-chart-failed") check(layout.cards.some((card) => card.failedBody), "failed chart alert is missing");
  if (scenario === "chat-grid-card") check(layout.cards.length, "grid card is missing");
  if (scenario.startsWith("chat-status-pill")) check(layout.statusPill, "scrolled-up status pill is missing");
  if (layout.statusPill) {
    check(near(layout.statusPill.center, layout.composer.center), "status pill is off the reading-column center", layout.statusPill);
    check(layout.statusPill.bottom <= layout.composer.y - 16 + tolerance, "status pill overlaps or crowds composer",
      { pill: layout.statusPill, composer: layout.composer });
  }
  return layout;
}

async function settle(browser) {
  await browser.waitUntil(() => browser.execute(() => document.fonts.status === "loaded"), { timeout: 10_000, interval: 50 });
  await browser.pause(100);
}

export async function openLayoutPreview(browser, devUrl, { scenario, locale = "en", theme = "dark" }) {
  const target = new URL("preview.html", `${String(devUrl).replace(/\/$/, "")}/`);
  target.search = new URLSearchParams({ scenario, locale, theme, layoutProbe: randomUUID() }).toString();
  await browser.url(target.href);
  await browser.switchToWindow("main").catch(() => {});
  let state;
  await browser.waitUntil(async () => {
    state = await browser.execute((url) => location.href === url ? {
      ready: document.documentElement.dataset.previewReady, error: document.documentElement.dataset.previewError,
    } : null, target.href);
    return state?.ready === "true" || state?.ready === "error";
  }, { timeout: 20_000, interval: 100, timeoutMsg: `Preview did not initialize at its exact URL: ${target.href}` });
  assert.notEqual(state.ready, "error", `${scenario}: ${state.error}`);
  // Stable frames make the proof equally useful on focused and WebView-only native runs.
  await browser.execute(() => {
    const style = document.createElement("style");
    style.textContent = "*,*::before,*::after{animation-duration:0s!important;transition-duration:0s!important;scroll-behavior:auto!important}";
    document.head.append(style);
  });
  await settle(browser);
  return target.href;
}

async function setShellState(browser, { navigation = true, panel = false }) {
  await browser.execute(({ navigation, panel }) => {
    const sidebar = document.querySelector('[data-slot="sidebar"][data-state]');
    if (innerWidth >= 900 && sidebar && (sidebar.dataset.state === "expanded") !== navigation) {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "b", metaKey: true, ctrlKey: !navigator.userAgent.includes("Mac"), bubbles: true }));
    }
    const workspace = document.querySelector(".workspace-panel");
    const open = workspace && !workspace.hidden && getComputedStyle(workspace).display !== "none";
    if (Boolean(open) !== panel) (panel ? document.querySelector('[data-workspace-split="side"]') : document.querySelector("[data-workspace-close]"))?.click();
  }, { navigation, panel });
  await browser.waitUntil(() => browser.execute(({ navigation, panel }) => {
    const sidebar = document.querySelector('[data-slot="sidebar"][data-state]');
    const workspace = document.querySelector(".workspace-panel");
    const open = Boolean(workspace && !workspace.hidden && getComputedStyle(workspace).display !== "none");
    return open === panel && (innerWidth < 900 || (sidebar?.dataset.state === "expanded") === navigation);
  }, { navigation, panel }), { timeout: 5_000, interval: 50, timeoutMsg: "Requested navigation/panel state did not settle" });
  await settle(browser);
}

/**
 * Uses fixture-only pages and restores URL, window geometry and local UI storage.
 * The caller owns the native GUI lease and existing WebDriver session.
 */
export async function runNativeLayoutRegression(browser, {
  devUrl, artifactDir, prefix = "native-layout", widths = layoutWidths, scenarios = layoutScenarios,
  locales = ["en", "ko"], themes = ["light", "dark"], height = 820, panelRestoration = true,
} = {}) {
  assert.ok(devUrl && artifactDir, "devUrl and artifactDir are required");
  const origin = new URL(devUrl).origin;
  const original = { rect: await browser.getWindowRect(), url: await browser.getUrl() };
  assert.equal(new URL(original.url).origin, origin, "Layout fixtures must use the attached window's existing dev origin");
  const storage = await browser.execute(() => Object.fromEntries(Object.entries(localStorage).filter(([key]) => key.startsWith("loupist."))));
  const resetStorage = () => browser.execute(() => {
    for (const key of Object.keys(localStorage)) if (key.startsWith("loupist.")) localStorage.removeItem(key);
  });
  const runtime = await browser.execute(() => ({ userAgent: navigator.userAgent, nativeBridge: Boolean(window.__TAURI_INTERNALS__) }));
  const report = { runtime, mode: !runtime.nativeBridge ? "headless-browser"
    : process.env.LOUPIST_E2E_ALLOW_UNFOCUSED === "1" ? "webview-only" : "focused-native",
    cases: [], skipped: [], restoration: null };
  await mkdir(artifactDir, { recursive: true });
  const reportPath = resolve(artifactDir, `${prefix}.json`);
  const save = () => writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
  const capture = async (name, metadata) => {
    const layout = await browser.execute(measureChatLayout);
    const shell = await browser.execute(measureShellLayout);
    const file = `${prefix}-${name}.png`;
    const entry = { ...metadata, file, layout, shell };
    report.cases.push(entry);
    await browser.saveScreenshot(resolve(artifactDir, file));
    try {
      assertShellLayout(shell, { label: name });
      assertChatLayout(layout, { scenario: metadata.scenario, label: name });
    }
    catch (error) { entry.error = error.message; throw error; }
    finally { await save(); }
    return layout;
  };
  let failure;
  try {
    for (const width of widths) {
      try { await setWindowSize(browser, { width, height }); }
      catch (error) {
        if (width <= 1680) throw error;
        report.skipped.push({ width, reason: error.message });
        await save();
        continue;
      }
      for (const locale of locales) for (const theme of themes) for (const scenario of scenarios) {
        await resetStorage();
        await openLayoutPreview(browser, devUrl, { scenario, locale, theme });
        const states = scenario === "chat-all-chats" && width >= 900
          ? [{ navigation: true, panel: false }, { navigation: false, panel: false }, { navigation: true, panel: true }, { navigation: false, panel: true }]
          : width >= 1060
          ? [{ navigation: true, panel: false }, { navigation: true, panel: true }, { navigation: false, panel: true }]
          : [{ navigation: width >= 900, panel: false }];
        for (const state of states) {
          await setShellState(browser, state);
          if (scenario.startsWith("chat-status-pill")) {
            await browser.execute(() => {
              const scroll = document.querySelector(".message-scroll");
              // Upward reader input ends following (left on by the previous state's Jump to latest); a bare scrollTop does not.
              scroll.dispatchEvent(new WheelEvent("wheel", { deltaY: -120, bubbles: true }));
              scroll.scrollTop = 0;
              scroll.dispatchEvent(new Event("scroll"));
            });
            await browser.waitUntil(() => browser.execute(() => Boolean(document.querySelector(".chat-status-pill"))),
              { timeout: 5_000, interval: 50, timeoutMsg: "Scrolling away from the end did not show Jump to latest" });
          }
          const name = `${scenario}-${width}-${locale}-${theme}-nav${Number(state.navigation)}-panel${Number(state.panel)}`;
          await capture(name, { scenario, width, locale, theme, ...state });
          if (scenario.startsWith("chat-status-pill")) {
            await browser.execute(() => document.querySelector(".chat-status-pill-jump")?.click());
            await browser.waitUntil(() => browser.execute(() => {
              const scroll = document.querySelector(".message-scroll");
              return scroll && scroll.scrollHeight - scroll.clientHeight - scroll.scrollTop <= 2 && !document.querySelector(".chat-status-pill");
            }), { timeout: 5_000, interval: 50, timeoutMsg: `${name}: Jump to latest did not reach the bottom` });
            await capture(`${name}-latest`, { scenario: "chat-latest", width, locale, theme, ...state });
          }
        }
      }
    }
    if (panelRestoration) {
      const wide = Math.max(...widths.filter((width) => width >= 1280 && !report.skipped.some((item) => item.width === width)));
      if (Number.isFinite(wide)) {
        await setWindowSize(browser, { width: wide, height });
        await resetStorage();
        await openLayoutPreview(browser, devUrl, { scenario: "chat-all-chats" });
        await setShellState(browser, { navigation: true, panel: true });
        const sizes = [];
        for (const width of [wide, 1061, wide]) {
          await setWindowSize(browser, { width, height });
          await settle(browser);
          const layout = await browser.execute(measureChatLayout);
          assertChatLayout(layout, { scenario: "chat-all-chats", label: `panel-restoration-${width}` });
          sizes.push({ window: width, actual: await readWindowGeometry(browser), panel: layout.panel?.width, chat: layout.chat.width });
        }
        report.restoration = sizes;
        assert.ok(Math.abs(sizes[0].panel - 460) <= 1.5 && sizes[1].panel < sizes[0].panel
          && Math.abs(sizes[2].panel - sizes[0].panel) <= 1.5, `Preferred panel size did not survive fitting: ${JSON.stringify(sizes)}`);
      } else report.skipped.push({ check: "panel-restoration", reason: "Selected widths have no window at least 1280 px wide" });
    }
  } catch (error) { failure = error; report.error = error.message; }
  finally {
    try {
      await browser.setWindowRect(original.rect.x, original.rect.y, original.rect.width, original.rect.height);
      await setWindowSize(browser, { width: original.rect.width, height: original.rect.height });
      await browser.execute((saved) => {
        for (const key of Object.keys(localStorage)) if (key.startsWith("loupist.")) localStorage.removeItem(key);
        for (const [key, value] of Object.entries(saved)) localStorage.setItem(key, value);
      }, storage);
      await browser.url(original.url);
      await browser.waitUntil(() => browser.execute((url) => location.href === url
        && (new URL(url).pathname.endsWith("preview.html")
          ? document.documentElement.dataset.previewReady === "true" : document.readyState === "complete"), original.url),
      { timeout: 20_000, interval: 100 });
      await browser.pause(250);
      report.restored = { url: await browser.getUrl(), geometry: await readWindowGeometry(browser) };
    } catch (error) {
      report.restoreError = error.message;
      failure = failure ? new AggregateError([failure, error], "Layout check and restoration failed") : error;
    }
    await save();
  }
  if (failure) throw failure;
  return report;
}

async function main() {
  const [sessionPath, ...args] = process.argv.slice(2);
  assert.ok(sessionPath, "usage: node native-layout.mjs <session.json> [--widths=700,800] [--scenarios=chat-rail-files] [--locales=en,ko] [--themes=dark] [--artifact-dir=PATH] [--prefix=NAME] [--no-panel-restoration]");
  const options = {};
  for (const arg of args) {
    if (arg === "--no-panel-restoration") { options.panelRestoration = false; continue; }
    const match = /^--(widths|scenarios|locales|themes|artifact-dir|prefix)=(.+)$/.exec(arg);
    assert.ok(match, `Unknown option: ${arg}`);
    const [, key, value] = match;
    if (key === "artifact-dir") options.artifactDir = resolve(value);
    else if (key === "prefix") { assert.match(value, /^[a-zA-Z0-9_-]+$/); options.prefix = value; }
    else options[key] = value.split(",").map((part) => key === "widths" ? Number(part) : part);
  }
  if (options.widths) assert.ok(options.widths.every((width) => Number.isInteger(width) && width >= 700 && width <= 2560));
  const session = JSON.parse(await readFile(sessionPath, "utf8"));
  const { sessionEndpointIsOwned } = await import("./wait-session-lib.mjs");
  assert.ok(sessionEndpointIsOwned(session), "Refusing an unowned or installed-app WebDriver endpoint");
  const { remote } = await import("webdriverio");
  const browser = await remote({ ...session.webdriverConfig, capabilities: session.capabilities,
    connectionRetryTimeout: 15_000, connectionRetryCount: 0, logLevel: "error" });
  try {
    const { focusNativeWindow } = await import("./native-window-focus.mjs");
    await focusNativeWindow(browser, session);
    const result = await runNativeLayoutRegression(browser, { devUrl: session.devUrl ?? await browser.getUrl(),
      artifactDir: resolve(dirname(fileURLToPath(import.meta.url)), "artifacts/layout"), ...options });
    console.log(`Native layout passed: ${result.cases.length} captures; ${result.skipped.length} skipped boundaries.`);
  } finally { await browser.deleteSession(); }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
