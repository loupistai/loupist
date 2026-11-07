#!/usr/bin/env node
import { readFile, mkdir } from "node:fs/promises";
import { dirname, isAbsolute, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createWebdriverHelpers } from "./webdriver-helpers.mjs";
import { openMainTab } from "./native-shell.mjs";
import { collectWorkProgress } from "./work-progress.mjs";
import { assertPresetSelection, livePreset, runLabelMatches, thinkingLabels } from "./live-preset.mjs";
import { readWindowGeometry, setWindowSize } from "./window-geometry.mjs";
import { sessionEndpointIsOwned } from "./wait-session-lib.mjs";

const e2eRoot = dirname(fileURLToPath(import.meta.url));
const artifactRoot = resolve(e2eRoot, "artifacts");
const chatModelSelector = ".composer-model-menu";
const cliArgs = process.argv.slice(2);
const explicitSession = cliArgs[0] === "--session";
const sessionPath = explicitSession ? cliArgs[1] : resolve(artifactRoot, "current-session.json");
const [command = "help", ...args] = explicitSession ? cliArgs.slice(2) : cliArgs;

const usage = `Usage:
  node apps/loupist/e2e/agent-driver.mjs [--session <absolute receipt>] <command> [arguments]
  node apps/loupist/e2e/agent-driver.mjs native-snapshot
  node apps/loupist/e2e/agent-driver.mjs inspect
  node apps/loupist/e2e/agent-driver.mjs window-default
  node apps/loupist/e2e/agent-driver.mjs window-size <700..2400> <560..1600>
  node apps/loupist/e2e/agent-driver.mjs click <selector>
  node apps/loupist/e2e/agent-driver.mjs click-text <visible text>
  node apps/loupist/e2e/agent-driver.mjs set-value <selector> <text>
  node apps/loupist/e2e/agent-driver.mjs select-option <selector> <exact text>
  node apps/loupist/e2e/agent-driver.mjs press <Enter|Escape|Tab|ArrowUp|ArrowDown>
  node apps/loupist/e2e/agent-driver.mjs screenshot <name>
  node apps/loupist/e2e/agent-driver.mjs wait-chat-idle [--strict]
  node apps/loupist/e2e/agent-driver.mjs progress
  node apps/loupist/e2e/agent-driver.mjs chat-model <model> <effort>
  node apps/loupist/e2e/agent-driver.mjs live-preset`;

function required(value, label) {
  if (!value) throw new Error(`${label} is required\n${usage}`);
  return value;
}

function parseWindowSize(args) {
  if (args.length !== 2 || args.some((value) => !/^\d+$/.test(value))) {
    throw new Error("window-size requires integer WIDTH HEIGHT (700..2400, 560..1600)");
  }
  const [width, height] = args.map(Number);
  if (!Number.isSafeInteger(width) || !Number.isSafeInteger(height)
    || width < 700 || width > 2400 || height < 560 || height > 1600) {
    throw new Error("window-size requires WIDTH 700..2400 and HEIGHT 560..1600");
  }
  return { width, height };
}

async function attach() {
  let session;
  try {
    session = JSON.parse(await readFile(sessionPath, "utf8"));
  } catch {
    throw new Error(
      `No active development window. Start npm run tauri dev or apps/loupist/e2e/up.sh first.`,
    );
  }
  if (session?.profile === "manual") {
    throw new Error(
      "This manual window has no WebDriver. Inspect it with OS tools, or close its recorded session with e2e/down.sh before starting e2e/up.sh without --manual.",
    );
  }
  if (!sessionEndpointIsOwned(session)) {
    throw new Error(
      "Refusing a session that is not the launcher-owned loopback Tauri endpoint",
    );
  }
  const { remote } = await import("webdriverio");
  const browser = await remote({
    ...session.webdriverConfig,
    capabilities: session.capabilities,
    logLevel: "error",
  });
  // A Linux session has no current window until one is selected; window
  // commands would read a 0x0 rect.
  await browser.switchToWindow("main");
  return browser;
}

async function visible(element) {
  try {
    return await element.isDisplayed();
  } catch {
    return false;
  }
}

