// Shared navigation and timing helpers for the native release-gate suites.
// Timings that decide a budget are read inside the page so a WebDriver
// round trip is not part of the measurement.
import { randomUUID } from "node:crypto";

/** Read the name and relationship counts from the visible World status.
 * The persistence suite selects English. Weekly and disagreement counts can
 * change independently of the World size. */
export async function readVisibleMapStats(browser) {
  return browser.execute(() => [...document.querySelectorAll("[data-testid=map-view] [data-testid=map-stats] > span")]
    .filter(element => !element.closest('[hidden], [inert], [aria-hidden="true"]')
      && element.getClientRects().length > 0 && getComputedStyle(element).visibility === "visible")
    .map(element => element.textContent?.trim().split(" · ", 2).join(" · ") ?? "")
    .find(text => /^(?:Showing [\d,]+ of )?[\d,]+ names? · [\d,]+ relationships?$/.test(text)) ?? "");
}

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
    for (const card of await browser.$$("button[data-testid=loupist-template-card]:not([inert] *)")) {
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
 * Sidebar row actions use opacity 0 until :hover or :focus-within.
 * A locked screen matches neither, so reveal the control before clicking.
 */
export async function clickControl(browser, selector) {
  const state = await browser.execute((sel) => {
    const element = document.querySelector(sel);
    if (!(element instanceof HTMLElement)) return "missing";
    const actions = element.closest("[data-testid=loupist-chat-menu]");
    if (actions instanceof HTMLElement) actions.style.opacity = "1";
    return element.matches(":disabled") ? "disabled" : "ready";
  }, selector);
  if (state === "missing") throw new Error(`Control was not in the page: ${selector}`);
  if (state === "disabled") throw new Error(`Control is disabled: ${selector}`);
  const element = await browser.$(selector);
  const popup = await element.getAttribute("aria-haspopup");
  const reportsExpansion = popup === "menu" ||
    (popup === "dialog" && (await element.getAttribute("aria-expanded")) !== null);
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
  if (reportsExpansion) {
    const opened = () => element.getAttribute("aria-expanded").then(value => value === "true");
    if (!await browser.waitUntil(opened, { timeout: 750, interval: 50 }).catch(() => false)) {
      // The pinned WebKit driver can omit Radix's pointerdown. Use the menu's
      // supported keyboard activation, then assert the real expanded state.
      await browser.execute((sel) => document.querySelector(sel)?.focus(), selector);
      await browser.keys("Enter");
      await browser.waitUntil(opened, {
        timeout: 5_000, interval: 100, timeoutMsg: `Popup did not open: ${selector}`,
      });
    }
  }
}

/** Open the Radix model picker and leave it open if it already is. */
export async function openComposerModelMenu(browser, selector = "[data-testid=composer-model-menu]", timeout = 20_000) {
  const picker = `${selector}[aria-haspopup="dialog"]`;
  const trigger = await browser.$(picker);
  await trigger.waitForDisplayed({ timeout });
  await trigger.waitForEnabled({ timeout });
  if ((await trigger.getAttribute("aria-expanded")) !== "true") {
    await clickControl(browser, picker);
  }
  await browser.waitUntil(async () => (await browser.$(picker).getAttribute("aria-expanded")) === "true", {
    timeout: 5_000, interval: 100, timeoutMsg: `Model picker did not open: ${selector}`,
  });
  return trigger;
}

/** Library is a persistent toolbar action at every Map width. */
export async function openMapLibrary(browser, timeout = 15_000) {
  const library = await browser.$("[data-testid=map-view] aside[data-testid=map-library]");
  if (await library.isDisplayed().catch(() => false)) return library;
  const button = await browser.$("[data-testid=map-view] button[data-map-library]");
  await button.waitForDisplayed({ timeout });
  await button.click();
  await library.waitForDisplayed({ timeout, timeoutMsg: "World Library did not open" });
  return library;
}

/** Custom connections start collapsed; preserve an already open disclosure. */
export async function openCustomConnections(browser) {
  const disclosure = await browser.$("[data-custom-connections]");
  if (await disclosure.getAttribute("open") === null) {
    await (await disclosure.$(":scope > summary")).click();
  }
}

/** The settings sheet is also role=dialog, so the first dialog is not the confirmation. */
/**
 * Waits until the host runs nothing: no reply, Memory save or other run in any Loupist
 * (`run_activity_list`). The host refuses to remove a Loupist while one of its runs is active, and a
 * slow runner can still be finishing a reply an earlier step started.
 */
export async function waitForNoRuns(browser, timeout = 60_000) {
  let runs = [];
  await browser.waitUntil(async () => {
    runs = await browser.execute(() => window.__TAURI_INTERNALS__.invoke("run_activity_list")).catch(() => null) ?? runs;
    return Array.isArray(runs) && runs.length === 0;
  }, { timeout, interval: 200 }).catch((error) => {
    throw new Error(`Runs were still active: ${JSON.stringify(runs.map(({ kind, loupist_id }) => ({ kind, loupist_id })))}`, { cause: error });
  });
}

/** Waits until no menu, popover or dialog is open, so the next control is not behind a layer. */
export async function waitForNoLayers(browser, timeout = 10_000) {
  let open = [];
  await browser.waitUntil(async () => {
    open = await browser.execute(() => [...document.querySelectorAll('[role="menu"], [role="dialog"], [role="alertdialog"], [data-radix-popper-content-wrapper]')]
      .filter((element) => element.getClientRects().length && !element.closest("[data-sidebar=sidebar], [hidden]"))
      // Hover can leave a non-blocking Tooltip on the settled composer.
      .filter((element) => !element.matches("[data-radix-popper-content-wrapper]") || !element.querySelector('[data-slot="tooltip-content"]'))
      .map((element) => element.getAttribute("role") ?? "popper"));
    return open.length === 0;
  }, { timeout, interval: 100 }).catch((error) => {
    throw new Error(`Layers were still open: ${JSON.stringify(open)}`, { cause: error });
  });
}

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
    return state.lang === "en" && /Get started|Connect your AI account|Create your first Loupist|New Loupist|Bot|Following|Research sessions/.test(state.text);
  }, {
    timeout: 20_000,
    interval: 150,
    timeoutMsg: "The interface did not switch to English",
  });
  if (failure) throw new Error(`Could not set the interface language to English: ${failure}`);
}

