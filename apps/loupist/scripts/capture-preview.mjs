#!/usr/bin/env node
import { createServer } from "vite";
import { execFileSync } from "node:child_process";
import { mkdir, mkdtemp, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { openChrome } from "./visual-diff.mjs";
import { assertChatLayout, measureChatLayout } from "../e2e/native-layout.mjs";
import { assertShellLayout, measureShellLayout } from "../e2e/shell-layout.mjs";
import { assertReaderLayout, measureReaderLayout } from "../e2e/reader-layout.mjs";
import { collectRunningCssMotion, reducedMotionViolations } from "./capture-motion.mjs";
import { markdownIsRendered } from "./capture-readiness.mjs";

// Reduced-motion guards use none/0ms/0.01ms. A 1ms total permits rounding of
// near-zero effects, but rejects visible motion and repeated near-zero effects.
const reducedMotionPolicy = { maxDurationMs: 1, allowlist: [] };
// No exceptions: progress has static text/icon status under reduced motion.
// Any future exception must name the exact target, type and effect, with a reason.

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(appRoot, "../..");
const stamp = new Date().toISOString().replace(/[-:]/g, "").replace("T", "-").slice(0, 15);
const args = process.argv.slice(2);
const layout = args[0] === "--chat-layout";
if (layout) args.shift();
const options = layout ? {
  scenarios: "chat-grid-card,browser-ready,shell-titlebar-grown,chat-header-resuming",
  themes: "light",
  sizes: "700x560,1060x820,1061x820,1280x820,1920x1080",
} : {};
for (let i = 0; i < args.length; i += 2) {
  if (!["--out", "--scenarios", "--themes", "--sizes", "--locales", "--browser-scenarios"].includes(args[i]) || !args[i + 1] || args[i + 1].startsWith("--")) {
    throw new Error("Usage: capture-preview.mjs [--chat-layout] [--browser-scenarios 1..15] [--locales en,ko,en-XA] [--out <dir>] [--scenarios a,b] [--themes light,dark] [--sizes 700x560,1280x820,1680x1050]");
  }
  options[args[i].slice(2)] = args[i + 1];
}
const previewRoot = join(appRoot, "output/design/preview");
const themes = [...new Set((options.themes ?? "light,dark").split(","))];
const locales = [...new Set((options.locales ?? "en").split(","))];
if (locales.some((locale) => !["en", "ko", "en-XA"].includes(locale))) throw new Error("Locales must be en, ko or en-XA");
const sizes = [...new Set((options.sizes ?? "700x560,1280x820,1680x1050").split(","))];
const browserScenarios = Number(options["browser-scenarios"] ?? "1");
if (!Number.isInteger(browserScenarios) || browserScenarios < 1 || browserScenarios > 15) throw new Error("--browser-scenarios must be between 1 and 15");
if (themes.some((theme) => !["light", "dark"].includes(theme))) throw new Error("Themes must be light or dark");
if (sizes.some((size) => !/^[1-9]\d{1,4}x[1-9]\d{1,4}$/.test(size))) throw new Error("Sizes must be positive WxH pairs");
if (typeof WebSocket === "undefined") throw new Error("Node 22 or later with built-in WebSocket is required");
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const html = (value) => String(value).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
let server, browser, cacheDir;
let interrupted = false;
const onSignal = () => { interrupted = true; };
process.on("SIGINT", onSignal);
process.on("SIGTERM", onSignal);
const started = performance.now();
const manifest = { createdAt: new Date().toISOString(), gitHead: null, dirty: null, totalCaptureMs: 0, reducedMotion: reducedMotionPolicy, cells: [] };

// Store the numbers before asserting so a failed cell has both its geometry and its screenshot.
function checkChatCell(cell, geometry, width, height) {
  const { viewport, stage, panel, panelOverlay, reading, chat } = geometry;
  const overlap = chat && panel ? Math.max(0, Math.min(chat.right, panel.right) - Math.max(chat.x, panel.x)) : 0;
  cell.layout = {
    viewport, sidebar: stage?.x, panel: panel?.width ?? 0,
    panelMode: panel ? (panelOverlay ? "overlay" : geometry.panelPlacement) : "closed",
    chat: chat?.width, uncoveredChat: chat && chat.width - overlap, reading: reading?.width,
  };
  assertChatLayout(geometry, { scenario: cell.scenario, label: cell.file });
  if (viewport.width !== width || viewport.height !== height) throw new Error("CSS viewport does not match requested size");
  if (cell.scenario === "chat-grid-card" && panel) throw new Error("Inline grid opened a workspace panel");
  if (cell.scenario !== "browser-ready") return;
  if (!panel || geometry.panelPlacement !== "side" || panelOverlay !== (width <= 1060)) {
    throw new Error("Article panel must overlay a narrow window and dock beside chat on desktop");
  }
  if (!stage || (width >= 900) !== (stage.x > 0)) throw new Error("Unexpected sidebar space");
  if (panel.right > width + 1) throw new Error("Panel escapes viewport");
  if (panelOverlay && Math.abs(chat.right - panel.x) > 1) throw new Error("Chat must wrap into the visible space beside the sheet");
  if (!panelOverlay && Math.abs(chat.right - panel.x) > 1) throw new Error("Dock must sit beside chat");
}

try {
  try {
    manifest.gitHead = execFileSync("git", ["rev-parse", "--verify", "--quiet", "HEAD"], { cwd: repoRoot, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
  } catch (error) {
    // A freshly initialized repository has no commit to attach to the capture.
    if (error.status !== 1) throw error;
  }
  manifest.dirty = Boolean(execFileSync("git", ["status", "--porcelain"], { cwd: repoRoot, encoding: "utf8" }).trim());
  await mkdir(options.out ? resolve(options.out) : previewRoot, { recursive: true });
  const out = options.out ? resolve(options.out) : await mkdtemp(join(previewRoot, `${stamp}-`));
  // A separate dependency optimizer keeps a simultaneous capture or native dev server intact.
  cacheDir = await mkdtemp(join(tmpdir(), "loupist-preview-vite-"));
  // Captures use a static source snapshot; filesystem watchers only consume resources.
  server = await createServer({ root: appRoot, configFile: join(appRoot, "vite.config.ts"), cacheDir, server: { fs: { allow: [appRoot, cacheDir, await realpath(join(appRoot, "node_modules"))] }, host: "127.0.0.1", port: 1430, strictPort: false, open: false, hmr: false, watch: null } });
  await server.listen();
  const registry = (await server.ssrLoadModule("/src/preview/scenarios.ts")).scenarios;
  const { FIXTURE_TIME } = await server.ssrLoadModule("/src/preview/fixtures.ts");
  // An undefined clock makes every pre-freeze `Date.now()` NaN, so a fixture
  // built at module load throws and no scenario ever becomes ready.
  if (!Number.isFinite(FIXTURE_TIME)) throw new Error("src/preview/fixtures.ts must export a finite FIXTURE_TIME");
  const ids = options.scenarios ? [...new Set(options.scenarios.split(","))] : registry.map((scenario) => scenario.id);
  for (const id of ids) if (!registry.some((scenario) => scenario.id === id)) throw new Error(`Unknown scenario: ${id}`);
  console.log(`Capturing ${ids.length} scenarios × ${themes.length} themes × ${sizes.length} sizes × ${locales.length} locales (${ids.length * themes.length * sizes.length * locales.length} cells); reduced-motion limit ${reducedMotionPolicy.maxDurationMs}ms`);
  const address = server.httpServer.address();
  const origin = `http://127.0.0.1:${address.port}`;
  // A long-lived page can stop answering CDP after many navigations. Bound its
  // lifetime to a bounded number of scenarios (one by default).
  for (const [scenarioIndex, scenario] of ids.entries()) {
    if (interrupted) throw new Error("Capture interrupted");
    const freshBrowser = !browser;
    browser ??= await openChrome();
    const { page, evaluate } = browser;
    const requests = new Set();
    let lastNetworkChange = performance.now();
    const stopNetworkListener = browser.onEvent(({ method, params }) => {
      if (method === "Network.requestWillBeSent" && params.type !== "WebSocket") { requests.add(params.requestId); lastNetworkChange = performance.now(); }
      if (method === "Network.loadingFinished" || method === "Network.loadingFailed") { requests.delete(params.requestId); lastNetworkChange = performance.now(); }
    });
    if (freshBrowser) await page("Page.addScriptToEvaluateOnNewDocument", { source: `
      // Freeze before any fixture IDs are created, including the world-graph route
      // whose interactive preview deliberately retains its native clock.
      const NativeDate = globalThis.Date;
      globalThis.Date = new Proxy(NativeDate, {
        construct(target, args) { return Reflect.construct(target, args.length ? args : [${FIXTURE_TIME}]); },
        apply() { return new NativeDate(${FIXTURE_TIME}).toString(); },
        get(target, key, receiver) { return key === 'now' ? () => ${FIXTURE_TIME} : Reflect.get(target, key, receiver); },
      });
      const style = document.createElement('style');
      // Let the app's media-query guards own motion so the check can detect gaps.
      style.textContent = '*, *::before, *::after { caret-color: transparent !important; scroll-behavior: auto !important; }';
      const attach = () => { if (document.documentElement && !style.isConnected) document.documentElement.append(style); };
      new MutationObserver(attach).observe(document, { childList: true, subtree: true }); attach();
    ` });
    // Fix locale and timezone, and keep fixture-only remote links from creating network noise.
    await page("Emulation.setTimezoneOverride", { timezoneId: "Asia/Seoul" });
    await page("Emulation.setLocaleOverride", { locale: "en-US" });
    await page("Network.enable");
    await page("Network.setCacheDisabled", { cacheDisabled: true });
    await page("Network.setBlockedURLs", { urls: ["https://*", "http://*.example/*"] });
    for (const theme of themes) for (const size of sizes) for (const locale of locales) {
      if (interrupted) throw new Error("Capture interrupted");
      const cellStarted = performance.now();
      const file = `${scenario}__${theme}__${size}${options.locales ? `__${locale}` : ""}.png`;
      const cell = { scenario, theme, size, locale, file, selection: null, reducedMotion: null, status: "error", error: null, capturedAt: null, captureMs: 0, attempts: 0 };
      // A cell that times out under machine load gets one more attempt from a fresh navigation.
      for (let attempt = 1; attempt <= 2 && cell.status !== "ok"; attempt += 1) {
        cell.attempts = attempt;
        cell.reducedMotion = null;
        try {
          const [width, height] = size.split("x").map(Number);
          await page("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 2, mobile: false });
          // A print scenario renders with print media, as the print dialog draws the page.
          const media = registry.find((entry) => entry.id === scenario)?.media ?? "";
          await page("Emulation.setEmulatedMedia", { media, features: [{ name: "prefers-reduced-motion", value: "reduce" }, { name: "prefers-color-scheme", value: theme }] });
          const url = `${origin}/preview.html?scenario=${scenario}&theme=${theme}&locale=${locale}&cell=${manifest.cells.length}`;
          requests.clear();
          // Each cell starts from a clean profile: remembered UI state (dismissed hints, legend choice) must not leak between scenarios.
          await page("Storage.clearDataForOrigin", { origin, storageTypes: "local_storage,session_storage,indexeddb" });
          const navigation = await page("Page.navigate", { url });
          if (navigation.errorText) throw new Error(navigation.errorText);
          const deadline = performance.now() + 30000;
          let ready = false;
          while (performance.now() < deadline) {
            const state = await evaluate(`location.href === ${JSON.stringify(url)} ? ({ ready: document.documentElement?.dataset.previewReady, error: document.documentElement?.dataset.previewError }) : ({})`);
            if (state.ready === "error") throw new Error(state.error || "Preview initialization failed");
            if (state.ready === "true") { ready = true; break; }
            await sleep(50);
          }
          if (!ready) throw new Error("previewReady did not become true within 30 seconds");
          // Fonts can be requested by a lazy chunk after previewReady. Observe DOM and
          // actual canvas pixels; a frozen force layout alone does not prove it painted.
          await evaluate(`(async () => {
            await document.fonts.ready;
            window.__captureQuietAt = performance.now();
            window.__captureObserver = new MutationObserver(() => { window.__captureQuietAt = performance.now(); });
            window.__captureObserver.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
          })()`);
          let previousCanvas = null, stableCanvas = 0, settled = false;
          const settleDeadline = performance.now() + 10000;
          while (performance.now() < settleDeadline) {
            const state = await evaluate(`({
              markdown: (${markdownIsRendered.toString()})(),
              fonts: document.fonts.status === 'loaded', quiet: performance.now() - window.__captureQuietAt,
              canvas: [...document.querySelectorAll('.map-canvas canvas')].map(canvas => canvas.toDataURL()).join('|')
            })`);
            stableCanvas = state.canvas === previousCanvas ? stableCanvas + 1 : 0;
            previousCanvas = state.canvas;
            if (state.markdown && state.fonts && state.quiet >= 200 && requests.size === 0 && performance.now() - lastNetworkChange >= 200 && stableCanvas >= 3) { settled = true; break; }
            await sleep(50);
          }
          await evaluate("window.__captureObserver.disconnect()");
          if (!settled) throw new Error("Preview did not settle: Markdown, fonts, lazy requests, DOM or map canvas still changing");
          await evaluate("document.fonts.ready.then(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))))");
          if (registry.find(item => item.id === scenario).selection) {
            const selection = await evaluate(`(() => {
              const selected = document.getSelection();
              const node = selected?.anchorNode;
              const action = document.querySelector('.quote-ask');
              const box = action?.getBoundingClientRect();
              const hit = box && document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
              return selected && !selected.isCollapsed && node?.isConnected && action && (hit === action || action.contains(hit)) ? selected.toString() : null;
            })()`);
            if (!selection?.trim()) throw new Error("Scenario lost its text selection before capture");
            cell.selection = selection;
          }
          // One motion snapshot per ready cell, before any screenshot is taken.
          const motion = await evaluate(`(${collectRunningCssMotion.toString()})()`);
          const violations = reducedMotionViolations(motion.animations, reducedMotionPolicy);
          cell.reducedMotion = { ...motion, violations };
          if (!motion.reducedMotion) throw new Error("Reduced-motion media emulation is not active");
          if (violations.length) throw new Error(`Reduced-motion violation (max ${reducedMotionPolicy.maxDurationMs}ms total): ${violations.map(item => `${item.target}: ${item.animationName ?? item.transitionProperty} (${item.type}, ${item.durationMs}ms × ${item.iterations})`).join("; ")}`);
          const screenshot = await page("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
          await writeFile(join(out, file), Buffer.from(screenshot.data, "base64"));
          if (media !== "print") {
            cell.shell = await evaluate(`(${measureShellLayout.toString()})()`);
            assertShellLayout(cell.shell, { label: file });
            cell.readers = await evaluate(`(${measureReaderLayout.toString()})()`);
            assertReaderLayout(cell.readers, { label: file });
          }
          if (layout) checkChatCell(cell, await evaluate(`(${measureChatLayout.toString()})()`), width, height);
          cell.status = "ok";
          cell.error = null;
          cell.capturedAt = new Date().toISOString();
        } catch (error) {
          cell.error = error.message;
          // A policy failure is deterministic; only readiness/load errors retry.
          if (cell.shell || cell.layout || cell.reducedMotion?.violations.length || cell.reducedMotion?.reducedMotion === false) break;
        }
      }
      cell.captureMs = Math.round(performance.now() - cellStarted);
      manifest.cells.push(cell);
      console.log(`${cell.status === "ok" ? "OK" : "ERROR"} ${file}${cell.layout ? ` ${JSON.stringify(cell.layout)}` : ""}${cell.error ? `: ${cell.error}` : ""}`);
    }
    stopNetworkListener();
    if ((scenarioIndex + 1) % browserScenarios === 0 || scenarioIndex === ids.length - 1) {
      await browser.close();
      browser = null;
    }
  }
  manifest.totalCaptureMs = Math.round(performance.now() - started);
  await writeFile(join(out, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
  const sections = ids.map((id) => `<section><h2>${html(registry.find((s) => s.id === id).title)} <small>${html(id)}</small></h2><table><thead><tr><th>Theme</th>${sizes.map((size) => `<th>${html(size)}</th>`).join("")}</tr></thead><tbody>${themes.flatMap((theme) => locales.map((locale) => `<tr><th>${theme} · ${locale}</th>${sizes.map((size) => {
    const cell = manifest.cells.find((cell) => cell.scenario === id && cell.theme === theme && cell.size === size && cell.locale === locale);
    return `<td>${cell.status === "ok" ? `<a href="${html(cell.file)}"><img src="${html(cell.file)}" loading="lazy" alt="${html(`${id}, ${theme}, ${size}`)}" width="360"></a>` : `<p class="error">${html(cell.error)}</p>`}<p>${html(`${theme} · ${size} · ${cell.captureMs} ms`)}</p></td>`;
  }).join("")}</tr>`)).join("")}</tbody></table></section>`).join("\n");
  await writeFile(join(out, "index.html"), `<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Loupist preview matrix</title><style>body{font:14px system-ui;margin:24px;background:#f4f4f5;color:#18181b}section{margin:32px 0;overflow:auto}table{border-collapse:collapse}td,th{padding:10px;text-align:left;vertical-align:top}img{display:block;width:360px;height:auto;border:1px solid #bbb}small{font-weight:normal;color:#666}.error{color:#b91c1c;width:360px}</style><h1>Loupist design preview</h1><p>Mock bridge design aid · ${html(manifest.gitHead ?? "No commits yet")}${manifest.dirty ? " · dirty checkout" : ""} · ${(manifest.totalCaptureMs / 1000).toFixed(1)} seconds</p>${sections}</html>`);
  console.log(`Preview matrix: ${out} (${(manifest.totalCaptureMs / 1000).toFixed(1)}s)`);
  if (manifest.cells.some((cell) => cell.status !== "ok")) process.exitCode = 1;
} catch (error) {
  console.error(error);
  process.exitCode = 1;
} finally {
  await browser?.close();
  if (server) await server.close();
  if (cacheDir) await rm(cacheDir, { recursive: true, force: true });
  process.off("SIGINT", onSignal); process.off("SIGTERM", onSignal);
}