async function inspect(browser) {
  const { title, text, controls } = await browser.execute(() => {
    const shown = (element) => {
      for (let node = element; node; node = node.parentElement) {
        if (node.hidden || node.hasAttribute("inert")) return false;
        if (node instanceof HTMLDetailsElement && !node.open && !node.querySelector(":scope > summary")?.contains(element)) return false;
        const style = getComputedStyle(node);
        if (style.display === "none" || style.visibility === "hidden") return false;
      }
      const rect = element.getBoundingClientRect();
      return rect.width > 0 && rect.height > 0;
    };
    const candidates = document.querySelectorAll(
      "button, a, input, textarea, select, summary, [role=button], [role=option], [role=menuitem], [role=menuitemradio], [role=combobox], [tabindex]",
    );
    const controls = [];
    for (const element of [...candidates].slice(0, 400)) {
      if (!shown(element)) continue;
      const tag = element.tagName.toLowerCase();
      const role = element.getAttribute("role");
      if (element.getAttribute("tabindex") === "-1" && !role && !["button", "a", "input", "textarea", "select", "summary"].includes(tag)) continue;
      const id = element.getAttribute("id");
      const ariaLabel = element.getAttribute("aria-label");
      const name = element.getAttribute("name");
      const type = element.getAttribute("type");
      const associatedLabel = "labels" in element ? [...(element.labels ?? [])].map(label => label.innerText).join(" ") : "";
      const label = (ariaLabel || associatedLabel || element.innerText || name || element.getAttribute("title") || "").trim().slice(0, 300);
      const selector = id
        ? `[id=${JSON.stringify(id)}]`
        : ariaLabel
          ? `[aria-label=${JSON.stringify(ariaLabel)}]`
          : name
            ? `${tag}[name=${JSON.stringify(name)}]`
            : null;
      controls.push({ tag, role, type, label, selector });
    }
    return { title: document.title, text: document.body.innerText.slice(0, 20_000), controls };
  });
  console.log(
    JSON.stringify(
      { title, geometry: await readWindowGeometry(browser), text, controls },
      null,
      2,
    ),
  );
}

async function displayed(browser, selector, timeout = 15_000) {
  const helpers = createWebdriverHelpers(browser, {
    defaultTimeout: timeout,
    bodyTextLimit: 20_000,
  });
  return helpers.displayed(selector, timeout);
}

async function selectExact(browser, selector, optionText) {
  const trigger = await displayed(browser, selector);
  await trigger.click();
  if ((await trigger.getAttribute("aria-expanded")) !== "true") {
    await trigger.click();
    await browser.keys("Space");
  }
  try {
    await clickVisibleMenuItem(browser, "option", optionText);
  } catch (error) {
    // WebKit can render an open Radix Select while omitting its option roles
    // from WebDriver. Keep focus on the visible trigger and use the component's
    // keyboard type-ahead; never click a hidden fallback element.
    if (!String(error).includes("Visible option not found")) throw error;
    await browser.keys(optionText);
    await browser.keys("Enter");
    await browser.waitUntil(
      async () => (await trigger.getText()).trim() === optionText,
      {
        timeout: 5_000,
        interval: 100,
        timeoutMsg: `Exact visible option was not retained: ${optionText}`,
      },
    );
  }
  return trigger;
}

async function clickVisibleMenuItem(browser, role, exactText) {
  const visibleLabels = [];
  for (let attempt = 0; attempt < 40; attempt += 1) {
    await browser.pause(100);
    let container = null;
    let items = await browser.$$(`[role="${role}"]`);
    if (role === "menuitemradio") {
      for (const menu of await browser.$$('[data-slot="dropdown-menu-content"]')) {
        if (!(await visible(menu))) continue;
        container = menu;
        items = await menu.$$(`[role="${role}"]`);
        break;
      }
      if (!container) continue;
    }
    for (const item of items) {
      const label = (await item.getText()).trim();
      if (!label) continue;
      const isVisible = await visible(item);
      if (isVisible) visibleLabels.push(label);
      if (label !== exactText) continue;
      if (isVisible) {
        await item.click();
        return;
      }
      if (container) {
        // Radix keeps clipped menu items in its roving-focus collection. Use
        // type-ahead instead of a WebKit execute script to reach one.
        await browser.keys(exactText);
        await browser.keys("Enter");
        return;
      }
    }
  }
  throw new Error(
    `Visible ${role} not found: ${exactText}; visible labels: ${visibleLabels.join(" | ") || "none"}`,
  );
}

