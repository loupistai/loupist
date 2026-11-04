// Opt-in native release journeys. The launcher supplies the live Pi directory.
// This file never reads or prints credential files.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { remote } from "webdriverio";
import { createWebdriverHelpers } from "./webdriver-helpers.mjs";
import { focusNativeWindow } from "./native-window-focus.mjs";
import { assertPresetSelection, livePreset } from "./live-preset.mjs";
import { thinkingLabel } from "../src/thinking-labels.mjs";
import {
  coldStartMark,
  dialogByTitle,
  mapNameCount,
  measureLoupistSwitch,
  clickControl,
  measureMapDrawProbe,
  measureMapProbe,
  onboardingStep,
  pageTiming,
  preferEnglish,
  scrub,
  selectedLoupistTitle,
  waitForOnboardingStep,
  openMainTab as openDestination,
  waitForShell,
} from "./native-shell.mjs";

const e2eRoot = dirname(fileURLToPath(import.meta.url));
const sessionPath = process.argv[2];
assert.ok(sessionPath, "usage: node native-live-journeys.mjs <current-session.json>");
const session = JSON.parse(await readFile(sessionPath, "utf8"));
const runId = new Date().toISOString().replace(/[:.]/g, "-");
const artifactDir = resolve(e2eRoot, "artifacts", "journeys", runId);
await mkdir(artifactDir, { recursive: true });
const sourcePath = resolve(e2eRoot, "fixtures/journeys/northwind-loom.md");
const sourceText = await readFile(sourcePath, "utf8");
const learnMcpUrl = "https://learn.microsoft.com/api/mcp";
const loupistName = "Portfolio Loupist";
const preset = livePreset();

const browser = await remote({
  ...session.webdriverConfig,
  capabilities: session.capabilities,
  connectionRetryTimeout: 15_000,
  connectionRetryCount: 0,
  logLevel: "error",
});
const { displayed, childWithText, clickButton, waitForText } = createWebdriverHelpers(browser, {
  defaultTimeout: 20_000,
  bodyTextLimit: 12_000,
  interval: 150,
});

// 3 needs the onboarding Loupist and the chat-attached note. 6 needs the Loupist.
const selectedJourneys = new Set(
  String(process.env.LOUPIST_E2E_JOURNEYS ?? "")
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean),
);
const localProviderJourney = selectedJourneys.size === 1 && selectedJourneys.has("7");
function journeyEnabled(id) {
  // Journey 7 needs an empty Pi directory, which the launcher creates only when 7 is selected alone.
  if (selectedJourneys.size === 0) return id !== "7";
  if (selectedJourneys.has(id)) return true;
  if (id === "1" && (selectedJourneys.has("3") || selectedJourneys.has("6"))) return true;
  if (id === "2" && selectedJourneys.has("3")) return true;
  return false;
}

const report = {
  build: "isolated-debug-e2e",
  signedRelease: false,
  selectedJourneys: selectedJourneys.size === 0 ? "all" : [...selectedJourneys],
  mode: process.env.LOUPIST_E2E_ALLOW_UNFOCUSED === "1" ? "webview-only" : "focused-native",
  credential: localProviderJourney ? "local-stand-in" : "pre-provisioned",
  holdingsImport: localProviderJourney ? null : "LOUPIST_E2E_PICK_FILE",
  modelId: localProviderJourney ? "local-test-model" : "openai-codex/gpt-6.1-sol",
  modelLabel: localProviderJourney ? "local-test-model" : preset.modelLabel,
  effort: localProviderJourney ? null : preset.effort,
  journeys: {},
  performance: {},
  blockingError: null,
};

const shot = (name) => browser.saveScreenshot(resolve(artifactDir, `${name}.png`));
async function step(name, action) {
  process.stdout.write(`• ${name} ... `);
  const before = report.blockingError ?? null;
  const result = await action();
  console.log(report.blockingError && report.blockingError !== before ? "recorded" : "passed");
  return result;
}
const bodySnippet = async () => scrub(await browser.execute(() => document.body.innerText.slice(0, 6_000)));

async function clickTemplate(name) {
  // The step transition keeps the previous step on screen briefly; wait for the card.
  let match = null;
  await browser.waitUntil(async () => {
    for (const card of await browser.$$("button.loupist-template-card")) {
      try {
        if ((await card.isDisplayed()) && (await card.getText()).trim().startsWith(name)) {
          match = card;
          return true;
        }
      } catch {
        // A card replaced mid-read is checked again on the next round.
      }
    }
    return false;
  }, { timeout: 10_000, interval: 100, timeoutMsg: `Loupist template was not visible: ${name}` });
  await match.click();
}

