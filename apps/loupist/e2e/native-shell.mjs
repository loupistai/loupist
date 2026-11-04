// Shared navigation and timing helpers for the native release-gate suites.
// Timings that decide a budget are read inside the page so a WebDriver
// round trip is not part of the measurement.
import { randomUUID } from "node:crypto";

export async function onboardingStep(browser) {
  const main = await browser.$("main[data-onboarding]");
  if (!(await main.isExisting())) return null;
  try {
    if (!(await main.isDisplayed())) return null;
  } catch {
    return null;
  }
  return await main.getAttribute("data-onboarding");
}

export async function waitForOnboardingStep(browser, step, timeout = 20_000) {
  await browser.waitUntil(async () => (await onboardingStep(browser)) === String(step), {
    timeout,
    interval: 100,
    timeoutMsg: `Onboarding did not reach step ${step}`,
  });
}

/** The step marker changes before the template cards finish their entry animation. */
export async function selectLoupistTemplate(browser, name, timeout = 15_000) {
  let clicked = false;
  await browser.waitUntil(async () => {
    for (const card of await browser.$$("button.loupist-template-card:not([inert] *)")) {
      try {
        if (!(await card.isDisplayed()) || !(await card.isEnabled())
          || !(await card.getText()).trim().startsWith(name)) continue;
        if (await card.getAttribute("aria-pressed") === "true") return true;
        if (!clicked) {
          await card.click();
          clicked = true;
        }
      } catch (error) {
        // A replaced card is not ready; an actual driver/click failure stays a failure.
        if (!/no longer attached|stale element/i.test(String(error?.message ?? error))) throw error;
      }
    }
    return false;
  }, { timeout, interval: 100, timeoutMsg: `Loupist template did not become selected: ${name}` });
}

/**
 * Leaves onboarding step 5 ("Meet your Loupist"). With followed assets it answers the
 * morning-update question (`morningUpdate`); without them the only action is Start chatting.
 * Returns which action it took.
 */
export async function finishMeetStep(browser, { morningUpdate = true, timeout = 30_000 } = {}) {
  await waitForOnboardingStep(browser, 5, timeout);
  const consent = await browser.$(`[data-onboarding-consent="${morningUpdate ? "yes" : "no"}"]`);
  if (await consent.isDisplayed().catch(() => false)) {
    await consent.click();
    return morningUpdate ? "morning-update" : "declined";
  }
  const start = await browser.$("[data-onboarding-start-chat]");
  await start.waitForDisplayed({ timeout: 5_000, timeoutMsg: "Step 5 offered no action" });
  await start.click();
  return "start";
}

/**
 * Sidebar row actions use opacity 0 until :hover or :focus-within.
 * A locked screen matches neither, so reveal the control before clicking.
 */
export async function clickControl(browser, selector) {
  const state = await browser.execute((sel) => {
    const element = document.querySelector(sel);
    if (!(element instanceof HTMLElement)) return "missing";
    const actions = element.closest(".loupist-row-actions, .loupist-chat-menu");
    if (actions instanceof HTMLElement) actions.style.opacity = "1";
    return element.matches(":disabled") ? "disabled" : "ready";
  }, selector);
  if (state === "missing") throw new Error(`Control was not in the page: ${selector}`);
  if (state === "disabled") throw new Error(`Control is disabled: ${selector}`);
  const element = await browser.$(selector);
  const isMenu = await element.getAttribute("aria-haspopup") === "menu";
  if (await element.isDisplayed().catch(() => false)) {
    await element.click();
  } else {
    const clicked = await browser.execute((sel) => {
      const element = document.querySelector(sel);
      if (!(element instanceof HTMLElement)) return false;
      element.click();
      return true;
    }, selector);
    if (!clicked) throw new Error(`Control could not be activated: ${selector}`);
  }
  if (isMenu) {
    const opened = () => element.getAttribute("aria-expanded").then(value => value === "true");
    if (!await browser.waitUntil(opened, { timeout: 750, interval: 50 }).catch(() => false)) {
      // The pinned WebKit driver can omit Radix's pointerdown. Use the menu's
      // supported keyboard activation, then assert the real expanded state.
      await browser.execute((sel) => document.querySelector(sel)?.focus(), selector);
      await browser.keys("Enter");
      await browser.waitUntil(opened, {
        timeout: 5_000, interval: 100, timeoutMsg: `Menu did not open: ${selector}`,
      });
    }
  }
}

