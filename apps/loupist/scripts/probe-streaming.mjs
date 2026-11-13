#!/usr/bin/env node
// Measure a streamed reply over time in the design preview (docs/ui/chat-answer.md#streaming):
// how text arrives, whether anything the reader has seen moves, what the live line says,
// what the page does by itself, what animates, and what changes at commit.
// Writes summary.json, frames.json and a filmstrip (one capture every --frames ms).
//
//   node scripts/probe-streaming.mjs [--scenario chat-live-research] [--size 1280x820]
//     [--theme light] [--locale en] [--frames 400] [--reduced-motion] [--scroll-test] [--out dir]
import { createServer } from "vite";
import { mkdir, mkdtemp, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { openChrome } from "./visual-diff.mjs";

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const option = (name, fallback) => { const index = args.indexOf(`--${name}`); return index >= 0 ? args[index + 1] : fallback; };
const scenario = option("scenario", "chat-live-research");
const out = resolve(option("out", join(appRoot, "output/design/probe", `${scenario}-${new Date().toISOString().replace(/[-:.]/g, "").slice(0, 15)}`)));
const [width, height] = option("size", "1280x820").split("x").map(Number);
const theme = option("theme", "light");
const locale = option("locale", "en");
const every = Number(option("frames", "400"));
const reducedMotion = args.includes("--reduced-motion");
const scrollTest = args.includes("--scroll-test");
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

// Samples the latest assistant turn every animation frame. The live reply is the last
// `article.message.assistant[data-message-id]` in the thread, never Home's checklist or morning message.
const instrument = `(() => {
  const probe = window.__probe = { frames: [], shifts: [], longTasks: [], moves: [], scrolls: [], animations: { max: 0, properties: {}, names: {} }, commit: null, lines: null };
  new PerformanceObserver((list) => { for (const entry of list.getEntries()) if (!entry.hadRecentInput) probe.shifts.push({ t: entry.startTime, value: entry.value, nodes: entry.sources.map((source) => source.node?.className?.toString?.().slice(0, 60) ?? source.node?.nodeName ?? "") }); }).observe({ type: "layout-shift", buffered: true });
  new PerformanceObserver((list) => { for (const entry of list.getEntries()) probe.longTasks.push({ t: entry.startTime, duration: entry.duration }); }).observe({ type: "longtask", buffered: true });
  let previous = "";
  let tops = new Map();
  let lastScroll = null;
  let wasStreaming = false;
  let footerTop = null;
  let frame = 0;
  const latest = () => [...document.querySelectorAll(".message-scroll article.message.assistant[data-message-id]")].at(-1) ?? null;
  const lineRects = (assistant) => {
    const paragraph = [...assistant.querySelectorAll('[data-turn-item="prose"] .message-content[data-tone="answer"] p')][0];
    if (!paragraph) return null;
    const range = document.createRange();
    range.selectNodeContents(paragraph);
    const rows = new Map();
    for (const rect of range.getClientRects()) { if (!rect.width) continue; const key = Math.round(rect.top); rows.set(key, (rows.get(key) ?? 0) + rect.width); }
    return [...rows.values()].slice(0, 3).map((value) => Math.round(value));
  };
  const sample = () => {
    frame += 1;
    const scroller = document.querySelector(".message-scroll");
    const assistant = latest();
    const now = Math.round(performance.now());
    const streaming = Boolean(assistant?.classList.contains("streaming"));
    const prose = assistant ? [...assistant.querySelectorAll('[data-turn-item="prose"] .message-content')].map((node) => node.textContent ?? "").join("") : "";
    const live = assistant?.querySelector(".turn-live:not([data-leaving]) .turn-tick-in .turn-live-label")?.textContent?.trim() ?? "";
    const target = assistant?.querySelector(".turn-live:not([data-leaving]) .turn-tick-in .turn-live-target")?.textContent?.trim() ?? "";
    const ledger = assistant?.querySelector(".turn-ledger-text .turn-tick-in")?.textContent?.trim() ?? "";
    if (assistant) {
      // Append-only: from its first appearance no block moves down, unless something above it grew (a card filling, a disclosure the reader opened).
      const base = assistant.getBoundingClientRect().top;
      const blocks = [...assistant.querySelectorAll(":scope > [data-turn-item], :scope > .turn-footer")];
      const next = new Map();
      blocks.forEach((block, index) => {
        const key = (block.dataset.turnItem ?? "footer") + ":" + index;
        const top = Math.round(block.getBoundingClientRect().top - base);
        const before = tops.get(key);
        if (before !== undefined && top > before + 1) probe.moves.push({ t: now, block: key, by: top - before, streaming });
        next.set(key, top);
      });
      tops = next;
      const footer = assistant.querySelector(".turn-footer");
      if (wasStreaming && !streaming && footer) {
        const after = Math.round(footer.getBoundingClientRect().top - base);
        probe.commit = { t: now, footerBefore: footerTop, footerAfter: after, linesBefore: probe.lines, linesAfter: lineRects(assistant), scrollBefore: lastScroll };
        setTimeout(() => { probe.commit.scrollAfter500 = Math.round(document.querySelector(".message-scroll")?.scrollTop ?? 0); }, 500);
      }
      if (streaming && footer) footerTop = Math.round(footer.getBoundingClientRect().top - base);
      if (streaming && frame % 6 === 0) probe.lines = lineRects(assistant);
      wasStreaming = streaming;
    }
    const scrollTop = scroller ? Math.round(scroller.scrollTop) : null;
    if (lastScroll !== null && scrollTop !== lastScroll) probe.scrolls.push({ t: now, from: lastScroll, to: scrollTop, streaming });
    lastScroll = scrollTop;
    if (frame % 3 === 0) {
      const running = document.getAnimations().filter((animation) => animation.playState === "running");
      probe.animations.max = Math.max(probe.animations.max, running.length);
      for (const animation of running) {
        const name = animation.animationName ?? animation.transitionProperty ?? "script";
        probe.animations.names[name] = (probe.animations.names[name] ?? 0) + 1;
        for (const keyframe of animation.effect?.getKeyframes?.() ?? []) for (const property of Object.keys(keyframe)) {
          if (["offset", "easing", "composite", "computedOffset"].includes(property)) continue;
          probe.animations.properties[property] = (probe.animations.properties[property] ?? 0) + 1;
        }
      }
    }
    const state = {
      gap: scroller ? Math.round(scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight) : null,
      scrollTop,
      assistant: Boolean(assistant),
      streaming,
      text: prose.length,
      live, target, ledger,
      pill: Boolean(document.querySelector(".chat-status-pill")),
      liveLines: assistant ? assistant.querySelectorAll(".turn-live:not([data-leaving])").length : 0,
    };
    const key = JSON.stringify(state);
    if (key !== previous) { previous = key; probe.frames.push({ t: now, ...state }); }
    requestAnimationFrame(sample);
  };
  requestAnimationFrame(sample);
})();`;

function percentile(values, p) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return Math.round(sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))]);
}

