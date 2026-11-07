#!/usr/bin/env node
// Measure a streamed reply over time in the design preview: what the reader sees
// before the first word, how text arrives, whether the thread shifts, and whether
// a reader who scrolls up keeps their place. Writes summary.json and a filmstrip.
import { createServer } from "vite";
import { mkdir, mkdtemp, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { openChrome } from "./visual-diff.mjs";

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const option = (name, fallback) => { const index = args.indexOf(`--${name}`); return index >= 0 ? args[index + 1] : fallback; };
const out = resolve(option("out", join(appRoot, "output/design/probe", new Date().toISOString().replace(/[-:.]/g, "").slice(0, 15))));
const [width, height] = option("size", "1280x820").split("x").map(Number);
const theme = option("theme", "light");
const reducedMotion = args.includes("--reduced-motion");
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

const instrument = `(() => {
  const probe = window.__probe = { frames: [], shifts: [], longTasks: [], mutations: [] };
  new PerformanceObserver((list) => { for (const entry of list.getEntries()) if (!entry.hadRecentInput) probe.shifts.push({ t: entry.startTime, value: entry.value, nodes: entry.sources.map((source) => source.node?.className?.toString?.().slice(0, 60) ?? source.node?.nodeName ?? "") }); }).observe({ type: "layout-shift", buffered: true });
  new PerformanceObserver((list) => { for (const entry of list.getEntries()) probe.longTasks.push({ t: entry.startTime, duration: entry.duration }); }).observe({ type: "longtask", buffered: true });
  let mutations = 0;
  const watch = () => document.body ? new MutationObserver((records) => { mutations += records.length; }).observe(document.body, { subtree: true, childList: true, characterData: true, attributes: true }) : requestAnimationFrame(watch);
  watch();
  let previous = "";
  const sample = () => {
    const scroller = document.querySelector(".message-scroll");
    const messages = [...document.querySelectorAll(".message")];
    const last = messages.at(-1);
    const assistant = last?.classList.contains("assistant") ? last : null;
    const prose = assistant ? [...assistant.querySelectorAll(".message-content")].map((node) => node.textContent ?? "").join("") : "";
    const status = assistant?.querySelector(".chat-progress-heading, .chat-progress-live, [data-run-status]")?.textContent?.trim().slice(0, 80) ?? "";
    const frame = {
      gap: scroller ? Math.round(scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight) : null,
      scrollTop: scroller ? Math.round(scroller.scrollTop) : null,
      assistant: Boolean(assistant),
      streaming: Boolean(assistant?.classList.contains("streaming")),
      text: prose.length,
      height: assistant ? Math.round(assistant.getBoundingClientRect().height) : 0,
      status,
      visible: assistant ? Math.round(assistant.getBoundingClientRect().height) > 4 && (assistant.textContent ?? "").trim().length > 0 : false,
      pill: Boolean(document.querySelector(".chat-status-pill")),
    };
    const key = JSON.stringify(frame);
    if (key !== previous) { previous = key; probe.frames.push({ t: Math.round(performance.now()), mutations, ...frame }); }
    requestAnimationFrame(sample);
  };
  requestAnimationFrame(sample);
})();`;

function percentile(values, p) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return Math.round(sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))]);
}

function summarize(probe, scrolledAt) {
  const frames = probe.frames;
  // The scenario submits as it opens: the run starts when the newest message is the user's question.
  const submitted = frames.findIndex((frame) => frame.gap !== null && !frame.assistant && frames.some((earlier) => earlier.t < frame.t && earlier.assistant));
  const start = frames[submitted]?.t ?? 0;
  const after = (frame) => frame.t >= start;
  const firstAssistant = frames.find((frame) => after(frame) && frame.assistant);
  const firstVisible = frames.find((frame) => after(frame) && frame.visible);
  const firstText = frames.find((frame) => after(frame) && frame.text > 0);
  const streamingFrames = frames.filter((frame) => after(frame) && frame.streaming);
  const done = frames.find((frame, index) => index > 0 && frame.assistant && !frame.streaming && frames[index - 1].streaming);
  const textFrames = frames.filter((frame, index) => index > 0 && frame.text !== frames[index - 1].text && frame.streaming);
  const gaps = textFrames.slice(1).map((frame, index) => frame.t - textFrames[index].t);
  const shrinks = frames.filter((frame, index) => index > 0 && after(frame) && frames[index - 1].t >= start && frame.assistant && frames[index - 1].assistant && frame.height < frames[index - 1].height - 2)
    .map((frame) => ({ t: frame.t - start, by: frames[frames.indexOf(frame) - 1].height - frame.height, text: frame.text }));
  const textDrops = frames.filter((frame, index) => index > 0 && frames[index - 1].t >= start && frames[index - 1].assistant && frame.text < frames[index - 1].text && frames[index - 1].text > 0)
    .map((frame) => ({ t: frame.t - start, from: frames[frames.indexOf(frame) - 1].text, to: frame.text }));
  const statuses = [...new Set(frames.filter(after).map((frame) => frame.status).filter(Boolean))];
  const window = (frame) => frame.t >= start && (!done || frame.t <= done.t + 600);
  const unpinned = frames.filter((frame) => window(frame) && frame.streaming && frame.gap > 24 && (scrolledAt === undefined || frame.t < scrolledAt));
  const mutationsDuringStream = streamingFrames.length ? streamingFrames.at(-1).mutations - streamingFrames[0].mutations : 0;
  return {
    msToAssistantShell: firstAssistant ? firstAssistant.t - start : null,
    msToFirstVisibleFeedback: firstVisible ? firstVisible.t - start : null,
    msToFirstAnswerText: firstText ? firstText.t - start : null,
    msToSettled: done ? done.t - start : null,
    statusLabelsSeen: statuses,
    textUpdates: textFrames.length,
    textUpdateGapMs: { p50: percentile(gaps, 0.5), p95: percentile(gaps, 0.95), max: gaps.length ? Math.max(...gaps) : null },
    heightShrinks: shrinks,
    textDrops,
    layoutShiftTotal: Number(probe.shifts.filter((shift) => shift.t >= start).reduce((sum, shift) => sum + shift.value, 0).toFixed(4)),
    layoutShifts: probe.shifts.filter((shift) => shift.t >= start).map((shift) => ({ ...shift, t: Math.round(shift.t - start), value: Number(shift.value.toFixed(4)) })),
    longTaskMs: Math.round(probe.longTasks.filter((task) => task.t >= start).reduce((sum, task) => sum + task.duration, 0)),
    domMutationsWhileStreaming: mutationsDuringStream,
    framesNotPinnedWhileStreaming: unpinned.length,
    maxGapWhilePinned: unpinned.length ? Math.max(...unpinned.map((frame) => frame.gap)) : 0,
  };
}

