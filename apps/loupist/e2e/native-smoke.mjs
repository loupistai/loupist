import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { remote } from "webdriverio";
import { createWebdriverHelpers } from "./webdriver-helpers.mjs";
import { minimumWindowSize, setWindowSize, withWindowSize } from "./window-geometry.mjs";
import { focusNativeWindow } from "./native-window-focus.mjs";
import { assertFormLegendsFit } from "./native-forms.mjs";
import { verifyNativeBrowserNavigation } from "./native-browser.mjs";
import {
  coldStartMark,
  clickControl,
  dialogByTitle,
  mapNameCount,
  measureLoupistSwitch,
  measureMapDrawProbe,
  measureMapProbe,
  onboardingStep,
  pageTiming,
  preferEnglish,
  scrub,
  selectedLoupistTitle,
  selectLoupistTemplate,
  waitForOnboardingStep,
  openMainTab,
  waitForShell,
} from "./native-shell.mjs";

const e2eRoot = dirname(fileURLToPath(import.meta.url));
const sessionPath = process.argv[2];
const extraArgs = process.argv.slice(3);
const full = extraArgs.includes("--full");
assert.ok(
  sessionPath && extraArgs.every((arg) => arg === "--full"),
  "usage: node native-smoke.mjs <current-session.json> [--full]",
);

const session = JSON.parse(await readFile(sessionPath, "utf8"));
console.log("native smoke: creating WebDriver session");
const browser = await remote({
  ...session.webdriverConfig,
  capabilities: session.capabilities,
  connectionRetryTimeout: 15_000,
  connectionRetryCount: 0,
  logLevel: "error",
});

const artifactRoot = resolve(e2eRoot, "artifacts");
await mkdir(artifactRoot, { recursive: true });

const {
  displayed,
  childWithText,
  clickButton,
  waitForText,
  waitForTextGone,
} = createWebdriverHelpers(browser, {
  defaultTimeout: 15_000,
  bodyTextLimit: 16_000,
});

const performanceReport = {
  build: "isolated-debug-e2e",
  signedRelease: false,
  mode: process.env.LOUPIST_E2E_ALLOW_UNFOCUSED === "1" ? "webview-only" : "focused-native",
  providerMarker: "LOUPIST_E2E_PROVIDER_READY",
  budgets: {},
};

async function step(name, action) {
  process.stdout.write(`• ${name} ... `);
  await action();
  console.log("passed");
}

const capture = (name) =>
  browser.saveScreenshot(resolve(artifactRoot, `native-smoke-${name}.png`));

async function clickTemplate(name) {
  await selectLoupistTemplate(browser, name);
}

async function openSettings(sectionId) {
  const section = await browser.$(`#settings-section-${sectionId}`);
  const alreadyOpen = (await section.isExisting()) && (await section.isDisplayed().catch(() => false));
  if (!alreadyOpen) await (await displayed("#main-tab-settings")).click();
  await (await displayed(`#settings-section-${sectionId}`)).click();
  await displayed(`#settings-section-${sectionId}[aria-current="page"]`);
}

async function visibleLoupistTitles() {
  return browser.execute(() =>
    [...document.querySelectorAll("button.loupist-header-identity")]
      .filter((button) => button.getClientRects().length > 0)
      .map((button) => button.querySelector("strong")?.textContent?.trim() ?? ""),
  );
}

async function removeUntilOnboarding() {
  for (let guard = 0; guard < 6; guard += 1) {
    if ((await onboardingStep(browser)) === "1") return;
    const titles = await visibleLoupistTitles();
    if (!titles.length) break;
    await (await displayed("[data-header-loupist-settings]")).click();
    const sheet = await displayed(".loupist-settings-page");
    await clickButton("Remove Loupist", sheet);
    const dialog = await dialogByTitle(browser, "Remove Loupist?");
    await clickButton("Remove Loupist", dialog);
    await browser.pause(400);
  }
  await waitForOnboardingStep(browser, 1, 20_000);
}