/** The settings sheet is also role=dialog, so the first dialog is not the confirmation. */
export async function dialogByTitle(browser, title, timeout = 20_000) {
  await browser.waitUntil(
    async () => browser.execute((expected) =>
      [...document.querySelectorAll('[role="dialog"]')].some((dialog) =>
        (dialog.querySelector("[data-slot='dialog-title']")?.textContent ?? "").includes(expected),
      ), title),
    { timeout, interval: 150, timeoutMsg: `Dialog “${title}” did not appear` },
  );
  for (const candidate of await browser.$$('[role="dialog"]')) {
    const heading = await candidate.$('[data-slot="dialog-title"]');
    if ((await heading.isExisting()) && (await heading.getText()).includes(title)) return candidate;
  }
  throw new Error(`Dialog “${title}” disappeared`);
}

/** Save appearance.language=en through the existing configuration command. */
export async function preferEnglish(browser) {
  await browser.execute(() => {
    const internals = window.__TAURI_INTERNALS__;
    if (!internals?.invoke) {
      window.__e2eLanguage = { pending: false, error: "Tauri invoke is not available" };
      return;
    }
    window.__e2eLanguage = { pending: true };
    internals.invoke("config_read", { request: { scope: { kind: "global" } } })
      .then((snapshot) => {
        if (snapshot?.effective?.appearance?.language === "en") return null;
        return internals.invoke("config_update", {
          request: {
            scope: snapshot.scope,
            expectedRevision: snapshot.revision,
            set: { appearance: { language: "en" } },
            unset: [],
          },
        });
      })
      .then((updated) => {
        if (updated) {
          window.dispatchEvent(new CustomEvent("loupist-configuration-changed"));
          window.dispatchEvent(new Event("focus"));
        }
        window.__e2eLanguage = { pending: false, ok: true };
      })
      .catch((error) => {
        window.__e2eLanguage = { pending: false, error: String(error?.message || error) };
      });
  });
  let failure = null;
  await browser.waitUntil(async () => {
    const state = await browser.execute(() => ({
      language: window.__e2eLanguage ?? null,
      lang: document.documentElement.lang,
      text: document.body?.innerText?.slice(0, 4_000) ?? "",
    }));
    if (state.language?.error) {
      failure = state.language.error;
      return true;
    }
    return state.lang === "en" && /Get started|New Loupist|My Bot|Research sessions|\bChat\b/.test(state.text);
  }, {
    timeout: 20_000,
    interval: 150,
    timeoutMsg: "The interface did not switch to English",
  });
  if (failure) throw new Error(`Could not set the interface language to English: ${failure}`);
}

export async function selectedLoupistTitle(browser) {
  return browser.execute(() =>
    document.querySelector(".loupist-header-identity[aria-pressed='true'] strong")?.textContent?.trim() ?? "",
  );
}

/**
 * Open Chat, Map, Briefings or Settings. Settings replaces the header tabs, so
 * it is left through its sidebar footer first.
 */
export async function openMainTab(browser, tab, timeout = 15_000) {
  if (!["chat", "map", "lens", "schedule", "briefings", "settings"].includes(tab)) throw new Error(`Unknown main tab: ${tab}`);
  const visible = (selector) => browser.$(selector).isDisplayed().catch(() => false);
  if (await visible("[data-loupist-settings-back]")) await (await browser.$("[data-loupist-settings-back]")).click();
  const inSettings = await visible("#main-panel-settings");
  if (tab === "settings") {
    if (!inSettings) await (await browser.$("#main-tab-settings")).click();
    await browser.$("#main-panel-settings").waitForDisplayed({ timeout, timeoutMsg: "Settings did not open" });
    return;
  }
  if (inSettings) {
    let back;
    for (const button of await browser.$$('[data-sidebar="footer"] button')) {
      if (await button.isDisplayed()) back = button;
    }
    if (!back) throw new Error("The Settings sidebar has no visible way back");
    await back.click();
    await browser.$(".loupist-header-selector").waitForDisplayed({ timeout });
  }
  if (tab === "map" || tab === "lens") {
    await clickControl(browser, ".header-panel-toggle");
    const item = await browser.$(`[data-workspace-kind="${tab === "map" ? "world" : "lens"}"]`);
    await item.waitForDisplayed({ timeout });
    await item.click();
    await browser.$(tab === "map" ? ".map-view" : ".memory-page").waitForDisplayed({ timeout });
  } else if (tab === "chat") {
    if (!await visible("#main-panel-chat")) await clickControl(browser, ".loupist-chat-row[data-home] .loupist-chat-button");
    if (await visible("[data-workspace-close]")) await clickControl(browser, "[data-workspace-close]");
    await browser.$("#main-panel-chat").waitForDisplayed({ timeout });
  } else {
    await clickControl(browser, "[data-bot-menu]");
    await clickControl(browser, "[data-bot-schedule]");
    await browser.$("#main-panel-schedule").waitForDisplayed({ timeout });
  }
}