export async function selectedLoupistTitle(browser) {
  return browser.execute(() => {
    const identity = document.querySelector("[data-testid=loupist-header-identity]");
    // The icon rail shows only the face; the name is then the button's label.
    return (identity?.querySelector("strong")?.textContent ?? identity?.getAttribute("aria-label") ?? "").trim();
  });
}

const loupistSwitcherTrigger = "[data-testid=loupist-header-identity]";
const loupistSwitcherMenu = ".loupist-switcher-menu";
const openSwitcherMenu = `${loupistSwitcherMenu}[data-state="open"]`;

/** Check a named button in the rendered WebView, without its driver visibility calculation. */
export async function waitForRenderedButton(browser, root, label, timeout = 15_000) {
  await browser.waitUntil(() => browser.execute((selector, name) => {
    const card = document.querySelector(selector);
    if (!card || card.closest("[hidden], [inert]")) return false;
    const button = [...card.querySelectorAll("button")].find(element =>
      element.textContent?.trim() === name && !element.disabled &&
      element.getClientRects().length > 0 && getComputedStyle(element).visibility === "visible");
    if (!button) return false;
    button.scrollIntoView({ block: "center" });
    return true;
  }, root, label), { timeout, interval: 100, timeoutMsg: `No rendered ${label} button in ${root}` });
}

/** Opens the Loupist switcher at the top of the sidebar: every Loupist, its settings and New Loupist. */
export async function openLoupistSwitcher(browser, timeout = 5_000) {
  const trigger = await browser.$(loupistSwitcherTrigger);
  if (await trigger.getAttribute("aria-expanded") !== "true") {
    await clickControl(browser, loupistSwitcherTrigger);
  }
  await browser.waitUntil(() => browser.execute((selector) => {
    const menu = document.querySelector(selector);
    return Boolean(menu?.getClientRects().length && getComputedStyle(menu).visibility === "visible");
  }, openSwitcherMenu), {
    timeout, interval: 100, timeoutMsg: "The Loupist switcher did not open",
  });
}