async function openModelMenu(browser, trigger) {
  const waitForMenu = async () => {
    for (let attempt = 0; attempt < 15; attempt += 1) {
      await browser.pause(100);
      for (const menu of await browser.$$('[data-slot="dropdown-menu-content"]')) {
        if (await visible(menu)) return menu;
      }
    }
    return null;
  };
  const alreadyOpen = await waitForMenu();
  if (alreadyOpen) return alreadyOpen;
  await trigger.click();
  let menu = await waitForMenu();
  for (const key of ["Space", "Enter"]) {
    if (menu) return menu;
    await browser.keys("Escape");
    await browser.keys(key);
    menu = await waitForMenu();
  }
  if (!menu) throw new Error("Model menu did not open");
  return menu;
}

async function visibleModelMenu(browser) {
  for (const menu of await browser.$$(
    '[data-slot="dropdown-menu-content"]',
  )) {
    if (await visible(menu)) return menu;
  }
  return null;
}

async function dismissModelMenu(browser, triggerSelector = chatModelSelector, outsideSelector = "#main-tab-chat") {
  for (let attempt = 0; attempt < 3; attempt += 1) {
    if (!(await visibleModelMenu(browser))) return;
    const trigger = await browser.$(triggerSelector);
    if (
      (await visible(trigger)) &&
      (await trigger.getAttribute("aria-expanded")) === "true"
    ) {
      await trigger.click();
    } else {
      await browser.keys("Escape");
    }
    try {
      await browser.waitUntil(async () => !(await visibleModelMenu(browser)), {
        timeout: 1_500,
        interval: 100,
      });
    } catch {
      // The native WebKit driver can retain a stale menu focus target after a
      // RadioItem selection. Move focus to a stable outside control before
      // retrying, matching a normal user dismissal without DOM scripting.
      const outside = await browser.$(outsideSelector);
      if (await visible(outside)) await outside.click();
      await browser.keys("Escape");
    }
  }
  if (await visibleModelMenu(browser)) {
    throw new Error("Model menu did not close");
  }
}

async function openModelAdvanced(browser) {
  const disclosure = await browser.$(".model-menu-advanced");
  if ((await disclosure.getAttribute("aria-expanded")) === "true") return;
  await disclosure.click();
  await browser.waitUntil(async () => (await disclosure.getAttribute("aria-expanded")) === "true", {
    timeout: 2_000, interval: 100, timeoutMsg: "Model advanced options did not open",
  });
}

async function clickThinkingLevel(browser, level) {
  const aliases = new Set(thinkingLabels(level));
  for (let attempt = 0; attempt < 20; attempt += 1) {
    for (const item of await browser.$$('.model-menu-choices [role="menuitemradio"]')) {
      const label = (await item.getText()).trim();
      if (aliases.has(label)) {
        await clickVisibleMenuItem(browser, "menuitemradio", label);
        return;
      }
    }
    await browser.pause(100);
  }
  throw new Error(`Thinking level ${level} is absent from the visible model menu`);
}

async function configureChatModel(browser, modelText, effortText) {
  const chatModel = chatModelSelector;
  const chatModelControl = await browser.$(chatModel);
  if (await visible(chatModelControl)) {
    let label = (await chatModelControl.getAttribute("aria-description") ?? await chatModelControl.getText()).trim();
    if (!runLabelMatches(label, modelText, effortText)) {
      if (!(await chatModelControl.isEnabled())) {
        throw new Error(`Visible Chat model control is disabled: ${label}`);
      }
      await openModelMenu(browser, chatModelControl);
      await clickVisibleMenuItem(browser, "menuitemradio", modelText);
      await openModelAdvanced(browser);
      await clickThinkingLevel(browser, effortText);
      label = await (await displayed(browser, chatModel)).getAttribute("aria-description");
    }
    if (!runLabelMatches(label, modelText, effortText)) {
      throw new Error(
        `Exact Chat model selection was not retained: ${label}`,
      );
    }
    await dismissModelMenu(browser);
    console.log(`Verified visible Chat run model: ${label}`);
    return;
  }
  throw new Error("Chat model controls are not visible");
}