async function removeEveryLoupist() {
  for (let guard = 0; guard < 8; guard += 1) {
    if (await onboardingStep(browser)) return;
    // The selected Loupist has a visible Settings action in the sidebar.
    const identity = await browser.$(".loupist-header-identity");
    if (!(await identity.isExisting()) || !(await identity.isDisplayed())) return;
    const opener = await displayed("[data-header-loupist-settings]");
    await opener.click();
    const sheet = await displayed(".loupist-settings-page");
    await clickButton("Remove Loupist", sheet);
    const dialog = await dialogByTitle(browser, "Remove Loupist?");
    await clickButton("Remove Loupist", dialog);
    await browser.waitUntil(
      async () => Boolean(await onboardingStep(browser)) || !(await browser.$(".loupist-settings-page").isDisplayed().catch(() => false)),
      { timeout: 20_000, timeoutMsg: "Removing a Loupist did not close Loupist settings" },
    );
  }
  if (!(await onboardingStep(browser))) throw new Error("Loupists remained after repeated removal");
}

async function visibleMenuItem(exactText) {
  for (const menu of await browser.$$('[data-slot="dropdown-menu-content"]')) {
    if (!(await menu.isDisplayed().catch(() => false))) continue;
    for (const item of await menu.$$('[role="menuitemradio"]')) {
      if ((await item.getText()).trim() === exactText) return item;
    }
  }
  return null;
}

async function choosePreset() {
  await openDestination(browser, "chat");
  const composer = await displayed('textarea[aria-label="Message Loupist"]');
  const modelMenu = await displayed('[aria-label$="Model settings for this message"]');
  await browser.waitUntil(async () => modelMenu.isEnabled(), {
    timeout: 90_000,
    timeoutMsg: "The Chat model menu did not become available",
  });
  await composer.click();
  await modelMenu.click();
  const choose = async (label) => {
    const item = await browser.waitUntil(async () => visibleMenuItem(label), {
      timeout: 8_000,
      interval: 150,
      timeoutMsg: `Model menu item was not visible: ${label}`,
    }).catch(() => null);
    if (item && (await item.isDisplayed().catch(() => false))) {
      await item.click();
      return;
    }
    await browser.keys(label);
    await browser.keys("Enter");
  };
  await choose(preset.modelLabel);
  await browser.pause(300);
  if (!(await visibleMenuItem(thinkingLabel(preset.effort))) && !(await visibleMenuItem(preset.effort))) {
    await modelMenu.click();
  }
  try {
    await choose(thinkingLabel(preset.effort));
  } catch {
    await choose(preset.effort);
  }
  await browser.keys("Escape");
  await composer.click();
  assertPresetSelection(await modelMenu.getAttribute("aria-description"), "Journey model menu");
}

async function openMainTab(tabId) {
  await openDestination(browser, tabId.replace("#main-tab-", ""), 90_000);
}

async function newChat() {
  await openDestination(browser, "chat");
  await clickControl(browser, `.loupist-list-heading button`);
  await displayed('textarea[aria-label="Message Loupist"]');
}