function summarize(probe) {
  const frames = probe.frames;
  const start = frames.find((frame) => frame.assistant)?.t ?? frames[0]?.t ?? 0;
  const done = frames.find((frame, index) => index > 0 && frame.assistant && !frame.streaming && frames[index - 1].streaming);
  const textFrames = frames.filter((frame, index) => index > 0 && frame.text !== frames[index - 1].text && frame.streaming);
  const gaps = textFrames.slice(1).map((frame, index) => frame.t - textFrames[index].t);
  const adds = textFrames.map((frame) => frame.text - frames[frames.indexOf(frame) - 1].text);
  const textDrops = frames.filter((frame, index) => index > 0 && frames[index - 1].assistant && frame.assistant && frame.text < frames[index - 1].text && frames[index - 1].text > 0)
    .map((frame) => ({ t: frame.t - start, from: frames[frames.indexOf(frame) - 1].text, to: frame.text }));
  const labels = [];
  for (const frame of frames) if (frame.live && labels.at(-1)?.label !== frame.live) labels.push({ t: frame.t - start, label: frame.live, target: frame.target });
  return {
    msToAssistantShell: frames.find((frame) => frame.assistant) ? 0 : null,
    msToSettled: done ? done.t - start : null,
    liveLabels: labels,
    ledgerTexts: [...new Set(frames.map((frame) => frame.ledger).filter(Boolean))],
    maxLiveLinesAtOnce: Math.max(0, ...frames.map((frame) => frame.liveLines)),
    textUpdates: textFrames.length,
    textUpdateGapMs: { p50: percentile(gaps, 0.5), p95: percentile(gaps, 0.95), max: gaps.length ? Math.max(...gaps) : null },
    textUpdateChars: { p95: percentile(adds, 0.95), max: adds.length ? Math.max(...adds) : null, over40: adds.filter((value) => value > 40).length },
    textDrops,
    // The live line is always the turn's last line and follows the text it is under; every other block must stay put.
    blocksMovedDown: probe.moves.filter((move) => !move.block.startsWith("footer")).map((move) => ({ ...move, t: move.t - start })),
    pageScrolls: probe.scrolls.map((scroll) => ({ ...scroll, t: scroll.t - start })),
    commit: probe.commit && { ...probe.commit, t: probe.commit.t - start },
    layoutShiftTotal: Number(probe.shifts.filter((shift) => shift.t >= start).reduce((sum, shift) => sum + shift.value, 0).toFixed(4)),
    layoutShifts: probe.shifts.filter((shift) => shift.t >= start).map((shift) => ({ ...shift, t: Math.round(shift.t - start), value: Number(shift.value.toFixed(4)) })),
    longTasks: probe.longTasks.filter((task) => task.t >= start).map((task) => ({ t: Math.round(task.t - start), duration: Math.round(task.duration) })),
    animations: probe.animations,
  };
}