async function configurePresetModel(browser, selector, context, outsideSelector) {
  const preset = livePreset();
  const control = await displayed(browser, selector);
  let label = (await control.getAttribute("aria-description") ?? await control.getText()).trim();
  if (!selectionReady(label)) {
    if (!(await control.isEnabled())) throw new Error(`${context} model control is disabled: ${label}`);
    try {
      await openModelMenu(browser, control);
      await clickVisibleMenuItem(browser, "menuitemradio", preset.modelLabel);
      await openModelAdvanced(browser);
      await clickThinkingLevel(browser, preset.effort);
    } catch (error) {
      const menu = await browser.$('[data-slot="dropdown-menu-content"]');
      if (!(await menu.isExisting())) throw error;
      // Preserve the bounded native keyboard path when WebKit has mounted the
      // focused Radix menu but cannot report its compositor visibility.
      await browser.keys(preset.modelLabel);
      await browser.keys("Enter");
      await browser.pause(1_100);
      await openModelAdvanced(browser);
      await clickThinkingLevel(browser, preset.effort);
      await browser.keys("Escape");
    }
    label = await (await displayed(browser, selector)).getAttribute("aria-description");
  }
  assertPresetSelection(label, `${context} model menu`);
  await dismissModelMenu(browser, selector, outsideSelector);
  return label;
}

async function configureLivePreset(browser) {
  const chatPanel = await browser.$("#main-panel-chat");
  if (await chatPanel.isExisting() && await chatPanel.getAttribute("aria-hidden") === "true") {
    await openMainTab(browser, "chat");
    await browser.waitUntil(async () => (await chatPanel.getAttribute("aria-hidden")) === "false", {
      timeout: 10_000,
      timeoutMsg: "Chat did not become active before selecting the live model",
    });
  }
  const label = await configurePresetModel(browser, chatModelSelector, "Chat", "#main-tab-chat");
  console.log(`Verified visible Chat run model: ${label}`);
}

function selectionReady(label) {
  try {
    assertPresetSelection(label, "Chat model menu");
    return true;
  } catch {
    return false;
  }
}

async function dumpProgress(browser, latestOnly = false) {
  const progress = await collectWorkProgress(browser, {
    latestOnly,
  });
  console.log(JSON.stringify({ progress }, null, 2));
}

function chatIdleReport(completedAnswers, progress) {
  const progressCount = Array.isArray(progress) ? progress.length : 0;
  const settled = completedAnswers > 0;
  return {
    idle: true,
    completedAnswers,
    progressCount,
    settledAnswer: settled,
    progress,
    note: settled
      ? "A nonempty completed assistant answer is visible."
      : "Chat is idle without a nonempty completed assistant answer. Empty progress is not a completed answer.",
  };
}