/** Acknowledge the launch notice through its dialog. Returns whether it was required. */
export async function acknowledgeLaunchNotice(browser, timeout = 30_000) {
  const settings = await browser.execute(async () => window.__TAURI_INTERNALS__.invoke("notice_settings_get"));
  if (settings.notice_acknowledged_version >= 1) return false;
  const dialog = await browser.$("[data-notice-dialog]");
  await dialog.waitForDisplayed({ timeout, timeoutMsg: "The required launch notice did not appear" });
  await (await dialog.$("[data-notice-acknowledgment]")).click();
  await (await dialog.$("button")).click();
  await browser.waitUntil(async () => !(await browser.$("[data-notice-dialog]").isExisting()), {
    timeout: 15_000, interval: 100, timeoutMsg: "Notice acknowledgment did not finish",
  });
  return true;
}

export async function waitForShell(browser, timeout = 30_000) {
  await browser.waitUntil(
    async () => {
      const tab = await browser.$(".loupist-header-selector");
      return (await tab.isExisting()) && (await tab.isDisplayed());
    },
    { timeout, interval: 150, timeoutMsg: "The Chat/Map/Briefings shell did not open" },
  );
}

export async function pageTiming(browser) {
  return browser.execute(() => {
    const navigation = performance.getEntriesByType("navigation")[0];
    const paint = performance.getEntriesByName("first-contentful-paint")[0];
    const largest = performance.getEntriesByType("largest-contentful-paint").at(-1);
    const shell = Boolean(document.querySelector(".loupist-header-selector"));
    const interactive = shell || Boolean(document.querySelector("main[data-onboarding] h1"));
    return {
      domContentLoadedMs: navigation?.domContentLoadedEventEnd ?? null,
      loadMs: navigation?.loadEventEnd ?? null,
      firstContentfulPaintMs: paint?.startTime ?? null,
      largestContentfulPaintMs: largest?.startTime ?? null,
      observedMs: performance.now(),
      interactivePresent: interactive,
      shell,
    };
  });
}

export function coldStartMark(timing) {
  const candidates = [
    timing.largestContentfulPaintMs,
    timing.firstContentfulPaintMs,
    timing.domContentLoadedMs,
  ].filter((value) => Number.isFinite(value) && value > 0);
  const contentMs = candidates.length ? Math.min(...candidates) : null;
  // Once the control is on screen, a late WebDriver attach makes observedMs
  // an upper bound. The content mark is the page's own paint time.
  const budgetMs = timing.interactivePresent && contentMs != null ? contentMs : timing.observedMs;
  return { ...timing, budgetMs };
}

export async function measureLoupistSwitch(browser, title) {
  const started = await browser.execute((name) => {
    const buttons = () => [...document.querySelectorAll("button.loupist-header-identity")];
    const button = buttons().find((item) => item.querySelector("strong")?.textContent?.trim() === name);
    if (!button) return { error: `Loupist button was not in the header: ${name}` };
    if (button.getAttribute("aria-pressed") === "true") {
      return { error: `Loupist was already selected: ${name}` };
    }
    const start = performance.now();
    window.__loupistSwitchMeasure = { pending: true, title: name };
    const finish = (result) => {
      window.__loupistSwitchMeasure = result;
    };
    const observer = new MutationObserver(() => {
      const current = buttons().find((item) => item.querySelector("strong")?.textContent?.trim() === name);
      if (current?.getAttribute("aria-pressed") === "true") {
        observer.disconnect();
        finish({ ms: performance.now() - start, title: name });
      }
    });
    observer.observe(document.body, { attributes: true, subtree: true, attributeFilter: ["aria-pressed"] });
    window.setTimeout(() => {
      if (window.__loupistSwitchMeasure?.pending) {
        observer.disconnect();
        finish({ error: `Loupist switch timed out: ${name}` });
      }
    }, 5_000);
    button.click();
    return { pending: true };
  }, title);
  if (started?.error) return started;
  await browser.waitUntil(
    async () => {
      const result = await browser.execute(() => window.__loupistSwitchMeasure ?? null);
      return Boolean(result && !result.pending);
    },
    { timeout: 6_000, interval: 50, timeoutMsg: `Loupist switch did not settle: ${title}` },
  );
  return browser.execute(() => window.__loupistSwitchMeasure);
}