async function selectOption(triggerSelector, optionText) {
  const trigger = await displayed(triggerSelector);
  await trigger.click();
  for (let attempt = 0; attempt < 15; attempt += 1) {
    for (const option of await browser.$$('[role="option"]')) {
      if ((await option.isDisplayed()) && (await option.getText()).trim() === optionText) {
        await option.click();
        return;
      }
    }
    await browser.pause(100);
  }
  await browser.keys(optionText);
  await browser.keys("Enter");
  await browser.waitUntil(async () => (await trigger.getText()).trim() === optionText, {
    timeout: 5_000,
    interval: 100,
    timeoutMsg: `Exact visible option was not retained: ${optionText}`,
  });
}

try {
  await focusNativeWindow(browser, session);
  await step("desktop window and CSS viewport", async () => {
    console.log(JSON.stringify(await setWindowSize(browser)));
  });

  await step("cold start to an interactive shell", async () => {
    await browser.waitUntil(async () => (await pageTiming(browser)).interactivePresent, {
      timeout: 60_000,
      timeoutMsg: "No interactive shell or onboarding control appeared",
    });
    const timing = coldStartMark(await pageTiming(browser));
    performanceReport.coldStart = {
      ...timing,
      surface: timing.shell ? "shell" : "onboarding",
      spawnToObservedMs: Number.isInteger(session.spawnedAtMs)
        ? Date.now() - session.spawnedAtMs
        : null,
      budgetMsLimit: 3_000,
      pass: Boolean(timing.shell) && timing.budgetMs <= 3_000,
      note: "budgetMs is the in-page paint mark. spawnToObservedMs includes process launch and is not the budget.",
    };
    console.log(JSON.stringify(performanceReport.coldStart));
  });

  await step("acknowledge the launch notice when required", async () => {
    const settings = await browser.execute(async () => window.__TAURI_INTERNALS__.invoke("notice_settings_get"));
    if (settings.notice_acknowledged_version >= 1) return;
    await browser.waitUntil(async () => Boolean(await onboardingStep(browser)) || (await browser.$("[data-notice-dialog]").isDisplayed()), {
      timeout: 30_000, timeoutMsg: "Neither onboarding nor the launch notice appeared",
    });
    if (await onboardingStep(browser)) return;
    const dialog = await displayed("[data-notice-dialog]");
    await (await dialog.$("[data-notice-acknowledgment]")).click();
    await (await dialog.$("button")).click();
    await browser.waitUntil(async () => !(await browser.$("[data-notice-dialog]").isExisting()), {
      timeout: 15_000, timeoutMsg: "Notice acknowledgment did not finish",
    });
  });

  await step("English interface and a clear onboarding start", async () => {
    await preferEnglish(browser);
    await removeUntilOnboarding();
    await waitForOnboardingStep(browser, 1, 60_000);
    await waitForText(await displayed("main[data-onboarding]"), "A research companion that learns what you care about");
    await displayed("[data-onboarding-start]");
    await capture("onboarding-welcome");
  });

  await step("a connected account picks a preset and enters its Bot", async () => {
    await (await displayed("[data-notice-acknowledgment]")).click();
    await clickButton("Get started");
    await waitForOnboardingStep(browser, 3, 20_000);
    await (await displayed("[data-onboarding-create]")).click();
    await waitForShell(browser, 30_000);
    assert.equal(await selectedLoupistTitle(browser), "Portfolio Loupist");
    await displayed(".loupist-chat-row[data-home] [aria-current=page]");
    await capture("onboarding-bot");
  });

  await step("shell, Loupist settings, and a new Chat draft", async () => {
    await openMainTab(browser, "lens");
    await displayed(".memory-page");
    await openMainTab(browser, "chat");
    await waitForText(await displayed("#main-panel-chat"), "AI can be wrong. Not a licensed financial adviser.");
    const before = (await browser.$$(".loupist-chat-button")).length;
    const newChatLabel = '.loupist-list-heading button';
    await clickControl(browser, newChatLabel);
    const afterFirst = (await browser.$$(".loupist-chat-button")).length;
    await clickControl(browser, newChatLabel);
    // An unsent topic Chat shows as one draft row under the Thread; a second New chat reuses it.
    assert.ok(afterFirst <= before + 1, "An unsent Chat shows at most one draft row");
    assert.equal((await browser.$$(".loupist-chat-button")).length, afterFirst, "A second New chat must reuse the unsent Chat");
    await (await displayed("[data-header-loupist-settings]")).click();
    const sheet = await displayed(".loupist-settings-page");
    await waitForText(sheet, "Loupist settings");
    await clickButton("Remove Loupist", sheet);
    const dialog = await dialogByTitle(browser, "Remove Loupist?");
    await capture("loupist-settings");
    await clickButton("Cancel", dialog);
    await browser.waitUntil(
      async () => !(await browser.execute(() =>
        [...document.querySelectorAll("[data-slot='dialog-title']")].some((node) =>
          node.textContent?.includes("Remove Loupist?"),
        ),
      )),
      { timeout: 5_000, timeoutMsg: "Remove Loupist confirmation did not close" },
    );
    const close = await sheet.$("[data-loupist-settings-back]");
    if (await close.isExisting()) await close.click();
    const titles = await visibleLoupistTitles();
    assert.ok(titles.includes("Portfolio Loupist"), `Sidebar loupists: ${titles.join(", ")}`);
    await capture("shell");
    performanceReport.shellObservedMs = (await pageTiming(browser)).observedMs;
  });

  await step("Loupist switch", async () => {
    let titles = (await visibleLoupistTitles()).filter(Boolean);
    if (titles.length < 2) {
      await (await displayed('button[aria-label="New Loupist"]')).click();
      const dialog = await displayed('[role="dialog"]');
      await clickTemplate("Dividend income");
      await assertFormLegendsFit(browser, { selector: "[data-loupist-tone]" });
      await clickButton("Create Loupist", dialog);
      await browser.waitUntil(async () => !(await browser.$('[role="dialog"]').isDisplayed().catch(() => false)), {
        timeout: 20_000,
        timeoutMsg: "New Loupist dialog did not close",
      });
      titles = (await visibleLoupistTitles()).filter(Boolean);
    }
    const selected = await selectedLoupistTitle(browser);
    const other = titles.find((title) => title && title !== selected);
    assert.ok(selected && other, `Two Loupists are required to time a switch: ${titles.join(", ")}`);
    const first = await measureLoupistSwitch(browser, other);
    assert.ok(!first.error, first.error ?? "");
    const back = await measureLoupistSwitch(browser, selected);
    assert.ok(!back.error, back.error ?? "");
    const ms = Math.max(first.ms, back.ms);
    performanceReport.loupistSwitch = {
      samples: [first, back],
      ms,
      budgetMsLimit: 300,
      pass: ms <= 300,
    };
    console.log(JSON.stringify(performanceReport.loupistSwitch));
  });

  await step("Settings Connections", async () => {
    await openSettings("connections");
    const panel = await displayed("#main-panel-settings");
    await waitForText(panel, "Connections");
    await waitForText(panel, "Your connections");
    await displayed("[data-custom-add]");
    await displayed('input[aria-label="Search connections"]');
    await waitForText(panel, "Market data platform");
    const unsupported = await browser.execute(() =>
      [...document.querySelectorAll('#main-panel-settings [role="tab"], #main-panel-settings [role="radio"]')]
        .map((element) => element.textContent?.trim() ?? "")
        .filter((text) => /Community|Libraries/.test(text)),
    );
    assert.deepEqual(unsupported, []);
    await capture("connections");
    await clickButton("Back");
    await waitForShell(browser);
  });

  await step("Map tab", async () => {
    await openMainTab(browser, "map");
    const panel = await displayed(".map-view");
    let count = await mapNameCount(browser);
    let surface = "";
    await browser.waitUntil(async () => {
      count = await mapNameCount(browser);
      surface = await browser.execute(() => {
        const canvas = document.querySelector(".map-view .map-canvas canvas");
        const box = canvas?.getBoundingClientRect();
        if (canvas && box && box.width > 1 && box.height > 1) return "canvas";
        const empty = document.querySelector(".map-view .map-empty");
        if (empty && empty.getClientRects().length > 0) return "empty";
        return "";
      });
      return surface === "canvas" || surface === "empty";
    }, {
      timeout: 20_000,
      timeoutMsg: `Map did not render a canvas or an empty state (${count?.text ?? "no status"})`,
    });
    if (surface === "canvas") {
      assert.ok(count?.text, "Map status did not render beside the canvas");
      performanceReport.mapTab = { surface: "canvas", stats: count.text };
    } else {
      assert.equal(count, null, "An empty Map should show guidance without a zero-count status");
      await waitForText(panel, "Your Loupist's map grows as you research");
      await waitForText(panel, "Ask in Chat");
      performanceReport.mapTab = {
        surface: "empty",
        stats: null,
        note: "A Loupist with no World nodes renders the empty state. The canvas measurement is the 1,000-node probe.",
      };
    }
    await capture("map");
  });

  if (full) await step("Connections filtering and Web Research", async () => {
    await openSettings("connections");
    const panel = await displayed("#main-panel-settings");
    await displayed('[aria-label="Details Market data platform"]');
    const openbbCard = await browser.execute(() => {
      const heading = [...document.querySelectorAll(".marketplace-card h3")].find(
        (element) => element.textContent === "Market data platform",
      );
      return heading?.closest(".marketplace-card")?.innerText ?? "";
    });
    if (process.env.LOUPIST_E2E_REQUIRE_STAGED_RUNTIMES === "1") {
      assert.match(openbbCard, /Ready/);
    } else {
      assert.match(openbbCard, /Connection unavailable|Ready/);
    }
    assert.doesNotMatch(openbbCard, /Needs setup/);
    const search = await displayed('input[aria-label="Search connections"]');
    await search.setValue("Korean");
    await waitForText(panel, "OpenDART");
    await waitForTextGone(panel, "SEC EDGAR");
    await search.setValue("");
    await waitForText(panel, "SEC EDGAR");
    await (await displayed('[aria-label="Settings Web Research"]')).click();
    let dialog = await displayed('[role="dialog"]');
    await selectOption('[aria-label="Search provider"]', "Exa only");
    await clickButton("Save setup", dialog);
    await waitForText(dialog, "Settings saved");
    await clickButton("Close", dialog);
    await (await displayed('[aria-label="Settings Web Research"]')).click();
    dialog = await displayed('[role="dialog"]');
    assert.equal(await (await displayed('[aria-label="Search provider"]')).getText(), "Exa only");
    await selectOption('[aria-label="Search provider"]', "Automatic");
    await clickButton("Save setup", dialog);
    await waitForText(dialog, "Settings saved");
    await clickButton("Close", dialog);
    await capture("connections-full");
  });

  if (full) await step("Model, appearance, and update Settings", async () => {
    await openSettings("model");
    const panel = await displayed("#main-panel-settings");
    const connect = await browser.$$("button");
    let opened = false;
    for (const button of connect) {
      if ((await button.isDisplayed()) && (await button.getText()).trim() === "Connect provider") {
        await button.click();
        opened = true;
        break;
      }
    }
    if (opened) {
      await displayed('button[aria-label="Choose OpenAI with ChatGPT"]');
      await browser.keys("Escape");
    }
    await openSettings("appearance");
    const dark = await displayed('button[title="Dark theme"]');
    await dark.click();
    await browser.waitUntil(async () => (await dark.getAttribute("aria-pressed")) === "true", {
      timeout: 5_000,
      timeoutMsg: "Dark theme was not saved",
    });
    assert.equal(await (await displayed(".app-shell")).getAttribute("data-theme"), "dark");
    const light = await displayed('button[title="Light theme"]');
    await light.click();
    await browser.waitUntil(async () => (await light.getAttribute("aria-pressed")) === "true", {
      timeout: 5_000,
      timeoutMsg: "Light theme was not saved",
    });
    await (await displayed('button[title="System theme"]')).click();
    await openSettings("updates");
    await clickButton("Check for updates", panel);
    await waitForText(panel, "Automatic updates are unavailable");
    await clickButton("Back");
  });

  if (full) await step("renderer cannot directly author Lens or World records", async () => {
    // knowledge_lens_archive stays: archiving a method from the Map is a user action, not authoring.
    const commands = [
      "knowledge_lens_create",
      "knowledge_lens_save",
      "knowledge_lens_rename",
      "knowledge_lens_draft_put",
      "knowledge_lens_drafts",
      "knowledge_lens_draft_delete",
      "knowledge_graph_curate",
    ];
    const results = await browser.execute(async (names) => Promise.all(names.map(async (command) => {
      try {
        await window.__TAURI_INTERNALS__.invoke(command, { request: {} });
        return { command, error: "Unexpectedly accepted" };
      } catch (error) {
        return { command, error: String(error) };
      }
    })), commands);
    for (const result of results) {
      assert.match(result.error, /not found/i, `${result.command} remains callable: ${result.error}`);
    }
  });

  if (full) await step("minimum window and accessible controls", () => withWindowSize(browser, minimumWindowSize, async () => {
    await openMainTab(browser, "chat");
    const audit = await browser.execute(() => {
      const visible = (element) => {
        const style = window.getComputedStyle(element);
        return style.display !== "none" && style.visibility !== "hidden" && element.getClientRects().length > 0;
      };
      const textFromIds = (value) =>
        (value ?? "").split(/\s+/).filter(Boolean)
          .map((id) => document.getElementById(id)?.textContent?.trim() ?? "")
          .filter(Boolean).join(" ");
      const controls = [...document.querySelectorAll('button, a[href], input, textarea, select, [role="button"], [role="tab"]')].filter(visible);
      const unnamed = controls.filter((element) => {
        const labels = element.labels ? [...element.labels].map((label) => label.textContent?.trim() ?? "").join(" ") : "";
        return !(
          element.getAttribute("aria-label")?.trim()
          || textFromIds(element.getAttribute("aria-labelledby"))
          || labels.trim()
          || element.textContent?.trim()
          || element.getAttribute("title")?.trim()
        );
      }).map((element) => element.outerHTML.slice(0, 240));
      return {
        viewportWidth: document.documentElement.clientWidth,
        documentWidth: document.documentElement.scrollWidth,
        unnamed,
        visibleH1Count: [...document.querySelectorAll("h1")].filter(visible).length,
      };
    });
    assert.ok(audit.documentWidth <= audit.viewportWidth + 1, JSON.stringify(audit));
    assert.deepEqual(audit.unnamed, []);
    await capture("minimum-window");
  }));

  await step("removing the last Loupist returns to onboarding", async () => {
    await clickButton("Back").catch(() => {});
    await waitForShell(browser);
    await removeUntilOnboarding();
    await waitForText(await displayed("main[data-onboarding]"), "Get started");
    await capture("onboarding-after-remove");
  });

  await step("1,000-node map probe", async () => {
    assert.ok(session.devUrl, "The launcher did not record devUrl, so the map probe cannot stay on this dev server");
    const unfocused = process.env.LOUPIST_E2E_ALLOW_UNFOCUSED === "1";
    const probe = unfocused
      ? await measureMapDrawProbe(browser, session.devUrl)
      : await measureMapProbe(browser, session.devUrl);
    assert.equal(probe.canvas, true, "The 1,000-node preview did not render a canvas");
    performanceReport.mapProbe = {
      ...probe,
      budgetP95Ms: 33,
      pass: probe.p95Ms <= 33,
      proxy: unfocused,
    };
    console.log(JSON.stringify(performanceReport.mapProbe));
    await capture("map-probe");
  });

  if (process.env.LOUPIST_E2E_ALLOW_UNFOCUSED !== "1") {
    await step("150-name two-hop map probe", async () => {
      const probe = await measureMapProbe(browser, session.devUrl, { scenario: "map-local-2hops-bounded" });
      assert.equal(probe.canvas, true, "The two-hop preview did not render a canvas");
      assert.ok(probe.nodes > 1 && probe.nodes <= 150, `The two-hop view drew ${probe.nodes} names; it stops at 150`);
      performanceReport.mapTwoHopProbe = { ...probe, budgetP95Ms: 33, pass: probe.p95Ms <= 33 };
      console.log(JSON.stringify(performanceReport.mapTwoHopProbe));
      await capture("map-two-hop-probe");
    });
  }

  // WebKit drops some text-decoration shorthands that Chromium accepts, which once left number
  // chips with no visible underline in the app while every jsdom and Chrome check passed.
  await step("number chips are underlined in the native WebView", async () => {
    await browser.url(`${String(session.devUrl).replace(/\/$/, "")}/preview.html?scenario=chat-number-chips`);
    let decoration = null;
    await browser.waitUntil(async () => {
      decoration = await browser.execute(() => {
        const chip = document.querySelector("article.message.assistant button.number-chip");
        if (!chip) return null;
        const style = getComputedStyle(chip);
        return { line: style.textDecorationLine, style: style.textDecorationStyle };
      });
      return decoration !== null;
    }, { timeout: 60_000, interval: 250, timeoutMsg: "The number-chip preview did not render a chip" });
    assert.match(decoration.line, /underline/, `Number chips have no underline in WebKit: ${JSON.stringify(decoration)}`);
    assert.equal(decoration.style, "dotted", `Number chips lost their dotted underline: ${JSON.stringify(decoration)}`);
    await capture("number-chip-underline");
  });

  await step("native browser preserves the article while blocking unsafe navigation", async () => {
    performanceReport.browserNavigation = await verifyNativeBrowserNavigation(browser);
    console.log(JSON.stringify(performanceReport.browserNavigation));
  });

  performanceReport.budgets = {
    coldStartMs: performanceReport.coldStart,
    loupistSwitchMs: performanceReport.loupistSwitch,
    chatFirstTokenMs: { recorded: false, note: "Provider-bound. The live journey records it." },
    mapP95Ms: performanceReport.mapProbe,
    mapTwoHopP95Ms: performanceReport.mapTwoHopProbe ?? null,
  };
  await writeFile(
    resolve(artifactRoot, "native-smoke-performance.json"),
    `${JSON.stringify(performanceReport, null, 2)}\n`,
  );
  await browser.saveScreenshot(resolve(artifactRoot, "native-release-smoke.png"));
  console.log(full ? "Loupist native release smoke passed." : "Loupist native core smoke passed.");
} catch (error) {
  const state = await browser.execute(() => ({
    onboarding: document.querySelector("main[data-onboarding]")?.getAttribute("data-onboarding") ?? null,
    current: [...document.querySelectorAll('[aria-current="page"]')].map((element) => ({
      id: element.id,
      text: element.textContent?.trim(),
    })),
    panels: [...document.querySelectorAll('[id^="main-panel-"]')].map((element) => element.id),
    body: document.body.innerText.slice(0, 8_000),
  })).catch(() => ({ body: "" }));
  console.error("Native failure state:", scrub(JSON.stringify(state)));
  await browser.saveScreenshot(resolve(artifactRoot, "native-release-smoke-failure.png")).catch(() => {});
  throw error;
} finally {
  await browser.deleteSession();
}