/** Wait out Radix's exit animation and focus restoration before the next action. */
export async function waitForLoupistSwitcherClosed(browser, timeout = 5_000) {
  await browser.$(loupistSwitcherMenu).waitForExist({
    reverse: true, timeout, timeoutMsg: "The Loupist switcher did not close",
  });
}

/** Every Loupist's id and name, read from the switcher, which is closed again. */
export async function switcherLoupists(browser) {
  await openLoupistSwitcher(browser);
  const loupists = await browser.execute(selector => [...document.querySelectorAll(`${selector} [data-loupist-option]`)].map((item) => ({
    id: item.getAttribute("data-loupist-option"),
    name: item.querySelector(".loupist-switcher-name")?.textContent?.trim() ?? "",
  })), openSwitcherMenu);
  // Keep the focus check and Escape in one WebView call. A portal can be
  // visible before its dismissal handler is ready, especially after a dialog.
  let lastState;
  try {
    await browser.waitUntil(async () => {
      lastState = await browser.execute(selector => {
        const menu = document.querySelector(selector);
        if (!menu) return "closed";
        const target = document.activeElement;
        if (!menu.contains(target)) return "focus outside menu";
        const escape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
        return target.dispatchEvent(escape) ? "Escape unhandled" : "Escape handled";
      }, openSwitcherMenu);
      return lastState === "closed";
    }, { timeout: 5_000, interval: 100 });
  } catch (cause) {
    throw new Error(`The Loupist switcher did not close (last WebView state: ${lastState ?? "unknown"})`, { cause });
  }
  await waitForLoupistSwitcherClosed(browser);
  return loupists;
}

/** Opens Loupist settings or New Loupist from the switcher. */
export async function loupistSwitcherAction(browser, action) {
  await openLoupistSwitcher(browser);
  const item = action === "settings" ? "[data-header-loupist-settings]" : "[data-testid=loupist-header-new]";
  await clickControl(browser, `${openSwitcherMenu} ${item}`);
}

/**
 * Open Bot, Following, Map, Lens, Schedule or Settings. Settings replaces the sidebar, so
 * it is left through its sidebar footer first.
 */
export async function openMainTab(browser, tab, timeout = 15_000) {
  if (!["chat", "following", "map", "lens", "schedule", "briefings", "settings"].includes(tab)) throw new Error(`Unknown main tab: ${tab}`);
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
    await browser.$("[data-testid=loupist-header-selector]").waitForDisplayed({ timeout });
  }
  if (tab === "map" || tab === "lens") {
    // Map and Lens are pages; opening one that is already shown is a no-op,
    // so successive reader checks can call this freely.
    if (await visible(tab === "map" ? "[data-testid=map-view]" : "[data-testid=memory-page]")) return;
    await clickControl(browser, `[data-sidebar-place="${tab}"] button`);
    await browser.$(tab === "map" ? "[data-testid=map-view]" : "[data-testid=memory-page]").waitForDisplayed({ timeout });
  } else if (tab === "chat") {
    if (!await visible("#main-panel-chat")) await clickControl(browser, "[data-testid=loupist-chat-row][data-home] [data-testid=loupist-chat-button]");
    if (await visible("[data-workspace-close]")) await clickControl(browser, "[data-workspace-close]");
    await browser.$("#main-panel-chat").waitForDisplayed({ timeout });
  } else {
    const place = tab === "following" ? "following" : "schedule";
    await clickControl(browser, `[data-sidebar-place="${place}"] button`);
    await browser.$(`#main-panel-${place}`).waitForDisplayed({ timeout });
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
      const tab = await browser.$("[data-testid=loupist-header-selector]");
      return (await tab.isExisting()) && (await tab.isDisplayed());
    },
    { timeout, interval: 150, timeoutMsg: "The Chat/World/Briefings shell did not open" },
  );
}

