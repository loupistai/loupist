import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { remote } from "webdriverio";
import { createPolicyWatch, createWebdriverHelpers } from "./webdriver-helpers.mjs";
import { describeViolations } from "./release-renderer.mjs";
import { minimumWindowSize, setWindowSize, withWindowSize } from "./window-geometry.mjs";
import { focusNativeWindow } from "./native-window-focus.mjs";
import { assertComposerGrows, assertComposerSelectAll, assertFormLegendsFit, assertNewLoupistPreviewShows, ComposerPreviewUnresponsive, composerMessageLayout, openComposerPreview } from "./native-forms.mjs";
import { verifyNativeBrowserNavigation } from "./native-browser.mjs";
import { verifyComposerAttachments } from "./native-attachments.mjs";
import { assertShellLayout, measureShellLayout } from "./shell-layout.mjs";
import { assertMapOverlays, measureMapOverlays } from "./map-layout.mjs";
import {
  coldStartMark,
  clickControl,
  dialogByTitle,
  mapNameCount,
  loupistSwitcherAction,
  measureLoupistSwitch,
  measureMapDrawProbe,
  measureMapProbe,
  onboardingStep,
  pageTiming,
  preferEnglish,
  scrub,
  selectedLoupistTitle,
  switcherLoupists,
  selectLoupistTemplate,
  waitForOnboardingStep,
  openMainTab,
  openCustomConnections,
  waitForNoRuns,
  waitForRenderedButton,
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

// Under the release renderer every step is checked against the shipped policy; the run lists all of them at the end.
const policy = createPolicyWatch(browser);
async function step(name, action) {
  process.stdout.write(`• ${name} ... `);
  try {
    await action();
  } finally {
    await policy.collect(name);
  }
  console.log("passed");
}
const reportPolicyViolations = () => {
  const violations = policy.violations();
  if (!violations.length) return;
  console.error(`Content-Security-Policy violations (${violations.length}):`);
  for (const [index, line] of describeViolations(violations).entries()) console.error(`  ${index + 1}. ${line}`);
  for (const entry of violations) console.error(`     step: ${entry.step}: ${entry.directive} ${entry.blocked}`);
};

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
  return (await switcherLoupists(browser)).map((item) => item.name);
}

async function removeUntilOnboarding() {
  for (let guard = 0; guard < 6; guard += 1) {
    if ((await onboardingStep(browser)) === "1") return;
    const [removed] = await visibleLoupistTitles();
    if (!removed) break;
    // The host refuses to remove a Loupist while one of its runs is active.
    await waitForNoRuns(browser);
    await loupistSwitcherAction(browser, "settings");
    const sheet = await displayed("[data-testid=loupist-settings-page]");
    await clickButton("Remove Loupist", sheet);
    const dialog = await dialogByTitle(browser, "Remove Loupist?");
    await clickButton("Remove Loupist", dialog);
    // A refused removal keeps the dialog open over the header with its reason; report that, not the header.
    await browser.waitUntil(async () => !(await dialog.isExisting().catch(() => false)) || !(await dialog.isDisplayed().catch(() => false)), {
      timeout: 30_000, interval: 150,
    }).catch(async (error) => {
      throw new Error(`Remove Loupist did not finish: ${await dialog.getText().catch(() => "")}`, { cause: error });
    });
    // Settings opened for a Loupist that is still in the header close when the
    // refreshed list drops it.
    await browser.waitUntil(async () => (await onboardingStep(browser)) === "1"
      || !(await visibleLoupistTitles()).includes(removed), {
      timeout: 15_000, timeoutMsg: `Removed Loupist stayed in the header: ${removed}`,
    });
  }
  await waitForOnboardingStep(browser, 1, 20_000);
}