async function sendPrompt(text, attachment) {
  await openDestination(browser, "chat");
  const previousAnswerIds = new Set();
  for (const article of await browser.$$("article.message.assistant")) {
    previousAnswerIds.add(await article.getAttribute("data-message-id"));
  }
  const newAnswers = async () => {
    const result = [];
    for (const article of await browser.$$("article.message.assistant")) {
      if (!previousAnswerIds.has(await article.getAttribute("data-message-id"))) result.push(article);
    }
    return result;
  };
  if (attachment) {
    // WKWebView rejects WebDriver setValue on input[type=file] with InvalidStateError.
    // Dispatch the same change event the composer already handles, using the fixture bytes.
    const attached = await browser.execute((fileName, contents) => {
      const input = document.querySelector('#main-panel-chat input[aria-label="Upload files"]');
      if (!(input instanceof HTMLInputElement)) return "missing-input";
      const file = new File([contents], fileName, { type: "text/markdown" });
      const transfer = new DataTransfer();
      transfer.items.add(file);
      if (!transfer.files.length) return "transfer-empty";
      // The file input sits beside the form, and the change handler clears it.
      input.files = transfer.files;
      if (input.files.length > 0) {
        input.dispatchEvent(new Event("change", { bubbles: true }));
        return "input";
      }
      const form = input.nextElementSibling instanceof HTMLFormElement
        ? input.nextElementSibling
        : input.parentElement?.querySelector("form");
      if (!(form instanceof HTMLFormElement)) return "assigned-0-no-form";
      form.dispatchEvent(new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: transfer }));
      return "drop";
    }, "northwind-loom.md", sourceText);
    if (attached !== "input" && attached !== "drop") {
      throw new Error(`The fixture note did not attach (${attached})`);
    }
    await browser.waitUntil(
      async () => (await (await displayed("#main-panel-chat")).getText()).includes("northwind-loom.md"),
      { timeout: 15_000, timeoutMsg: "The fixture note did not attach" },
    );
  }
  const composer = await displayed('textarea[aria-label="Message Loupist"]');
  await composer.setValue(text);
  const started = Date.now();
  await (await displayed('button[aria-label="Send"]')).click();
  let firstTokenMs = null;
  await browser.waitUntil(async () => {
    for (const article of await newAnswers()) {
      if (!(await article.isDisplayed().catch(() => false))) continue;
      const textContent = (await article.getText()).replace(/\s+/g, " ").trim();
      if (!textContent || /^Starting response…?$/u.test(textContent)) continue;
      if (firstTokenMs == null) firstTokenMs = Date.now() - started;
      return true;
    }
    return false;
  }, { timeout: localProviderJourney ? 60_000 : 180_000,
    interval: 200, timeoutMsg: "No streamed answer text appeared" });
  await browser.waitUntil(async () => {
    for (const button of await browser.$$('button[aria-label="Stop response"]')) {
      if (await button.isDisplayed().catch(() => false)) return false;
    }
    for (const article of await browser.$$("article.message.assistant.streaming")) {
      if (await article.isDisplayed().catch(() => false)) return false;
    }
    return (await newAnswers()).length > 0;
  }, { timeout: 480_000, interval: 250, timeoutMsg: "The answer did not finish" });
  return { firstTokenMs, totalMs: Date.now() - started };
}

async function answerText() {
  const articles = await browser.$$("article.message.assistant");
  const last = articles.at(-1);
  if (!last) return "";
  return (await last.getText()).replace(/\s+/g, " ").trim();
}