async function run(browser, origin) {
  const { page, evaluate } = browser;
  await page("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
  await page("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-motion", value: reducedMotion ? "reduce" : "no-preference" }, { name: "prefers-color-scheme", value: theme }] });
  await page("Page.navigate", { url: `${origin}/preview.html?scenario=${scenario}&theme=${theme}&locale=${locale}` });
  for (let tries = 0; tries < 200; tries += 1) {
    const ready = await evaluate("document.documentElement.dataset.previewReady ?? null").catch(() => null);
    if (ready === "true") break;
    if (ready === "error") throw new Error(await evaluate("document.documentElement.dataset.previewError"));
    await sleep(50);
  }
  const metrics = async () => Object.fromEntries((await page("Performance.getMetrics")).metrics.map(({ name, value }) => [name, value]));
  await page("Performance.enable", { timeDomain: "threadTicks" });
  const before = await metrics();
  const frames = join(out, "frames");
  await mkdir(frames, { recursive: true });
  const shots = [];
  const began = Date.now();
  let scrolled = null;
  let answered = false;
  while (Date.now() - began < 45_000) {
    const state = await evaluate(`(() => { const f = window.__probe.frames.at(-1); return f ? { streaming: f.streaming, text: f.text, assistant: f.assistant } : null; })()`);
    const elapsed = Date.now() - began;
    if (shots.length * every <= elapsed) {
      const { data } = await page("Page.captureScreenshot", { format: "png" });
      const name = `${String(shots.length).padStart(3, "0")}-${elapsed}ms.png`;
      await writeFile(join(frames, name), Buffer.from(data, "base64"));
      shots.push(name);
    }
    // A question waits for the reader: answer it with the recommended option after a beat.
    if (!answered && elapsed > 1_000) {
      answered = await evaluate(`(() => { const card = document.querySelector(".question-card"); if (!card || !window.__askedAt) { if (card) window.__askedAt = performance.now(); return false; } if (performance.now() - window.__askedAt < 2500) return false; card.querySelector("[data-recommended]")?.click(); return true; })()`);
    }
    if (scrollTest && state?.text > 200 && !scrolled) {
      await page("Input.dispatchMouseEvent", { type: "mouseWheel", x: width * 0.6, y: height * 0.45, deltaX: 0, deltaY: -240 });
      await sleep(400);
      scrolled = await evaluate(`(() => { const s = document.querySelector(".message-scroll"); return { at: Math.round(s.scrollTop), pill: Boolean(document.querySelector(".chat-status-pill")) }; })()`);
      await sleep(1_200);
      scrolled.after = await evaluate(`Math.round(document.querySelector(".message-scroll").scrollTop)`);
      scrolled.jump = await evaluate(`(() => { const b = document.querySelector(".chat-status-pill-jump"); b?.click(); return Boolean(b); })()`);
      await sleep(700);
      scrolled.gapAfterJump = await evaluate(`(() => { const s = document.querySelector(".message-scroll"); const f = [...document.querySelectorAll(".turn-footer")].at(-1); return Math.round(f.getBoundingClientRect().bottom - s.getBoundingClientRect().bottom); })()`);
    }
    if (state && state.assistant && !state.streaming && (state.text > 0 || elapsed > 8_000) && elapsed > 3_000) { await sleep(900); break; }
    await sleep(Math.min(60, every / 2));
  }
  const after = await metrics();
  const cpu = Object.fromEntries(["ScriptDuration", "LayoutDuration", "RecalcStyleDuration", "TaskDuration"].map((name) => [name, Math.round((after[name] - before[name]) * 1000)]));
  const { data } = await page("Page.captureScreenshot", { format: "png" });
  await writeFile(join(frames, "final.png"), Buffer.from(data, "base64"));
  const probe = await evaluate("window.__probe");
  await writeFile(join(out, "frames.json"), JSON.stringify(probe.frames));
  return { ...summarize(probe), cpuMs: cpu, ...(scrollTest ? { scroll: scrolled } : {}), shots: shots.length };
}

let server, browser, cacheDir;
try {
  await rm(out, { recursive: true, force: true });
  await mkdir(out, { recursive: true });
  cacheDir = await mkdtemp(join(tmpdir(), "loupist-probe-vite-"));
  server = await createServer({ root: appRoot, configFile: join(appRoot, "vite.config.ts"), cacheDir, logLevel: "error", server: { fs: { allow: [appRoot, cacheDir, await realpath(join(appRoot, "node_modules"))] }, host: "127.0.0.1", port: 1430, strictPort: false, open: false, hmr: false, watch: null } });
  await server.listen();
  const origin = `http://127.0.0.1:${server.httpServer.address().port}`;
  browser = await openChrome();
  await browser.page("Page.addScriptToEvaluateOnNewDocument", { source: instrument });
  await browser.page("Network.enable");
  await browser.page("Network.setBlockedURLs", { urls: ["https://*", "http://*.example/*"] });
  const summary = await run(browser, origin);
  const result = { scenario, size: `${width}x${height}`, theme, locale, reducedMotion, ...summary };
  await writeFile(join(out, "summary.json"), JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result, null, 2));
  console.log(`Probe: ${out}`);
} finally {
  await browser?.close();
  await server?.close();
  if (cacheDir) await rm(cacheDir, { recursive: true, force: true });
}