// A hand-off Bot runs on its own (`LOUPIST_E2E_SLOW_HANDOFF`, macOS Apple Silicon debug builds;
// run-native-smoke.sh sets it there with the knowledge fixture, README "Deterministic native suites").
// The fixture's Pi child takes four steps two seconds apart, then stays open twelve seconds more.
const slowHandoff = process.env.LOUPIST_E2E_SLOW_HANDOFF === "1";
const BOT_ROW = "[data-testid=loupist-chat-row][data-home] [data-testid=loupist-chat-button]";
// Bot posts a hand-off only once its session ends; nothing while it works.
/** Bot's post for the hand-off titled `title`; the fixture also holds an older stopped hand-off. */
const handoffPost = (title) => `#main-panel-chat article[aria-label=${JSON.stringify(`Research: ${title}`)}]`;
const GUIDE = "Put the delivery dates first.";

const visibleText = (selector) => browser.execute((sel) => {
  const element = [...document.querySelectorAll(sel)].find((node) => node instanceof HTMLElement && node.offsetParent !== null);
  return element ? element.textContent?.trim() ?? "" : null;
}, selector);

async function waitForChange(selector, label, timeout) {
  const before = await visibleText(selector);
  await browser.waitUntil(async () => {
    const now = await visibleText(selector);
    return now !== null && now !== before;
  }, { timeout, interval: 200, timeoutMsg: `${label} did not change from “${before}”` });
}

async function openBot() {
  await openMainTab(browser, "chat");
  if (await browser.$(BOT_ROW).getAttribute("aria-current") !== "page") await clickControl(browser, BOT_ROW);
  await displayed(`${BOT_ROW}[aria-current="page"]`);
}

/** The sidebar row of the session titled `title`, once the host has listed it. */
async function sessionRow(title) {
  // Match the title alone: a running row also carries its "Running" status inside the button.
  const label = await childWithText(browser, "[data-testid=loupist-chat-button] [data-testid=loupist-chat-title]", title, 30_000);
  const thread = await browser.execute((element) => element.closest("[data-testid=loupist-chat-button]")?.getAttribute("data-thread-id") ?? "", label);
  return `[data-testid=loupist-chat-button][data-thread-id=${JSON.stringify(thread)}]`;
}

/** Opens the running hand-off's session from its sidebar row and waits for its live work. */
async function openRunningSession(row) {
  await clickControl(browser, row);
  await displayed(`${row}[aria-current="page"]`, 10_000);
  await displayed('#main-panel-chat [data-turn-latest] [data-testid="turn-live"]', 15_000);
  await displayed('#main-panel-chat button[aria-label="Stop response"]');
}