async function run(browser, origin, label, { scrollTest }) {
  const { page, evaluate } = browser;
  await page("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
  await page("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-motion", value: reducedMotion ? "reduce" : "no-preference" }] });
  await page("Page.navigate", { url: `${origin}/preview.html?scenario=chat-live-research&theme=${theme}` });
  for (let tries = 0; tries < 200; tries += 1) {
    const ready = await evaluate("document.documentElement.dataset.previewReady ?? null").catch(() => null);
    if (ready === "true") break;
    if (ready === "error") throw new Error(await evaluate("document.documentElement.dataset.previewError"));
    await sleep(50);
  }
  const metrics = async () => Object.fromEntries((await page("Performance.getMetrics")).metrics.map(({ name, value }) => [name, value]));
  await page("Performance.enable", { timeDomain: "threadTicks" });
  const before = await metrics();
  const frames = join(out, label);
  await mkdir(frames, { recursive: true });
  const shots = [];
  const began = Date.now();
  let scrolledAt;
  let scrollCheck;
  let jumped = false;
  while (Date.now() - began < 16_000) {
    const state = await evaluate(`(() => { const f = window.__probe.frames.at(-1); return f ? { streaming: f.streaming, text: f.text, assistant: f.assistant, t: f.t } : null; })()`);
    const elapsed = Date.now() - began;
    if (shots.length * 400 <= elapsed) {
      const { data } = await page("Page.captureScreenshot", { format: "png" });
      const name = `${String(shots.length).padStart(2, "0")}-${elapsed}ms.png`;
      await writeFile(join(frames, name), Buffer.from(data, "base64"));
      shots.push(name);
    }
    if (scrollTest && state?.text > 200 && scrolledAt === undefined) {
      scrolledAt = state.t;
      await page("Input.dispatchMouseEvent", { type: "mouseWheel", x: width * 0.6, y: height * 0.45, deltaX: 0, deltaY: -420 });
      await sleep(700);
      scrollCheck = await evaluate(`(() => { const s = document.querySelector(".message-scroll"); return { gap: Math.round(s.scrollHeight - s.scrollTop - s.clientHeight), jump: Boolean(document.querySelector(".chat-status-pill")) }; })()`);
    }
    if (scrollTest && scrollCheck && !jumped && Date.now() - began > 0 && state?.streaming) {
      const later = await evaluate(`(() => { const s = document.querySelector(".message-scroll"); return Math.round(s.scrollHeight - s.scrollTop - s.clientHeight); })()`);
      scrollCheck.gapAfterMoreText = later;
      jumped = await evaluate(`(() => { const b = document.querySelector(".chat-status-pill-jump"); b?.click(); return Boolean(b); })()`);
      await sleep(900);
      scrollCheck.gapAfterJump = await evaluate(`(() => { const s = document.querySelector(".message-scroll"); return Math.round(s.scrollHeight - s.scrollTop - s.clientHeight); })()`);
    }
    if (state && state.assistant && !state.streaming && state.text > 0) { await sleep(800); break; }
    await sleep(60);
  }
  const after = await metrics();
  const cpu = Object.fromEntries(["ScriptDuration", "LayoutDuration", "RecalcStyleDuration", "TaskDuration"].map((name) => [name, Math.round((after[name] - before[name]) * 1000)]));
  const { data } = await page("Page.captureScreenshot", { format: "png" });
  await writeFile(join(frames, "final.png"), Buffer.from(data, "base64"));
  const probe = await evaluate("window.__probe");
  const summary = { ...summarize(probe, scrollTest ? scrolledAt : undefined), cpuMs: cpu, ...(scrollTest ? { scroll: { ...scrollCheck, jumpButtonFound: jumped } } : {}) };
  await writeFile(join(frames, "frames.json"), JSON.stringify(probe.frames));
  return { summary, shots };
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
  const pinned = await run(browser, origin, "pinned", { scrollTest: false });
  const scrolled = await run(browser, origin, "scrolled-up", { scrollTest: true });
  const result = { size: `${width}x${height}`, theme, reducedMotion, pinned: pinned.summary, scrolledUp: scrolled.summary };
  await writeFile(join(out, "summary.json"), JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result, null, 2));
  console.log(`Probe: ${out}`);
} finally {
  await browser?.close();
  await server?.close();
  if (cacheDir) await rm(cacheDir, { recursive: true, force: true });
}