export async function pageTiming(browser) {
  return browser.execute(() => {
    const navigation = performance.getEntriesByType("navigation")[0];
    const paint = performance.getEntriesByName("first-contentful-paint")[0];
    const largest = performance.getEntriesByType("largest-contentful-paint").at(-1);
    const shell = Boolean(document.querySelector("[data-testid=loupist-header-selector]"));
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
  await openLoupistSwitcher(browser);
  const startMeasurement = (name) => {
    const option = [...document.querySelectorAll(".loupist-switcher-menu [data-loupist-option]")]
      .find((item) => item.querySelector(".loupist-switcher-name")?.textContent?.trim() === name);
    if (!option) return null;
    if (option.getAttribute("aria-checked") === "true") return { error: `Loupist was already selected: ${name}` };
    const target = option.getAttribute("data-loupist-option");
    const current = () => document.querySelector("[data-testid=loupist-header-identity]")?.getAttribute("data-loupist-id");
    const start = performance.now();
    window.__loupistSwitchMeasure = { pending: true, title: name };
    const finish = (result) => {
      window.__loupistSwitchMeasure = result;
    };
    const observer = new MutationObserver(() => {
      if (current() === target) {
        observer.disconnect();
        finish({ ms: performance.now() - start, title: name });
      }
    });
    observer.observe(document.body, { attributes: true, childList: true, subtree: true, attributeFilter: ["data-loupist-id"] });
    window.setTimeout(() => {
      if (window.__loupistSwitchMeasure?.pending) {
        observer.disconnect();
        finish({ error: `Loupist switch timed out: ${name}` });
      }
    }, 5_000);
    option.click();
    return { pending: true };
  };
  let started;
  try {
    await browser.waitUntil(
      async () => {
        // A list refresh after a Loupist is created can close the open menu.
        await openLoupistSwitcher(browser);
        started = await browser.execute(startMeasurement, title);
        return Boolean(started);
      },
      { timeout: 15_000, interval: 150 },
    );
  } catch {
    const listed = await browser.execute(() => {
      const menu = document.querySelector(".loupist-switcher-menu");
      return menu ? [...menu.querySelectorAll(".loupist-switcher-name")].map((item) => item.textContent?.trim() ?? "").join(", ") : "menu closed";
    });
    throw new Error(`Loupist was not in the switcher: ${title} (listed: ${listed})`);
  }
  if (started?.error) return started;
  await browser.waitUntil(
    async () => {
      const result = await browser.execute(() => window.__loupistSwitchMeasure ?? null);
      return Boolean(result && !result.pending);
    },
    { timeout: 6_000, interval: 50, timeoutMsg: `Loupist switch did not settle: ${title}` },
  );
  await waitForLoupistSwitcherClosed(browser);
  return browser.execute(() => window.__loupistSwitchMeasure);
}

export async function mapNameCount(browser) {
  const stats = await browser.$("[data-testid=map-stats]");
  if (!(await stats.isExisting())) return null;
  const text = (await stats.getText()).replace(/\s+/g, " ");
  // "37 on the map · …" on the whole Map; "12 of 37 on the map around …" around one company.
  const match = text.match(/([\d,]+)\s+(?:in World|names)/);
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
        // A top-level error field is reserved by the native script executor.
        if (status.previewReady === "error") return { previewError: status.previewError || "Preview initialization failed" };
        if (status.previewReady !== "true") return null;
        const host = document.querySelector('#main-panel-map [data-testid=map-view] [data-testid=map-canvas]');
        const element = host?.querySelector("canvas");
        const box = element?.getBoundingClientRect();
        return {
          text: (host?.querySelector("[data-testid=map-probe]")?.textContent ?? "").replace(/\s+/g, " ").trim(),
          canvas: Boolean(element && box && box.width > 1 && box.height > 1),
          firstPaintMs: host?.getAttribute("data-first-paint-ms") || null,
        };
      }, url);
      if (snapshot?.previewError) return true;
      parsed = parse(snapshot?.text ?? "");
      return Boolean(parsed && snapshot?.canvas);
    },
    {
      timeout,
      interval: 250,
      timeoutMsg: `The ${scenario} map probe did not report a summary on a rendered canvas at ${url}`,
    },
  );
  if (snapshot?.previewError) throw new Error(`The ${scenario} preview failed: ${snapshot.previewError}`);
  return { ...parsed, canvas: snapshot.canvas, firstPaintMs: snapshot.firstPaintMs, url };
}

export function scrub(text) {
  return String(text ?? "")
    .replace(/\b(?:sk|rk|pk)-[A-Za-z0-9_\-.]{8,}\b/g, "[redacted]")
    .replace(/\beyJ[A-Za-z0-9_\-.]{12,}\b/g, "[redacted]")
    .replace(/Bearer\s+\S+/gi, "Bearer [redacted]");
}