async function runSlowHandoffJourney() {
  await preferEnglish(browser);
  await waitForShell(browser);
  await openBot();
  // In this fixture the Bot already resolves to the only offered model.
  // Look into it locks that resolved model when it starts the scripted child.
  await browser.waitUntil(async () => (await browser.$("[data-testid=composer-model-menu]").getAttribute("aria-label"))?.startsWith("Fixture research model"), {
    timeout: 5_000, timeoutMsg: "The Bot composer did not resolve the fixture model",
  });
  const draft = await (await displayed("[data-chat-composer]")).getValue();
  const linkLabel = await (await displayed('#main-panel-chat article[aria-label^="New link: "]', 30_000)).getAttribute("aria-label");
  const link = `#main-panel-chat article[aria-label=${JSON.stringify(linkLabel)}]`;
  // The session is titled for both ends of the link ("Link: A and B").
  const title = linkLabel.replace(/^New link: /, "Link: ");
  await clickButton("Look into it", await displayed(link));

  // Bot posts nothing while the session works: the link says it is being researched, and the
  // session's row in the sidebar is the way in.
  await waitForText(await displayed(link), "Researching it. The result will arrive here.", 10_000);
  const row = await sessionRow(title);
  assert.equal(await browser.$(handoffPost(title)).isExisting(), false, "Bot posted the hand-off while its session worked");
  await capture("handoff-running-bot");

  // The row joins the run: the work so far, then each step as it arrives.
  await openRunningSession(row);
  await waitForChange('#main-panel-chat [data-turn-latest] [data-testid="turn-live"]', "The session's live step", 10_000);
  // Leaving does not stop it; coming back joins it again.
  await openBot();
  assert.equal(await (await displayed("[data-chat-composer]")).getValue(), draft, "Opening the session changed Bot's draft");
  await waitForText(await displayed(link), "Researching it. The result will arrive here.");
  assert.equal(await browser.$(handoffPost(title)).isExisting(), false, "Bot posted the hand-off while its session worked");
  await openRunningSession(row);

  // Guidance reaches the run from the session's composer.
  const composer = await displayed("[data-chat-composer]");
  await browser.execute((element) => element.scrollIntoView({ block: "nearest" }), composer);
  await composer.setValue(GUIDE);
  const guide = '#main-panel-chat button[aria-label="Guide this response"]';
  await browser.waitUntil(() => browser.execute((selector) => {
    const button = document.querySelector(selector);
    if (!(button instanceof HTMLButtonElement) || button.disabled) return false;
    button.scrollIntoView({ block: "nearest" });
    return true;
  }, guide), { timeout: 15_000, timeoutMsg: "The running reply never offered guidance for the draft" });
  await clickControl(browser, guide);
  await browser.waitUntil(async () => (await composer.getValue()) === "", { timeout: 10_000, timeoutMsg: "The guidance was not accepted" });
  await waitForText(await displayed("#main-panel-chat [data-turn-latest]"), GUIDE, 10_000);
  await capture("handoff-running-session");

  // Stop ends the run in its session; Bot then posts it once, Stopped with Open, and the link
  // offers to look again.
  await (await displayed('#main-panel-chat button[aria-label="Stop response"]')).click();
  await browser.waitUntil(async () => !(await browser.$('#main-panel-chat button[aria-label="Stop response"]').isDisplayed().catch(() => false)), {
    timeout: 20_000, timeoutMsg: "Stop did not end the hand-off's run",
  });
  await openBot();
  const stoppedPost = `${handoffPost(title)}[data-status="stopped"]`;
  const stopped = await displayed(stoppedPost, 20_000);
  await waitForText(stopped, "Stopped");
  assert.equal((await browser.$$(handoffPost(title))).length, 1, "Bot posted the hand-off more than once");
  assert.equal(await stopped.$("button=Stop").isExisting(), false, "Bot's post of a stopped hand-off offers Stop");
  await waitForRenderedButton(browser, stoppedPost, "Open");
  await childWithText(await displayed(link), "button", "Look into it again", 20_000);
  await capture("handoff-stopped-card");
}