export async function mapNameCount(browser) {
  const stats = await browser.$(".map-stats");
  if (!(await stats.isExisting())) return null;
  const text = (await stats.getText()).replace(/\s+/g, " ");
  // "37 on the map · …" on the whole Map; "12 of 37 on the map around …" around one company.
  const match = text.match(/([\d,]+)\s+(?:on the map|names)/);
  return {
    text,
    names: match ? Number(match[1].replaceAll(",", "")) : null,
  };
}

export function parseMapDrawProbe(text) {
  const match = String(text).match(
    /1000 nodes · draw median ([\d.]+) ms · draw p95 ([\d.]+) ms/,
  );
  if (!match) return null;
  return {
    kind: "draw-cost",
    text: match[0],
    medianMs: Number(match[1]),
    p95Ms: Number(match[2]),
    note: "Proxy for the 33 ms frame budget. This is draw cost, not frame cadence.",
  };
}

export async function measureMapDrawProbe(browser, devUrl, timeout = 120_000) {
  return measurePreviewMapProbe(browser, devUrl, "map-1000", "draw", timeout, parseMapDrawProbe);
}

export function parseMapProbe(text) {
  const match = String(text).match(
    /(\d+) nodes · median ([\d.]+) ms · p95 ([\d.]+) ms/,
  );
  if (!match) return null;
  return { text: match[0], nodes: Number(match[1]), medianMs: Number(match[2]), p95Ms: Number(match[3]) };
}

/** Frame times on a preview Map: the 1,000-node overview by default, or another Map scenario such as the two-hop view. */
export async function measureMapProbe(browser, devUrl, { scenario = "map-1000", timeout = 120_000 } = {}) {
  return measurePreviewMapProbe(browser, devUrl, scenario, "1", timeout, parseMapProbe);
}

async function measurePreviewMapProbe(browser, devUrl, scenario, mode, timeout, parse) {
  // The native driver returns after assigning location.href, before navigation commits.
  // A unique URL also distinguishes a repeated measurement of the same scenario.
  const target = new URL("preview.html", `${String(devUrl).replace(/\/$/, "")}/`);
  target.search = new URLSearchParams({ scenario, probe: mode, nativeProbe: randomUUID() }).toString();
  const url = target.href;
  await browser.url(url);
  await browser.switchToWindow("main").catch(() => {});
  let snapshot = null;
  let parsed = null;
  await browser.waitUntil(
    async () => {
      // Read all evidence in one document turn, from the main Map rather than a
      // hidden Chat mini-map. Never combine an old summary with a new canvas.
      snapshot = await browser.execute((expectedUrl) => {
        if (location.href !== expectedUrl) return null;
        const status = document.documentElement.dataset;
        if (status.previewReady === "error") return { error: status.previewError || "Preview initialization failed" };
        if (status.previewReady !== "true") return null;
        const host = document.querySelector('#workspace-panel[data-panel-kind="world"] .map-view .map-canvas');
        const element = host?.querySelector("canvas");
        const box = element?.getBoundingClientRect();
        return {
          text: (host?.querySelector(".map-probe")?.textContent ?? "").replace(/\s+/g, " ").trim(),
          canvas: Boolean(element && box && box.width > 1 && box.height > 1),
          firstPaintMs: host?.getAttribute("data-first-paint-ms") || null,
        };
      }, url);
      if (snapshot?.error) return true;
      parsed = parse(snapshot?.text ?? "");
      return Boolean(parsed && snapshot?.canvas);
    },
    {
      timeout,
      interval: 250,
      timeoutMsg: `The ${scenario} map probe did not report a summary on a rendered canvas at ${url}`,
    },
  );
  if (snapshot?.error) throw new Error(`The ${scenario} preview failed: ${snapshot.error}`);
  return { ...parsed, canvas: snapshot.canvas, firstPaintMs: snapshot.firstPaintMs, url };
}

export function scrub(text) {
  return String(text ?? "")
    .replace(/\b(?:sk|rk|pk)-[A-Za-z0-9_\-.]{8,}\b/g, "[redacted]")
    .replace(/\beyJ[A-Za-z0-9_\-.]{12,}\b/g, "[redacted]")
    .replace(/Bearer\s+\S+/gi, "Bearer [redacted]");
}