try {
  await focusNativeWindow(browser, session);
  await step(localProviderJourney ? "cold start to onboarding" : "cold start to the shell", async () => {
    // With journey 7's empty Pi directory nothing is connected yet, so the app opens on onboarding.
    if (localProviderJourney) {
      await browser.waitUntil(async () => Boolean(await onboardingStep(browser)), { timeout: 90_000, timeoutMsg: "Onboarding did not open" });
    } else await waitForShell(browser, 90_000);
    const timing = coldStartMark(await pageTiming(browser));
    report.performance.coldStart = {
      ...timing,
      surface: localProviderJourney ? "onboarding" : "shell",
      spawnToObservedMs: Number.isInteger(session.spawnedAtMs) ? Date.now() - session.spawnedAtMs : null,
      budgetMsLimit: 3_000,
      pass: (localProviderJourney ? timing.interactivePresent : timing.shell) && timing.budgetMs <= 3_000,
      note: "budgetMs is the in-page paint mark. spawnToObservedMs includes process launch and is not the budget.",
    };
    console.log(JSON.stringify(report.performance.coldStart));
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

  if (journeyEnabled("1")) await step("onboarding with a pre-provisioned account", async () => {
    await preferEnglish(browser);
    await removeEveryLoupist();
    await waitForOnboardingStep(browser, 1, 30_000);
    await shot("01-onboarding-welcome");
    if (!(await (await displayed("[data-notice-acknowledgment]")).isSelected())) await (await displayed("[data-notice-acknowledgment]")).click();
    await clickButton("Get started");
    await waitForShell(browser, 30_000);
    assert.equal(await selectedLoupistTitle(browser), loupistName);
    report.journeys.onboarding = { credential: "pre-provisioned", oauthAutomated: false };
    await shot("01-onboarding-shell");
  });

  if (journeyEnabled("switch")) await step("Loupist switch", async () => {
    await (await displayed('button[aria-label="New Loupist"]')).click();
    const dialog = await displayed('[role="dialog"]');
    await clickTemplate("Dividend income");
    await clickButton("Create Loupist", dialog);
    await browser.waitUntil(async () => !(await browser.$('[role="dialog"]').isDisplayed().catch(() => false)), {
      timeout: 20_000,
      timeoutMsg: "New Loupist dialog did not close",
    });
    const selected = await selectedLoupistTitle(browser);
    assert.ok(selected, "A Loupist was not selected after Create Loupist");
    const target = selected === loupistName ? "Dividend Loupist" : loupistName;
    const toOther = await measureLoupistSwitch(browser, target);
    assert.ok(!toOther.error, toOther.error ?? "");
    const back = await measureLoupistSwitch(browser, selected);
    assert.ok(!back.error, back.error ?? "");
    const ms = Math.max(toOther.ms, back.ms);
    report.performance.loupistSwitch = { samples: [toOther, back], ms, budgetMsLimit: 300, pass: ms <= 300 };
    console.log(JSON.stringify(report.performance.loupistSwitch));
  });

  if (journeyEnabled("2")) await step("useful answer", async () => {
    await choosePreset();
    const timing = await sendPrompt(
      "Using only the attached note, what loom-yield token did Northwind Loom record on 12 March 2026? Cite the note. This is a fixture question, not a request for a recommendation.",
      sourcePath,
    );
    // The Markdown renderer can still be replacing its plain fallback when the run ends. A figure
    // linked to its source renders as a number chip, which is that source's citation too.
    let visibleCitations = 0;
    await browser.waitUntil(async () => {
      visibleCitations = 0;
      for (const citation of await browser.$$("#main-panel-chat button.loupist-citation, #main-panel-chat button.number-chip")) {
        if (await citation.isDisplayed().catch(() => false)) visibleCitations += 1;
      }
      return visibleCitations > 0;
    }, { timeout: 15_000, interval: 250 }).catch(() => undefined);
    const citationDom = await browser.execute(() => {
      const panel = document.querySelector("#main-panel-chat");
      const count = (selector) => panel?.querySelectorAll(selector).length ?? 0;
      const chip = panel?.querySelector("button.number-chip");
      const style = chip ? getComputedStyle(chip) : null;
      return {
        citationButtons: count("button.loupist-citation"),
        numberChips: count("button.number-chip"),
        unavailable: count(".loupist-unavailable-citation"),
        fallback: count(".message-response-fallback"),
        numberChipDecoration: style ? `${style.textDecorationLine} ${style.textDecorationStyle} ${style.textDecorationColor}` : null,
      };
    });
    const answer = await answerText();
    await waitForText(await displayed("#main-panel-chat"), "AI can be wrong. Not a licensed financial adviser.");
    report.performance.chatFirstToken = {
      firstTokenMs: timing.firstTokenMs,
      totalMs: timing.totalMs,
      budget: "provider-bound, recorded only",
    };
    report.journeys.usefulAnswer = {
      ...report.performance.chatFirstToken,
      citations: visibleCitations,
      citationDom,
      mentionsToken: answer.includes("18420"),
    };
    await shot("02-useful-answer");
    assert.ok(visibleCitations >= 1, `Expected a citation (${JSON.stringify(citationDom)}) in: ${scrub(answer).slice(0, 1_200)}`);
    assert.match(answer, /18420/, `The answer did not use the fixture token: ${scrub(answer).slice(0, 1_200)}`);
    console.log(JSON.stringify(report.performance.chatFirstToken));
  });

  if (journeyEnabled("3")) await step("map grows", async () => {
    await openMainTab("#main-tab-map");
    const before = await mapNameCount(browser);
    const beforeNames = before?.names ?? 0;
    const addVisible = await browser.execute(() =>
      [...document.querySelectorAll(".map-view button")].some((button) =>
        button.textContent?.trim().replace(/^\+\s*/u, "") === "Add source" && button.getClientRects().length > 0,
      ),
    );
    if (!addVisible) {
      await clickControl(browser, ".map-view [data-map-more]");
      await (await displayed('[role="menuitemcheckbox"][data-map-library]')).click();
    }
    await clickButton("Add source");
    const dialog = await displayed('[role="dialog"]');
    await (await dialog.$('[aria-label="Source title"]')).setValue("Northwind Loom note");
    await (await dialog.$('[aria-label="Source text"]')).setValue(sourceText);
    await shot("03-map-add-source");
    await clickButton("Save source", dialog);
    await browser.waitUntil(async () => !(await browser.$('[role="dialog"]').isDisplayed().catch(() => false)), {
      timeout: 30_000,
      timeoutMsg: "Save source did not close",
    });
    let after = before;
    await browser.waitUntil(async () => {
      after = await mapNameCount(browser);
      return (after?.names ?? 0) > beforeNames;
    }, {
      timeout: 600_000,
      interval: 2_000,
      timeoutMsg: `Map node count did not increase from ${beforeNames}. Last status: ${after?.text ?? "missing"}`,
    });
    await openDestination(browser, "chat");
    let chip = "";
    let chipError = null;
    try {
      await browser.waitUntil(async () => {
        chip = await browser.execute(() =>
          [...document.querySelectorAll('.chat-receipt [data-receipt="names"]')].map((element) => element.textContent?.trim() ?? "").find(Boolean) ?? "",
        );
        return Boolean(chip);
      }, { timeout: 180_000, interval: 2_000, timeoutMsg: `Map grew to ${after?.text ?? "unknown"} but the answer receipt shows no “+n on the map” segment` });
      assert.match(chip, /\+[\d,]+\s+on the map/);
    } catch (error) {
      chipError = error instanceof Error ? error.message : String(error);
    }
    report.journeys.map = {
      before: before?.text ?? null,
      after: after?.text ?? null,
      chip: chip || null,
      chipError,
      classification: chipError
        ? "product bug: map growth events are sealed without the chat origin, so the receipt cannot show the new names"
        : "passed",
    };
    await shot("03-map-grew");
    if (chipError) report.blockingError = chipError;
  });

  if (journeyEnabled("4")) await step("principle is saved and applied", async () => {
    await choosePreset();
    await newChat();
    await sendPrompt("Remember this rule: Separate the Northwind Loom note from any market price.");
    await browser.waitUntil(async () => {
      const row = await browser.$('[data-memory-change="create:principle"]');
      // The receipt says "Saved 1 principle" and the list below it shows the principle as its own card.
      return (await row.isExisting()) && (await row.getText()).includes("Separate the Northwind Loom note");
    }, { timeout: 30_000, timeoutMsg: `The answer did not show the saved principle: ${scrub(await answerText()).slice(0, 1_000)}` });
    await shot("04-principle-saved");
    await newChat();
    await choosePreset();
    await sendPrompt("How should I treat the Northwind Loom note if someone compares it with a market price?");
    const answer = await answerText();
    // MessageResponse renders a lens: markdown link as button.loupist-lens-link, not an <a href>.
    const lensControl = await browser.execute(() => {
      const button = document.querySelector("#main-panel-chat button.loupist-lens-link");
      return button
        ? { text: (button.textContent ?? "").trim().slice(0, 120), role: button.getAttribute("role") ?? "" }
        : null;
    });
    const section = /Checked against your principles/i.test(answer);
    report.journeys.principle = { applied: section, lensControl };
    await shot("04-principle-applied");
    if (!section || !lensControl) {
      const message = section
        ? "The principles section rendered, but no Lens link button was in the answer"
        : `The answer did not include Checked against your principles: ${scrub(answer).slice(0, 500)}`;
      report.journeys.principle.error = message;
      report.blockingError = [report.blockingError, message].filter(Boolean).join("\n");
    }
  });

  if (journeyEnabled("5")) await step("briefing", async () => {
    await openDestination(browser, "chat");
    await clickControl(browser, ".loupist-chat-row[data-home] .loupist-chat-button");
    await sendPrompt("Schedule a weekday morning briefing for 08:00 Asia/Seoul, covering Northwind Loom. Activate it now.");
    await openMainTab("#main-tab-schedule");
    const panel = await displayed("#main-panel-schedule");
    // The briefing is the first scheduled task below the pinned Watching row; its row carries Run now.
    const briefingTask = "#main-panel-schedule .schedule-tasks:not(.schedule-pinned) .schedule-task";
    await clickButton("Run now", await displayed(briefingTask));
    // Runs are listed under their task.
    await (await displayed(`${briefingTask} .schedule-task-summary`)).click();
    // The run list re-renders as the run progresses; read it from the DOM, not a held element.
    const firstRun = () => browser.execute(() => {
      const item = document.querySelector("#main-panel-schedule .schedule-history .briefings-item");
      return item && item.getClientRects().length ? { text: item.textContent ?? "", unread: item.getAttribute("data-unread") } : null;
    });
    await browser.waitUntil(async () => {
      const run = await firstRun();
      return Boolean(run) && !/Preparing/i.test(run.text);
    }, { timeout: 720_000, interval: 2_000, timeoutMsg: `Briefing did not finish: ${await bodySnippet()}` });
    assert.equal((await firstRun())?.unread, "true", "A finished briefing should arrive unread");
    await shot("05-briefing-unread");
    await (await displayed("#main-panel-schedule .schedule-history .briefings-item")).click();
    const readerElement = await displayed(".briefings-reader");
    const reader = scrub(await readerElement.getText());
    assert.ok(reader.trim().length > 0, "Opening the briefing did not render a reader");
    // A failed run also renders a reader; only a finished briefing passes.
    if (await (await readerElement.$(".briefings-failure")).isExisting()) {
      const details = await browser.execute(() => document.querySelector(".briefings-failure-details pre")?.textContent ?? "");
      throw new Error(`The briefing did not finish: ${scrub(details).replace(/[\w.+-]+@[\w-]+\.[\w.]+/g, "[email]").slice(0, 1_500)}`);
    }
    const unfocused = process.env.LOUPIST_E2E_ALLOW_UNFOCUSED === "1";
    let read = false;
    if (!unfocused) {
      await browser.waitUntil(async () => (await firstRun())?.unread !== "true", {
        timeout: 8_000,
        timeoutMsg: "Opening the briefing did not mark it read",
      });
      read = true;
    } else {
      await browser.pause(1_500);
      read = (await firstRun())?.unread !== "true";
    }
    report.journeys.briefing = {
      opened: true,
      readerRendered: true,
      read,
      readOnOpen: read ? "observed" : "deferred: requires an unlocked screen",
    };
    await shot("05-briefing-read");
  });

  if (journeyEnabled("6")) await step("custom Microsoft Learn connection", async () => {
    const note = (message) => {
      report.journeys.mcp = { ...(report.journeys.mcp ?? {}), url: learnMcpUrl, error: message };
      report.blockingError = [report.blockingError, message].filter(Boolean).join("\n");
    };
    try {
      await openMainTab("#main-tab-settings");
      await (await displayed("#settings-section-connections")).click();
      await (await displayed("[data-custom-add]")).click();
      // The optional name Input omits the type attribute, so input[type=text] does not match it.
      const dialog = await displayed(".custom-add");
      await (await dialog.$('input[type="url"]')).setValue(learnMcpUrl);
      let name = await dialog.$("input:not([type])");
      if (!(await name.isExisting())) name = await dialog.$('input[type="text"]');
      await name.waitForExist({ timeout: 10_000 });
      await name.setValue("Microsoft Learn");
      await shot("06-mcp-url");
      await clickButton("Connect", dialog);
      await browser.waitUntil(async () => (await browser.$$("[data-custom-tool]")).length > 0, {
        timeout: 120_000,
        timeoutMsg: `Microsoft Learn tools were not listed: ${await bodySnippet()}`,
      });
      const toolNames = await browser.execute(() =>
        [...document.querySelectorAll("[data-custom-tool]")].map((row) => row.getAttribute("data-custom-tool") ?? ""),
      );
      const allowLabel = (text) => text === "Allow";
      const denyLabel = (text) => /^Don[’']t allow$/.test(text);
      let allowedTool = "";
      for (const toolName of toolNames) {
        const row = await browser.$(`[data-custom-tool="${toolName}"]`);
        const buttons = await row.$$("button");
        let choice = null;
        for (const button of buttons) {
          const text = (await button.getText()).trim();
          if (!allowedTool && allowLabel(text)) choice = button;
          if (allowedTool && denyLabel(text)) choice = button;
        }
        if (!choice) continue;
        if (!allowedTool) allowedTool = toolName;
        await choice.click();
        await browser.waitUntil(async () => (await choice.getAttribute("aria-pressed")) === "true", {
          timeout: 20_000,
          timeoutMsg: `Approval for ${toolName} did not stick`,
        });
      }
      assert.ok(allowedTool, "No read-only tool offered Allow");
      await shot("06-mcp-review");
      await browser.waitUntil(async () => {
        for (const button of await dialog.$$("button")) {
          if ((await button.getText()).trim() === "Done" && await button.isEnabled()) return true;
        }
        return false;
      }, { timeout: 30_000, timeoutMsg: "Done stayed disabled after tool review" });
      await clickButton("Done", dialog);
      await browser.waitUntil(async () => !(await browser.$(".custom-add").isDisplayed().catch(() => false)), {
        timeout: 20_000,
        timeoutMsg: "Done did not close the Add connection dialog",
      });
      // A new connection keeps no results until "Save results to Memory" is on.
      await (await displayed("button.custom-row")).click();
      await displayed(".custom-detail");
      const memory = await browser.$(".custom-detail [role='switch'][aria-label^='Save results']");
      await memory.waitForExist({ timeout: 10_000 });
      if ((await memory.getAttribute("aria-checked")) !== "true") await memory.click();
      await browser.waitUntil(async () => (await memory.getAttribute("aria-checked")) === "true", {
        timeout: 20_000,
        timeoutMsg: "Could not allow Microsoft Learn results to be kept",
      });
      await (await displayed(".custom-detail [data-slot='sheet-close']")).click();
      await browser.waitUntil(async () => !(await browser.$(".custom-detail").isDisplayed().catch(() => false)), {
        timeout: 20_000,
        timeoutMsg: "Connection detail did not close",
      });
      // Settings replaces the Chat/Map/Briefings header. Back is the same control the renderer tests use.
      if (!(await browser.$(".loupist-header-selector").isDisplayed().catch(() => false))) await clickButton("Back");
      await newChat();
      await choosePreset();
      await sendPrompt(`Use the Microsoft Learn connection's ${allowedTool} tool to find one documentation title. Save that tool result to Memory. Name the tool you called.`);
      // A read-only tool without an approval waits in the reply's approval card (composer D3); allow it once.
      const approval = await browser.$(".approval-card");
      if (await approval.waitForDisplayed({ timeout: 15_000 }).then(() => true).catch(() => false)) {
        await shot("06-approval-card");
        await clickButton("Allow once", approval);
        await browser.waitUntil(async () => !(await approval.isDisplayed().catch(() => false)), {
          timeout: 30_000,
          timeoutMsg: "The approval card did not settle after Allow once",
        });
      }
      const answer = await answerText();
      const progress = await browser.execute(() =>
        [...document.querySelectorAll(".chat-progress-action, .chat-progress-target")].map((element) => element.textContent?.trim() ?? "").join("\n"),
      );
      const sawTool = `${progress}\n${answer}`.includes(allowedTool) || progress.toLowerCase().includes("microsoft");
      await openMainTab("#main-tab-map");
      const library = await browser.$("[data-map-library]");
      if ((await library.isExisting()) && (await library.getAttribute("aria-expanded")) !== "true") await library.click();
      let libraryText = "";
      await browser.waitUntil(async () => {
        libraryText = await browser.execute(() => document.querySelector(".source-library")?.innerText ?? "");
        return libraryText.includes("From Microsoft Learn");
      }, {
        timeout: 90_000,
        interval: 2_000,
        timeoutMsg: "Saved sources did not show From Microsoft Learn",
      }).catch(() => {});
      const origin = libraryText.includes("From Microsoft Learn") || answer.includes("From Microsoft Learn");
      report.journeys.mcp = { url: learnMcpUrl, allowedTool, sawTool, origin };
      await openDestination(browser, "chat");
      await shot("06-mcp-answer");
      if (!sawTool) note(`The ${allowedTool} call was not visible: ${scrub(progress).slice(0, 400)} ${scrub(answer).slice(0, 400)}`);
      else if (!origin) note("The source origin “From Microsoft Learn” was not visible");
    } catch (error) {
      const message = scrub(error instanceof Error ? error.message : String(error));
      note(message);
      await shot("06-mcp-failed").catch(() => {});
    }
  });

  if (journeyEnabled("7")) await step("custom OpenAI-compatible provider from onboarding", async () => {
    // A local stand-in for Ollama or LM Studio. It records only the auth prefix and model it was sent.
    const seen = [];
    const server = createServer((request, response) => {
      let body = "";
      request.on("data", (chunk) => { body += chunk; });
      request.on("end", () => {
        let model = null, history = "";
        try {
          const payload = JSON.parse(body);
          model = payload.model ?? null;
          history = JSON.stringify(payload.messages ?? []);
        } catch {}
        const followUp = history.includes("What did you say earlier?");
        seen.push({
          path: request.url,
          bearer: (request.headers.authorization ?? "") === "Bearer e2e-custom-key",
          model,
          followUp,
          hasFirstQuestion: history.includes("Say hello."),
          hasFirstAnswer: history.includes("Hello from the custom provider."),
        });
        response.writeHead(200, { "content-type": "text/event-stream" });
        for (const chunk of [
          { choices: [{ index: 0, delta: { role: "assistant", content: followUp ? "Earlier I said: Hello from the custom provider." : "Hello from the custom provider." } }] },
          { choices: [{ index: 0, delta: {}, finish_reason: "stop" }], usage: { prompt_tokens: 5, completion_tokens: 6, total_tokens: 11 } },
        ]) response.write(`data: ${JSON.stringify({ id: "e2e", object: "chat.completion.chunk", created: 0, model: "local-test-model", ...chunk })}\n\n`);
        response.end("data: [DONE]\n\n");
      });
    });
    await new Promise((resolveListen) => server.listen(0, "127.0.0.1", resolveListen));
    const baseUrl = `http://127.0.0.1:${server.address().port}/v1`;
    try {
      await preferEnglish(browser);
      await waitForOnboardingStep(browser, 1, 30_000);
      if (!(await (await displayed("[data-notice-acknowledgment]")).isSelected())) await (await displayed("[data-notice-acknowledgment]")).click();
      await clickButton("Get started");
      await waitForOnboardingStep(browser, 2, 20_000);
      const connect = await displayed('main[data-onboarding="2"]');
      await (await childWithText(connect, "summary", "Other ways to connect")).click();
      await (await displayed('[data-connect-row="custom"]')).click();
      await dialogByTitle(browser, "Custom endpoint");
      const form = await displayed("[data-custom-sheet]");
      const fields = await form.$$("input[type=url], input:not([type]), input[type=text]");
      await fields[0].setValue("Local test server");
      await fields[1].setValue(baseUrl);
      await (await form.$("input[type=password]")).setValue("e2e-custom-key");
      // The stand-in only streams chat completions, so its models are typed by hand.
      await (await form.$("[data-type-ids]")).click();
      await (await form.$("textarea")).setValue("local-test-model");
      await shot("07-custom-form");
      await (await form.$("[data-custom-add]")).click();
      await waitForShell(browser, 30_000);
      const picker = await displayed('[aria-label$="Model settings for this message"]');
      // The composer shows "Auto"; the resolved model is the control's description.
      assert.match(await picker.getAttribute("aria-description") ?? "", /local-test-model/u, "Chat did not default to the custom provider's model");
      const firstTiming = await sendPrompt("Say hello.");
      const answer = await answerText();
      await shot("07-custom-answer");
      assert.match(answer, /Hello from the custom provider\./u, `The custom provider did not answer: ${scrub(answer).slice(0, 400)}`);
      const chat = seen.find((item) => item.path === "/v1/chat/completions");
      assert.ok(chat, `The custom provider received no chat request: ${JSON.stringify(seen)}`);
      assert.equal(chat.bearer, true, "The saved key was not sent to the custom provider");
      assert.equal(chat.model, "local-test-model");
      await sendPrompt("What did you say earlier? Answer in one sentence.");
      const followUpAnswer = await answerText();
      assert.match(followUpAnswer, /Earlier I said: Hello from the custom provider\./u);
      const followUp = seen.find((item) => item.path === "/v1/chat/completions" && item.followUp);
      assert.ok(followUp?.hasFirstQuestion && followUp.hasFirstAnswer, "The second request lost the first turn's context");
      assert.equal(followUp.bearer, true);
      await shot("07-custom-follow-up");
      await browser.refresh();
      await waitForShell(browser, 30_000);
      await waitForText(await displayed("#main-panel-chat"), "Earlier I said: Hello from the custom provider.");
      assert.match(await answerText(), /Earlier I said: Hello from the custom provider\./u, "The completed answer did not survive reload");
      await shot("07-custom-reloaded");
      // Removing it signs it out and drops its models.
      await openMainTab("#main-tab-settings");
      await (await displayed("#settings-section-model")).click();
      await clickControl(browser, 'button[aria-label="Remove Local test server"]');
      const confirm = await dialogByTitle(browser, "Remove Local test server?");
      await clickButton("Remove", confirm);
      await browser.waitUntil(async () => !(await browser.$('button[aria-label="Remove Local test server"]').isExisting()), {
        timeout: 30_000, timeoutMsg: "The custom provider was not removed",
      });
      await clickButton("Back");
      await waitForShell(browser, 30_000);
      await openMainTab("#main-tab-chat");
      assert.match(await answerText(), /Earlier I said: Hello from the custom provider\./u, "Removing the provider hid its saved Chat");
      await shot("07-custom-after-removal");
      report.journeys.customProvider = { answered: true, followUpRetainedContext: true, reloaded: true,
        answerKeptAfterRemoval: true, firstTokenMs: firstTiming.firstTokenMs,
        keySent: chat.bearer, model: chat.model, removed: true };
    } finally {
      await new Promise((resolveClose) => server.close(resolveClose));
    }
  });

  if (session.devUrl && journeyEnabled("probe")) {
    await step("1,000-node map probe", async () => {
      const unfocused = process.env.LOUPIST_E2E_ALLOW_UNFOCUSED === "1";
      const probe = unfocused
        ? await measureMapDrawProbe(browser, session.devUrl)
        : await measureMapProbe(browser, session.devUrl);
      report.performance.mapProbe = { ...probe, budgetP95Ms: 33, pass: probe.p95Ms <= 33, proxy: unfocused };
      await shot("07-map-probe");
      console.log(JSON.stringify(report.performance.mapProbe));
      assert.equal(probe.canvas, true, "The 1,000-node preview did not render a canvas");
    });
  }

  await writeFile(resolve(artifactDir, "timings.json"), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`Journey artifacts: ${artifactDir}`);
  if (report.blockingError) throw new Error(report.blockingError);
  console.log("Loupist native live journeys passed.");
} catch (error) {
  report.error = scrub(error instanceof Error ? error.stack ?? error.message : String(error));
  await writeFile(resolve(artifactDir, "timings.json"), `${JSON.stringify(report, null, 2)}\n`).catch(() => {});
  await shot("failure").catch(() => {});
  console.error(report.error);
  throw error;
} finally {
  await browser.deleteSession();
}