async function verifyScriptedUsage() {
  const link = await displayed('#main-panel-chat article[aria-label^="New link: "]');
  const title = (await link.getAttribute("aria-label")).replace(/^New link: /, "Link: ");
  await clickButton("Look into it again", link);
  await displayed(`#main-panel-chat article[aria-label=${JSON.stringify(`Research: ${title}`)}][data-status="done"]`, 45_000);
  await openMainTab(browser, "settings");
  await clickButton("Usage", await displayed("nav[aria-label='Settings sections']"));
  // The scripted Pi emits a stopping assistant message without token usage.
  const usage = /^(\d[\d,]*) requests?$/u;
  await browser.waitUntil(async () => {
    const statuses = await browser.$$("#main-panel-settings [role='status']");
    for (const status of statuses) {
      const match = (await status.getText()).match(usage);
      if (match && Number(match[1].replaceAll(",", "")) >= 1) return true;
    }
    return false;
  }, { timeout: 15_000, timeoutMsg: "Usage did not count the scripted answer's request" });
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

let previewUnresponsive = false;
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

  // The knowledge fixture's Loupist holds the Map link; the next step removes it.
  if (slowHandoff) await step("a Bot hand-off runs on its own with nothing in Bot, streams into its session, takes guidance, stops and is posted once", runSlowHandoffJourney);
  if (slowHandoff) await step("Usage counts the completed scripted hand-off", verifyScriptedUsage);
  // The fixture model takes images; under --release-renderer this runs with the shipped policy enforced.
  if (slowHandoff) await step("an image and a file from the + menu's file input are previewed, sent and shown on the sent message", async () => {
    await openBot();
    performanceReport.attachments = await verifyComposerAttachments(browser);
    await capture("attachments-sent");
  });
  else console.log("• a Bot hand-off journey ... skipped: LOUPIST_E2E_SLOW_HANDOFF needs a macOS Apple Silicon debug build");

  await step("English interface and a clear onboarding start", async () => {
    await preferEnglish(browser);
    await removeUntilOnboarding();
    await waitForOnboardingStep(browser, 1, 60_000);
    await waitForText(await displayed("main[data-onboarding]"), "Watch. Remember. Explain.");
    await displayed("[data-onboarding-start]");
    await capture("onboarding-welcome");
  });

  await step("a connected account picks a preset and enters its Loupist", async () => {
    await (await displayed("[data-notice-acknowledgment]")).click();
    await clickButton("Get started");
    await waitForOnboardingStep(browser, 3, 20_000);
    await selectLoupistTemplate(browser, "Watchlist research");
    assert.equal(await browser.$("#onboarding-focus").isExisting(), false);
    assert.equal(await browser.$("[data-loupist-tone]").isExisting(), false);
    await (await displayed("[data-onboarding-next]")).click();
    // The name step: the template suggests a name, and the face colour is chosen beside it.
    await waitForOnboardingStep(browser, 4, 20_000);
    assert.equal(await (await displayed("#onboarding-name")).getValue(), "Watchlist Loupist");
    await displayed('main[data-onboarding="4"] [data-loupist-identity] .loupist-face');
    await (await displayed("[data-onboarding-create]")).click();
    await waitForShell(browser, 30_000);
    assert.equal(await selectedLoupistTitle(browser), "Watchlist Loupist");
    await displayed("[data-testid=loupist-chat-row][data-home] [aria-current=page]");
    await capture("onboarding-bot");
  });

  await step("shell, Loupist settings, and a new Chat draft", async () => {
    await openMainTab(browser, "lens");
    await displayed("[data-testid=memory-page]");
    await openMainTab(browser, "chat");
    await waitForText(await displayed("#main-panel-chat"), "AI can be wrong. Not a licensed financial adviser.");
    const before = (await browser.$$("[data-testid=loupist-chat-button]")).length;
    const newChatLabel = '[data-testid=navigation-new]';
    await clickControl(browser, newChatLabel);
    await displayed('[data-testid=loupist-chat-button][data-thread-id="__empty-chat__"][aria-current="page"]');
    const afterFirst = (await browser.$$("[data-testid=loupist-chat-button]")).length;
    await clickControl(browser, newChatLabel);
    // An unsent research session shows as one draft row under Bot; a second New research reuses it.
    assert.ok(afterFirst <= before + 1, "An unsent research session shows at most one draft row");
    assert.equal((await browser.$$("[data-testid=loupist-chat-button]")).length, afterFirst, "A second New research must reuse the unsent research session");
    await loupistSwitcherAction(browser, "settings");
    const sheet = await displayed("[data-testid=loupist-settings-page]");
    // The page header says Loupist settings; the page's own heading names the Loupist.
    await waitForText(sheet, "Watchlist Loupist");
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
    assert.ok(titles.includes("Watchlist Loupist"), `Sidebar loupists: ${titles.join(", ")}`);
    await capture("shell");
    performanceReport.shellObservedMs = (await pageTiming(browser)).observedMs;
  });

  await step("Loupist switch", async () => {
    let titles = (await visibleLoupistTitles()).filter(Boolean);
    if (titles.length < 2) {
      await loupistSwitcherAction(browser, "new");
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

  await step("Notification defaults use the full heading width in English and Korean", async () => {
    const selector = '[data-testid="notification-defaults"]';
    const check = async (title) => {
      await openSettings("notifications");
      await displayed(selector);
      const [legend] = await assertFormLegendsFit(browser, { selector });
      assert.equal(legend.text, title);
    };
    await check("Defaults by kind");
    await openSettings("appearance");
    await clickButton("한국어", await displayed("#main-panel-settings"));
    try {
      await browser.waitUntil(() => browser.execute(() => document.documentElement.lang === "ko"), {
        timeout: 10_000, timeoutMsg: "The interface did not switch to Korean",
      });
      await check("종류별 기본값");
    } finally {
      await preferEnglish(browser);
    }
  });

  await step("Settings Data connections", async () => {
    await openSettings("connections");
    const panel = await displayed("#main-panel-settings");
    // The page header names the section; the page opens with its description and the list.
    await waitForText(panel, "Available connections");
    if (process.env.LOUPIST_DEV_CUSTOM_CONNECTIONS === "1") {
      await openCustomConnections(browser);
      await displayed("[data-custom-connections] [data-custom-add]");
    } else {
      assert.equal(await browser.$("[data-custom-connections]").isExisting(), false);
      assert.equal(await browser.$("[data-custom-add]").isExisting(), false);
    }
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

  // A Settings list taller than the window once pushed Back below it, and focus then scrolled the whole app.
  await step("Settings at the minimum window keeps Back in the window and the shell in place", () => withWindowSize(browser, minimumWindowSize, async () => {
    await openSettings("agent");
    const advanced = await displayed("[data-testid=agent-config-advanced]");
    if ((await advanced.getAttribute("open")) === null) await (await advanced.$(":scope > summary")).click();
    for (let press = 0; press < 12; press += 1) await browser.keys("Tab");
    const shell = await browser.execute(measureShellLayout);
    assertShellLayout(shell, { label: "settings-minimum-window" });
    await capture("settings-minimum-window");
    await clickButton("Back");
    await waitForShell(browser);
  }));

  await step("World tab", async () => {
    await openMainTab(browser, "map");
    const panel = await displayed("[data-testid=map-view]");
    let count = await mapNameCount(browser);
    let surface = "";
    await browser.waitUntil(async () => {
      count = await mapNameCount(browser);
      surface = await browser.execute(() => {
        const canvas = document.querySelector("[data-testid=map-view] [data-testid=map-canvas] canvas");
        const box = canvas?.getBoundingClientRect();
        if (canvas && box && box.width > 1 && box.height > 1) return "canvas";
        const empty = document.querySelector("[data-testid=map-view] [data-testid=map-empty]");
        if (empty && empty.getClientRects().length > 0) return "empty";
        return "";
      });
      return surface === "canvas" || surface === "empty";
    }, {
      timeout: 20_000,
      timeoutMsg: `World did not render a canvas or an empty state (${count?.text ?? "no status"})`,
    });
    if (surface === "canvas") {
      assert.ok(count?.text, "World status did not render beside the canvas");
      // WKWebView: the opening camera frames the drawing, never in a corner of the canvas.
      let framing = null;
      await browser.waitUntil(async () => {
        framing = await browser.execute(() => {
          const canvas = document.querySelector("[data-testid=map-view] [data-testid=map-canvas] canvas");
          const stage = document.querySelector("[data-testid=map-view] .map-stage");
          const context = canvas?.getContext("2d");
          const background = stage && getComputedStyle(stage).backgroundColor.match(/[\d.]+/g)?.slice(0, 3).map(Number);
          if (!canvas || !context || !background || !canvas.width || !canvas.height) return null;
          const { width, height } = canvas;
          const pixels = context.getImageData(0, 0, width, height).data;
          let minX = width, minY = height, maxX = -1, maxY = -1;
          for (let y = 0; y < height; y += 3) for (let x = 0; x < width; x += 3) {
            const at = (y * width + x) * 4;
            if (Math.abs(pixels[at] - background[0]) + Math.abs(pixels[at + 1] - background[1]) + Math.abs(pixels[at + 2] - background[2]) > 60) {
              minX = Math.min(minX, x); minY = Math.min(minY, y); maxX = Math.max(maxX, x); maxY = Math.max(maxY, y);
            }
          }
          return maxX < 0 ? null : { x: (minX + maxX) / 2 / width, y: (minY + maxY) / 2 / height };
        });
        return Boolean(framing && framing.x > .2 && framing.x < .8 && framing.y > .15 && framing.y < .85);
      }, { timeout: 10_000, timeoutMsg: `World drawing was not framed in its canvas: ${JSON.stringify(framing)}` });
      // WKWebView, motion on: once What's new or the Library has slid in, the controls over the canvas sit beside it.
      for (const control of ["data-map-whats-new", "data-map-library"]) {
        const button = `[data-testid=map-view] button[${control}]`;
        await clickControl(browser, button);
        await displayed("[data-testid=map-view] aside[data-testid=map-library]");
        let maps = [];
        await browser.waitUntil(async () => {
          maps = await browser.execute(measureMapOverlays);
          try { assertMapOverlays(maps, { label: control }); } catch { return false; }
          return maps.some((map) => map.covers.length > 0);
        }, { timeout: 5_000, timeoutMsg: `World controls stayed under the panel ${button} opened: ${JSON.stringify(maps)}` });
        await clickControl(browser, button);
        await browser.waitUntil(async () => !await browser.$("[data-testid=map-view] aside[data-testid=map-library]").isDisplayed().catch(() => false), {
          timeout: 5_000, timeoutMsg: `The panel ${button} opened did not close`,
        });
      }
      performanceReport.mapTab = { surface: "canvas", stats: count.text, framing };
    } else {
      assert.equal(count, null, "An empty World should show guidance without a zero-count status");
      await waitForText(panel, "Your World grows as you research");
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
    await displayed('[aria-label="Market data platform details"]');
    const openbbCard = await browser.execute(() => {
      const heading = [...document.querySelectorAll("[data-testid=marketplace-card] [id^=\"connection-\"]")].find(
        (element) => element.textContent === "Market data platform",
      );
      return heading?.closest("[data-testid=marketplace-card]")?.innerText ?? "";
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
    await (await displayed('[aria-label="Web Research settings"]')).click();
    let dialog = await displayed('[role="dialog"]');
    await selectOption('[aria-label="Search provider"]', "Exa only");
    await clickButton("Save setup", dialog);
    await waitForText(dialog, "Settings saved");
    await clickButton("Close", dialog);
    await (await displayed('[aria-label="Web Research settings"]')).click();
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
    await clickButton("Add an AI account", panel);
    const connect = await dialogByTitle(browser, "Add an AI account");
    await (await connect.$('[data-connect-row="sign-in-openai"]')).waitForDisplayed({ timeout: 10_000 });
    await browser.keys("Escape");
    await openSettings("appearance");
    const dark = await displayed('button[title="Dark theme"]');
    await dark.click();
    await browser.waitUntil(async () => (await dark.getAttribute("aria-pressed")) === "true", {
      timeout: 5_000,
      timeoutMsg: "Dark theme was not saved",
    });
    assert.equal(await (await displayed("[data-testid=app-shell]")).getAttribute("data-theme"), "dark");
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
    // New Loupist at the minimum window: the templates, then the whole preview card above the fold.
    await loupistSwitcherAction(browser, "new");
    const dialog = await displayed('[role="dialog"]');
    await assertNewLoupistPreviewShows(browser);
    await capture("minimum-window-new-loupist");
    await clickButton("Cancel", dialog);
    await browser.waitUntil(async () => !(await browser.$('[role="dialog"]').isDisplayed().catch(() => false)), {
      timeout: 5_000, timeoutMsg: "New Loupist dialog did not close",
    });
  }));

  await step("removing the last Loupist returns to onboarding", async () => {
    await clickButton("Back").catch(() => {});
    await waitForShell(browser);
    await removeUntilOnboarding();
    await waitForText(await displayed("main[data-onboarding]"), "Get started");
    await capture("onboarding-after-remove");
  });

  await step("1,000-node map probe", async () => {
    assert.ok(session.devUrl, "The launcher did not record devUrl, so World probe cannot stay on this dev server");
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
        const chip = document.querySelector("article.message.assistant button[data-testid=number-chip]");
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

  const composerPreview = `${String(session.devUrl).replace(/\/$/, "")}/preview.html?scenario=composer-compact`;
  const composerWindow = await browser.getWindowSize();

  await step("⌘A selects the whole composer message from the Edit menu and the selection shows its text", async () => {
    await openComposerPreview(browser, `${composerPreview}&composerProbe=default`);
    performanceReport.composerSelectAll = await assertComposerSelectAll(browser);
  });

  // WKWebView has no `field-sizing: content`; the composer sizes its message itself.
  await step("the composer grows with its text, then scrolls with the caret in view", async () => {
    for (const size of [null, minimumWindowSize]) {
      if (size) await setWindowSize(browser, size);
      await openComposerPreview(browser, `${composerPreview}&composerProbe=${size ? "minimum" : "default"}`);
      await assertComposerGrows(browser, { onCapped: () => capture(`composer-capped${size ? "-minimum" : ""}`) });
    }
  });

  await step("the first research reply leaves a one-line composer beside a docked panel", async () => {
    await setWindowSize(browser, { width: 1280, height: 820 });
    await openComposerPreview(browser, composerPreview);
    await clickControl(browser, '[data-testid="navigation-new"]');
    await displayed('[data-thread-id="__empty-chat__"][aria-current="page"]');
    await clickControl(browser, '[data-workspace-split="side"]');
    await displayed('#workspace-panel[role="complementary"][data-placement="side"]');
    const empty = await browser.execute(composerMessageLayout);
    assert.ok(await browser.execute(() => {
      document.querySelector('textarea[data-chat-composer="true"]')?.focus();
      return document.execCommand("insertText", false, "Summarize the results.\nFocus on revenue.\nState the comparison period.");
    }), "The composer refused multiline input");
    const multiline = await browser.execute(composerMessageLayout);
    assert.ok(multiline.height > empty.height, "The multiline first question must grow the composer");
    await clickControl(browser, '.composer button[type="submit"]');
    await displayed('article.message.assistant.complete');
    await displayed('#workspace-panel[role="complementary"][data-placement="side"]');
    assert.equal(await (await displayed('textarea[data-chat-composer="true"]')).getValue(), "");
    const cleared = await browser.execute(composerMessageLayout);
    assert.ok(Math.abs(cleared.height - cleared.minHeight) <= 1.5, `The cleared composer is still tall: ${JSON.stringify(cleared)}`);
    await setWindowSize(browser, composerWindow);
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
  reportPolicyViolations();
  assert.equal(policy.violations().length, 0, "The app broke the shipped Content-Security-Policy; see the list above.");
  console.log(full ? "Loupist native release smoke passed." : "Loupist native core smoke passed.");
} catch (error) {
  if (!(error instanceof assert.AssertionError && /Content-Security-Policy/.test(error.message))) reportPolicyViolations();
  // A renderer that missed its script deadline cannot provide page diagnostics.
  if (error instanceof ComposerPreviewUnresponsive) {
    previewUnresponsive = true;
    console.error(error.message);
    throw error;
  }
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
  await browser.deleteSession().catch((error) => {
    if (!previewUnresponsive) throw error;
  });
}