async function waitForChatIdle(browser, strict = false) {
  await browser.waitUntil(
    async () => {
      const runningComposer = await browser.$(".composer-running");
      if (await visible(runningComposer)) return false;
      const stop = await browser.$('button[aria-label="Stop response"]');
      if (await visible(stop)) return false;
      for (const assistant of await browser.$$(
        "article.message.assistant.streaming",
      )) {
        if (await visible(assistant)) return false;
      }
      return true;
    },
    {
      timeout: 600_000,
      interval: 500,
      timeoutMsg: "Timed out waiting for the current Chat response to settle",
    },
  );
  await dismissModelMenu(browser);
  const answers = [];
  for (const article of await browser.$$("article.message.assistant.complete")) {
    if (!(await visible(article))) continue;
    const text = ((await article.getText()) || "").trim();
    if (text) answers.push(text);
  }
  const progress = await collectWorkProgress(browser, { latestOnly: true });
  const report = chatIdleReport(answers.length, progress);
  console.log(JSON.stringify(report, null, 2));
  if (strict && !report.settledAnswer) {
    throw new Error("wait-chat-idle --strict found no nonempty completed assistant answer");
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
if (!sessionPath || !isAbsolute(sessionPath)) throw new Error("--session requires an absolute receipt path.");
if (command === "help" || command === "--help" || command === "-h") {
  console.log(usage);
  process.exit(0);
}

if (command === "native-snapshot") {
  if (args.length) throw new Error("native-snapshot takes no arguments");
  const { captureOwnedWindow } = await import("./native-snapshot.mjs");
  console.log(JSON.stringify(await captureOwnedWindow(sessionPath, artifactRoot), null, 2));
  process.exit(0);
}

let browser;
try {
  const requestedSize = command === "window-size" ? parseWindowSize(args) : null;
  browser = await attach();
  if (command === "inspect") {
    await inspect(browser);
  } else if (command === "window-default" || command === "window-size") {
    const before = await browser.getWindowSize();
    const geometry = await setWindowSize(browser, requestedSize ?? undefined);
    console.log(JSON.stringify({ before, ...geometry }, null, 2));
  } else if (command === "click") {
    const element = await displayed(browser, required(args[0], "selector"));
    await element.waitForEnabled({ timeout: 15_000 });
    await element.click();
    console.log("Clicked visible control.");
  } else if (command === "click-text") {
    const needle = required(args[0], "visible text");
    let clicked = false;
    const buttons = await browser.$$(
      "[role=menuitemradio], [role=menuitem], [role=option], button, summary",
    );
    for (const button of buttons) {
      if (!(await visible(button))) continue;
      const label = ((await button.getText()) || "").trim();
      if (!label.includes(needle)) continue;
      const rendered = await browser.execute((element) => {
        for (let node = element; node; node = node.parentElement) {
          if (node.hidden || node.hasAttribute("inert")) return false;
          if (node instanceof HTMLDetailsElement && !node.open && !node.querySelector(":scope > summary")?.contains(element)) return false;
          const style = getComputedStyle(node);
          if (style.display === "none" || style.visibility === "hidden") return false;
        }
        const rect = element.getBoundingClientRect();
        return rect.width > 0 && rect.height > 0;
      }, button);
      if (!rendered) continue;
      await button.waitForEnabled({ timeout: 15_000 });
      await button.click();
      clicked = true;
      break;
    }
    if (!clicked) throw new Error(`Visible button not found: ${needle}`);
    console.log("Clicked visible control.");
  } else if (command === "set-value") {
    const element = await displayed(browser, required(args[0], "selector"));
    await element.setValue(required(args[1], "text"));
    console.log(
      "Set the visible control value (value intentionally not echoed).",
    );
  } else if (command === "select-option") {
    await selectExact(
      browser,
      required(args[0], "selector"),
      required(args[1], "option text"),
    );
    console.log(`Selected visible option: ${args[1]}`);
  } else if (command === "press") {
    const key = required(args[0], "key");
    const allowed = new Set(["Enter", "Escape", "Tab", "ArrowUp", "ArrowDown"]);
    if (!allowed.has(key)) throw new Error(`Key is not allowed: ${key}`);
    await browser.keys(key);
    console.log(`Pressed ${key}.`);
  } else if (command === "screenshot") {
    const name = required(args[0], "name");
    if (!/^[a-z0-9][a-z0-9._-]*$/i.test(name) || name.includes("..")) {
      throw new Error("Screenshot name must be a safe basename");
    }
    await mkdir(artifactRoot, { recursive: true, mode: 0o700 });
    const path = resolve(
      artifactRoot,
      name.endsWith(".png") ? name : `${name}.png`,
    );
    await browser.saveScreenshot(path);
    console.log(path);
  } else if (command === "wait-chat-idle") {
    if (args.some((arg) => arg !== "--strict")) {
      throw new Error(`Unknown wait-chat-idle option\n${usage}`);
    }
    await waitForChatIdle(browser, args.includes("--strict"));
  } else if (command === "progress") {
    await dumpProgress(browser, false);
  } else if (command === "chat-model") {
    await configureChatModel(
      browser,
      required(args[0], "model"),
      required(args[1], "effort"),
    );
  } else if (command === "live-preset") {
    await configureLivePreset(browser);
  } else {
    throw new Error(`Unknown command: ${command}\n${usage}`);
  }
} finally {
  if (browser) await browser.deleteSession();
}
}
