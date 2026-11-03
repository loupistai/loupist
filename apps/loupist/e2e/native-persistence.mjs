/**
 * Proves that what a user creates in the app survives the process and the folder.
 *
 *   seed     Fresh app data plus the e2e knowledge fixture (a sealed World and a
 *            saved Chat turn). Through the UI the user creates a Loupist, a Chat,
 *            a Source, a principle, a Journal entry, Loupist instructions and
 *            global/project settings.
 *   restart  The same app data in a new process shows all of it.
 *   copy     A fresh app opens a copy of the Loupist folder. Folder data follows
 *            the copy; app data (the other Loupist, global settings, the launch
 *            notice acknowledgment) does not.
 */
import assert from "node:assert/strict";
import { mkdir, readFile, realpath, writeFile } from "node:fs/promises";
import { basename, dirname, isAbsolute, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { remote } from "webdriverio";
import { createWebdriverHelpers } from "./webdriver-helpers.mjs";
import { setWindowSize } from "./window-geometry.mjs";
import { focusNativeWindow } from "./native-window-focus.mjs";
import {
  acknowledgeLaunchNotice,
  clickControl,
  dialogByTitle,
  openMainTab,
  preferEnglish,
  scrub,
  waitForShell,
} from "./native-shell.mjs";

const e2eRoot = dirname(fileURLToPath(import.meta.url));
const [sessionPath, phase, stateArg] = process.argv.slice(2);
assert.ok(
  sessionPath && ["seed", "restart", "copy"].includes(phase) && stateArg && isAbsolute(stateArg),
  "usage: node native-persistence.mjs <session.json> <seed|restart|copy> <absolute-phase-state-root>",
);

// run-app.sh registers <state>/workspace as the first Loupist; its name is the folder name.
const FOLDER_LOUPIST = "workspace";
const NEW_LOUPIST = "Persistence Loupist";
const CHAT = "Persistence chat";
// Saved by the knowledge fixture (LOUPIST_E2E_KNOWLEDGE_SEED) in the seed launch.
const FIXTURE = { chat: "Authoring chat", reply: "The sealed note was read.", source: "Sealed note", note: "Sealed research note", journal: "Keep the sealed note.", method: "Desk method" };
const SOURCE = { title: "Persistence source", text: "persistence-anchor sentence: delivered margins beat guidance." };
const PRINCIPLE = { statement: "Compare guidance with delivered results before trusting it." };
const JOURNAL = { judgment: "Margins stay above guidance through next year.", reasoning: "Recorded by the persistence suite." };
const INSTRUCTIONS = "Cite the saved persistence source before answering.";
const RETRIES = { global: 5, appDefault: 3 };

const stateRoot = await realpath(stateArg);
const runRoot = dirname(stateRoot);
const recordPath = resolve(runRoot, "persistence-record.json");
const globalConfig = resolve(stateRoot, "app-data", "config.toml");
const projectConfig = resolve(stateRoot, "workspace", ".loupist", "config.toml");
const artifactRoot = resolve(e2eRoot, "artifacts", "persistence", basename(runRoot));
await mkdir(artifactRoot, { recursive: true });

const session = JSON.parse(await readFile(sessionPath, "utf8"));
const browser = await remote({
  ...session.webdriverConfig,
  capabilities: session.capabilities,
  connectionRetryTimeout: 15_000,
  connectionRetryCount: 0,
  logLevel: "error",
});
const { displayed, childWithText, clickButton, waitForText } = createWebdriverHelpers(browser, {
  defaultTimeout: 20_000,
  bodyTextLimit: 24_000,
});
const report = {
  phase,
  mode: process.env.LOUPIST_E2E_ALLOW_UNFOCUSED === "1" ? "webview-only" : "focused-native",
  providerQuality: "not_executed",
  checks: [],
};

async function step(name, action) {
  process.stdout.write(`• ${name} ... `);
  await action();
  report.checks.push(name);
  console.log("passed");
}

async function setField(selector, value) {
  const field = await displayed(selector);
  await field.click();
  await field.setValue(value);
  await browser.waitUntil(async () => (await field.getValue()) === value, {
    timeout: 5_000, timeoutMsg: `${selector} did not take its value`,
  });
}

async function waitGone(element, message) {
  await browser.waitUntil(async () => !(await element.isDisplayed().catch(() => false)), { timeout: 20_000, timeoutMsg: message });
}

async function visibleWithText(selector, text, timeout = 20_000) {
  let found;
  await browser.waitUntil(async () => {
    for (const element of await browser.$$(selector)) {
      if ((await element.isDisplayed().catch(() => false)) && (await element.getText()).includes(text)) {
        found = element;
        return true;
      }
    }
    return false;
  }, { timeout, interval: 150, timeoutMsg: `No visible ${selector} with “${text}”` });
  return found;
}

const tomlFile = (path) => readFile(path, "utf8").catch(() => "");
// macOS temp paths reach the app through /var -> /private/var; compare real folders.
const realFile = async (path) => resolve(await realpath(dirname(path)).catch(() => dirname(path)), basename(path));

// Loupists and Chats ------------------------------------------------------------

async function sidebarLoupists() {
  return browser.execute(() => [...document.querySelectorAll("button.loupist-header-identity")].map((item) => ({
    id: item.getAttribute("data-loupist-id"),
    name: item.querySelector("strong")?.textContent?.trim() ?? "",
  })));
}

async function selectLoupist(name) {
  if (await browser.$("#main-panel-settings").isDisplayed().catch(() => false)) await openMainTab(browser, "chat");
  const target = (await sidebarLoupists()).find((item) => item.name === name);
  assert.ok(target?.id, `Loupist was not in the sidebar: ${name}`);
  const selector = `button.loupist-header-identity[data-loupist-id=${JSON.stringify(target.id)}]`;
  const button = await displayed(selector);
  if ((await button.getAttribute("aria-pressed")) !== "true") await button.click();
  await browser.waitUntil(async () => (await browser.$(selector).getAttribute("aria-pressed")) === "true"
    && (await browser.$(".loupist-header-identity[aria-pressed='true'] strong").getText().catch(() => "")) === name, {
    timeout: 20_000, timeoutMsg: `Loupist was not selected: ${name}`,
  });
}

const chatRow = (title) => `.loupist-navigation button.loupist-chat-button[aria-label=${JSON.stringify(title)}]`;

async function chatTitles() {
  return browser.execute(() => [...document.querySelectorAll(".loupist-navigation button.loupist-chat-button")].map((button) => button.getAttribute("aria-label")));
}

async function createLoupist() {
  await (await displayed("button.loupist-header-new")).click();
  const dialog = await dialogByTitle(browser, "New Loupist");
  await (await childWithText(dialog, "button.loupist-template-card strong", "Dividend income")).click();
  await setField("#new-loupist-name", NEW_LOUPIST);
  await clickButton("Create Loupist", dialog);
  await waitGone(dialog, "The New Loupist dialog did not close");
  await selectLoupist(NEW_LOUPIST);
}

/** The composer's /new command creates and opens a saved, empty Chat. */
async function createChatFromComposer() {
  await openMainTab(browser, "chat");
  const before = await chatTitles();
  const composer = await displayed("#main-panel-chat textarea[data-chat-composer]");
  await composer.click();
  await composer.setValue("/new");
  await (await displayed('[role="listbox"] [role="option"][aria-label="New chat"]')).click();
  await browser.waitUntil(async () => (await chatTitles()).filter((title) => title === "New chat").length === before.filter((title) => title === "New chat").length + 1, {
    timeout: 15_000, timeoutMsg: "The /new command did not save a Chat",
  });
  await browser.waitUntil(async () => (await browser.$(chatRow("New chat")).getAttribute("aria-current")) === "page", {
    timeout: 10_000, timeoutMsg: "The new Chat did not open",
  });
}

async function renameChat(from, to) {
  const trigger = `.loupist-navigation button.loupist-chat-menu[aria-label=${JSON.stringify(`Options for ${from}`)}]`;
  await clickControl(browser, trigger);
  if ((await browser.$(trigger).getAttribute("aria-expanded")) !== "true") {
    // The pinned driver can click without the pointerdown Radix listens for.
    await browser.execute((selector) => document.querySelector(selector)?.focus(), trigger);
    await browser.keys("Enter");
  }
  await browser.waitUntil(async () => (await browser.$(trigger).getAttribute("aria-expanded")) === "true", {
    timeout: 5_000, timeoutMsg: "The Chat options menu did not open",
  });
  await (await visibleWithText('[role="menuitem"]', "Rename")).click();
  const dialog = await dialogByTitle(browser, "Rename Chat");
  await setField("#rename-thread-name", to);
  await clickButton("Save", dialog);
  await waitGone(dialog, "The Rename Chat dialog did not close");
  await browser.$(chatRow(to)).waitForExist({ timeout: 15_000, timeoutMsg: `Chat was not renamed to ${to}` });
}

async function verifyChats() {
  const titles = await chatTitles();
  assert.ok(titles.includes(CHAT) && titles.includes(FIXTURE.chat), `Chats for ${FOLDER_LOUPIST}: ${titles.join(", ")}`);
  await (await displayed(chatRow(FIXTURE.chat))).click();
  await waitForText(await displayed("#main-panel-chat"), FIXTURE.reply);
  await (await displayed(chatRow(CHAT))).click();
  await browser.waitUntil(async () => (await browser.$(chatRow(CHAT)).getAttribute("aria-current")) === "page"
    && (await browser.$$("#main-panel-chat [data-message-id]")).length === 0, {
    timeout: 10_000, timeoutMsg: "The empty saved Chat did not open",
  });
}

// Map and its Library ---------------------------------------------------------

async function mapStats() {
  let text = "";
  await browser.waitUntil(async () => {
    text = (await browser.$(".map-view .map-stats span").getText().catch(() => "")).trim();
    return /^[\d,]+ on the map · [\d,]+ relationships$/.test(text);
  }, { timeout: 30_000, interval: 200, timeoutMsg: `The Map did not report its size (${text})` });
  return text;
}

/** Journal entries are Map nodes, so the size is read once the Library writes have settled. */
async function settledMapStats() {
  await openMainTab(browser, "map");
  let previous = await mapStats();
  for (let read = 0; read < 10; read += 1) {
    await browser.pause(1_000);
    const current = await mapStats();
    if (current === previous) return current;
    previous = current;
  }
  throw new Error(`The Map size did not settle (${previous})`);
}

async function openLibrary(tabLabel) {
  await openMainTab(browser, "map");
  await mapStats();
  if (!await browser.$(".map-view aside.map-library").isDisplayed().catch(() => false)) {
    await (await displayed(".map-view [data-map-more]")).click();
    await (await displayed('[role="menuitemcheckbox"][data-map-library]')).click();
  }
  const library = await displayed(".map-view aside.map-library");
  const tab = await childWithText(library, '[role="tab"]', tabLabel);
  if ((await tab.getAttribute("aria-selected")) !== "true") {
    await tab.click();
    if ((await tab.getAttribute("aria-selected")) !== "true") {
      await browser.execute((element) => element.focus(), tab);
      await browser.keys("Enter");
    }
  }
  await browser.waitUntil(async () => (await tab.getAttribute("aria-selected")) === "true", {
    timeout: 5_000, timeoutMsg: `Library tab did not open: ${tabLabel}`,
  });
  return library;
}

async function openMemorySegment(segment) {
  await openMainTab(browser, "lens");
  const page = await displayed('section.memory-page');
  const tabIndex = { about: 1, principles: 2, methods: 3 }[segment];
  assert.ok(tabIndex, `Unknown What it knows segment: ${segment}`);
  await (await displayed(`section.memory-page [role="tablist"] [role="tab"]:nth-child(${tabIndex})`)).click();
  await browser.waitUntil(async () => (await page.getAttribute('data-segment')) === segment, {
    timeout: 5_000, timeoutMsg: `What it knows did not open ${segment}`,
  });
  return page;
}

async function addSource() {
  const library = await openLibrary("Sources");
  await clickButton("Add source", library);
  const form = await displayed('form[aria-label="Add source"]');
  await setField('form[aria-label="Add source"] input[aria-label="Source title"]', SOURCE.title);
  await setField('form[aria-label="Add source"] textarea[aria-label="Source text"]', SOURCE.text);
  await clickButton("Save source", form);
  await waitGone(form, "The source dialog did not close after saving");
  const reader = await displayed('.map-view article[aria-label="Source reader"]');
  await waitForText(reader, SOURCE.text);
  const id = await reader.getAttribute("data-source-id");
  assert.match(id ?? "", /^src_[0-9a-f]{32}$/);
  return id;
}

async function verifySource(id) {
  const library = await openLibrary("Sources");
  await waitForText(library, FIXTURE.source);
  const row = await displayed(`.map-view aside.map-library button.source-library-row[data-source-id="${id}"]`);
  await waitForText(row, SOURCE.title);
  await row.click();
  const reader = await displayed('.map-view article[aria-label="Source reader"]');
  await waitForText(reader, SOURCE.text);
  assert.equal(await reader.getAttribute("data-source-id"), id, "The saved Source changed identity");
}

async function addPrinciple() {
  await openMainTab(browser, "chat");
  await (await displayed(chatRow(FIXTURE.chat))).click();
  const answer = await displayed("article.message.assistant[data-latest]");
  await clickButton("Suggest a correction", answer);
  await setField(".answer-correction textarea", PRINCIPLE.statement);
  await clickButton("Save correction", answer);
  await (await displayed("[data-feedback-principle]")).click();
  const form = await displayed("form.principle-form");
  await (await form.$('button[type="submit"]')).click();
  await waitGone(form, "The principle form did not close after saving");
  await verifyPrinciple();
}

async function verifyPrinciple() {
  const page = await openMemorySegment("principles");
  const row = await visibleWithText("section.memory-page button.memory-row", PRINCIPLE.statement);
  await row.click();
  await waitForText(await displayed("section.memory-page aside.memory-reader"), PRINCIPLE.statement);
  await openMemorySegment("methods");
  await waitForText(page, FIXTURE.method);
}

async function addJournalEntry() {
  const library = await openLibrary("Journal & notes");
  await clickButton("New entry", library);
  const form = await displayed(".map-view aside.map-library form.journal-form");
  const [judgment, reasoning] = await form.$$("textarea");
  await judgment.setValue(JOURNAL.judgment);
  await reasoning.setValue(JOURNAL.reasoning);
  await clickButton("Save entry", form);
  await waitGone(form, "The Journal form did not close after saving");
  const row = await visibleWithText(".map-view aside.map-library button.journal-library-row", JOURNAL.judgment);
  return row.getAttribute("data-research-id");
}

async function verifyJournal(id) {
  const library = await openLibrary("Journal & notes");
  await waitForText(library, FIXTURE.journal);
  await waitForText(library, FIXTURE.note);
  const row = await displayed(`.map-view aside.map-library button.journal-library-row[data-research-id="${id}"]`);
  await waitForText(row, JOURNAL.judgment);
  await row.click();
  const reader = await displayed('.map-view aside.map-reader article[aria-label]');
  await waitForText(reader, JOURNAL.judgment);
  await waitForText(reader, JOURNAL.reasoning);
}

// Loupist and app settings ---------------------------------------------------------

async function saveInstructions() {
  await openMainTab(browser, "chat");
  await (await displayed("[data-header-loupist-settings]")).click();
  const field = "#loupist-profile-instructions";
  await setField(field, INSTRUCTIONS);
  await clickButton("Save", await displayed(".loupist-profile-form"));
  await browser.waitUntil(async () => (await (await browser.$(field)).getValue()) === INSTRUCTIONS
    && (await browser.$(".loupist-profile-form [role='status']").getText().catch(() => "")) === "Saved", {
    timeout: 10_000, timeoutMsg: "Loupist instructions did not save",
  });
}

async function verifyInstructions() {
  await openMainTab(browser, "chat");
  await (await displayed("[data-header-loupist-settings]")).click();
  await browser.waitUntil(async () => (await (await displayed("#loupist-profile-instructions")).getValue()) === INSTRUCTIONS, {
    timeout: 10_000, timeoutMsg: "Loupist instructions were not restored",
  });
}

async function settingsSection(id) {
  await openMainTab(browser, "settings");
  await (await displayed(`#settings-section-${id}`)).click();
  await displayed(`#settings-section-${id}[aria-current="page"]`);
}

async function expand(selector) {
  const details = await displayed(selector);
  if ((await details.getAttribute("open")) === null) await (await details.$("summary")).click();
  await browser.waitUntil(async () => (await details.getAttribute("open")) !== null, { timeout: 5_000, timeoutMsg: `${selector} did not open` });
}

/** Chat preferences for the global defaults or the folder Loupist, with the file it edits. */
async function chatPreferences(scope) {
  if (scope === "global") await settingsSection("agent");
  else {
    await openMainTab(browser, "chat");
    await (await displayed("[data-header-loupist-settings]")).click();
    await expand('[data-loupist-section="chat"]');
  }
  await expand(".agent-config-file-details");
  const expected = scope === "global" ? globalConfig : projectConfig;
  let observed = "";
  await browser.waitUntil(async () => {
    observed = (await browser.$(".agent-config-file button[title]").getAttribute("title").catch(() => "")) ?? "";
    return Boolean(observed) && (await realFile(observed)) === expected;
  }, { timeout: 10_000, timeoutMsg: `Chat preferences did not switch to ${scope}` }).catch((error) => {
    throw new Error(`${error.message}; actual=${observed}; expected=${expected}`);
  });
  await expand(".agent-config-advanced");
}

async function configValue(path, expected) {
  let actual;
  await browser.waitUntil(async () => {
    actual = await (await displayed(`[id="config-${path}"]`)).getValue();
    return actual === String(expected);
  }, { timeout: 10_000, timeoutMsg: `${path} showed ${actual}, expected ${expected}` }).catch((error) => {
    throw new Error(`${error.message}; shown=${actual}`);
  });
}

async function saveRetries(value) {
  const input = await displayed('[id="config-pi.retry.max_retries"]');
  await input.setValue(String(value));
  const save = await displayed('button[aria-label="Save Maximum retries"]');
  await browser.waitUntil(() => save.isEnabled(), { timeout: 5_000, timeoutMsg: "Maximum retries could not be saved" });
  await save.click();
  await browser.waitUntil(async () => !(await save.isEnabled()) && (await input.isEnabled()) && (await input.getValue()) === String(value), {
    timeout: 10_000, timeoutMsg: "Maximum retries did not save",
  });
}

async function theme(label) {
  await settingsSection("appearance");
  return displayed(`button[aria-label="${label} theme"]`);
}

async function assertPressed(element, message) {
  await browser.waitUntil(async () => (await element.getAttribute("aria-pressed")) === "true", { timeout: 10_000, timeoutMsg: message });
}

async function seedSettings() {
  await chatPreferences("global");
  await saveRetries(RETRIES.global);
  assert.match(await tomlFile(globalConfig), /max_retries\s*=\s*5\b/, "Global retries did not reach config.toml");
  await chatPreferences(FOLDER_LOUPIST);
  await configValue("pi.retry.max_retries", RETRIES.global);
  assert.equal(await browser.$('button[aria-label="Reset Maximum retries"]').isExisting(), false, "An inherited value has no local reset");
  await (await displayed('[id="config-pi.retry.enabled"]')).selectByAttribute("value", "false");
  await browser.waitUntil(async () => /enabled\s*=\s*false/.test(await tomlFile(projectConfig)), {
    timeout: 10_000, timeoutMsg: "The Loupist's retry setting did not reach its folder config.toml",
  });
  assert.doesNotMatch(await tomlFile(projectConfig), /max_retries/, "The inherited value must not be copied into the folder");
  const dark = await theme("Dark");
  await dark.click();
  await assertPressed(dark, "Dark theme was not selected");
  await browser.waitUntil(async () => /theme\s*=\s*"dark"/.test(await tomlFile(globalConfig)), {
    timeout: 10_000, timeoutMsg: "The theme did not reach the global config.toml",
  });
}

/** After a restart global values remain; in a copied folder only the Loupist's own values follow. */
async function verifySettings({ copied }) {
  const retries = copied ? RETRIES.appDefault : RETRIES.global;
  await chatPreferences("global");
  await configValue("pi.retry.max_retries", retries);
  await chatPreferences(FOLDER_LOUPIST);
  await configValue("pi.retry.enabled", false);
  await configValue("pi.retry.max_retries", retries);
  await assertPressed(await theme(copied ? "System" : "Dark"), `Theme after ${phase} did not match its owner`);
  if (!copied) {
    const english = await childWithText(await displayed('#main-panel-settings [role="group"][aria-label="Language"]'), "button", "English");
    await assertPressed(english, "The interface language was not restored");
  }
}

// Phases ----------------------------------------------------------------------------

async function startShell({ fresh }) {
  await focusNativeWindow(browser, session);
  report.window = await setWindowSize(browser);
  await waitForShell(browser, 60_000);
  const required = await acknowledgeLaunchNotice(browser);
  assert.equal(required, fresh, fresh ? "Fresh app data must ask for the launch notice" : "The launch notice acknowledgment was not kept");
  if (fresh) {
    await preferEnglish(browser);
    return;
  }
  // The saved language applies without setting it again.
  await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.lang)) === "en", {
    timeout: 20_000, timeoutMsg: "The saved interface language was not applied after restart",
  });
  await browser.waitUntil(async () => (await browser.$(".app-shell").getAttribute("data-theme")) === "dark", {
    timeout: 10_000, timeoutMsg: "The saved theme was not applied after restart",
  });
}

async function seed() {
  const record = {};
  await step("fresh app data asks for the notice and starts with the folder Loupist", async () => {
    await startShell({ fresh: true });
    const loupists = await sidebarLoupists();
    assert.deepEqual(loupists.map((loupist) => loupist.name), [FOLDER_LOUPIST]);
    record.folderLoupistId = loupists[0].id;
  });
  await step("create a Loupist from New Loupist", async () => {
    await createLoupist();
    record.newLoupistId = (await sidebarLoupists()).find((loupist) => loupist.name === NEW_LOUPIST)?.id;
    assert.ok(record.newLoupistId, "The new Loupist has no identity in the sidebar");
  });
  await step("create a Chat with /new and rename it", async () => {
    await selectLoupist(FOLDER_LOUPIST);
    await createChatFromComposer();
    await renameChat("New chat", CHAT);
  });
  await step("save a Source from the Map Library", async () => {
    record.sourceId = await addSource();
  });
  await step("save a rule in What it knows", addPrinciple);
  await step("save a Journal entry in the Library", async () => {
    record.journalId = await addJournalEntry();
    assert.ok(record.journalId, "The Journal entry has no identity");
  });
  await step("save Loupist instructions", saveInstructions);
  await step("save global and Loupist Chat preferences and the theme", seedSettings);
  await step("record the Map with the World and the new Journal entry", async () => {
    await selectLoupist(FOLDER_LOUPIST);
    record.mapStats = await settledMapStats();
  });
  await browser.saveScreenshot(resolve(artifactRoot, "seed.png"));
  await writeFile(recordPath, `${JSON.stringify(record, null, 2)}\n`);
  report.record = record;
}

async function verify(record, { copied }) {
  await step(copied ? "a fresh app opens the copied folder as the same Loupist" : "the restarted app keeps the notice, language, theme and Loupists", async () => {
    await startShell({ fresh: copied });
    const loupists = await sidebarLoupists();
    const expected = copied
      ? [{ id: record.folderLoupistId, name: FOLDER_LOUPIST }]
      : [{ id: record.folderLoupistId, name: FOLDER_LOUPIST }, { id: record.newLoupistId, name: NEW_LOUPIST }];
    assert.deepEqual(
      [...loupists].sort((a, b) => a.name.localeCompare(b.name)),
      [...expected].sort((a, b) => a.name.localeCompare(b.name)),
      "Loupists after the launch do not match their owners",
    );
  });
  if (!copied) await step("the created Loupist keeps its template focus", async () => {
    await selectLoupist(NEW_LOUPIST);
    await (await displayed("[data-header-loupist-settings]")).click();
    assert.equal(await (await displayed("#loupist-profile-focus")).getValue(), "Dividend payers and payout safety");
    await (await displayed("[data-loupist-settings-back]")).click();
  });
  await step("saved and renamed Chats", async () => {
    await selectLoupist(FOLDER_LOUPIST);
    await verifyChats();
  });
  await step("the Map keeps its World and Journal nodes", async () => {
    assert.equal(await settledMapStats(), record.mapStats);
  });
  await step("the Library keeps the Source", () => verifySource(record.sourceId));
  await step("What it knows keeps the rule and learned method", verifyPrinciple);
  await step("the Library keeps the Journal entry", () => verifyJournal(record.journalId));
  await step("Loupist instructions", verifyInstructions);
  await step(copied ? "Loupist preferences follow the folder; global settings stay with the app" : "global and Loupist preferences", () => verifySettings({ copied }));
  await browser.saveScreenshot(resolve(artifactRoot, `${phase}.png`));
}

try {
  if (phase === "seed") await seed();
  else {
    const record = JSON.parse(await readFile(recordPath, "utf8"));
    await verify(record, { copied: phase === "copy" });
  }
  report.status = "passed";
  await writeFile(resolve(artifactRoot, `${phase}.json`), `${JSON.stringify(report, null, 2)}\n`);
  console.log(`Loupist native persistence ${phase} passed.`);
} catch (error) {
  const state = await browser.execute(() => ({
    panels: [...document.querySelectorAll('[id^="main-panel-"]')].filter((element) => !element.hidden).map((element) => element.id),
    dialogs: [...document.querySelectorAll('[role="dialog"]')].map((element) => element.getAttribute("aria-label") ?? element.textContent?.slice(0, 80)),
    body: document.body.innerText.slice(0, 6_000),
  })).catch(() => ({}));
  console.error("\nNative failure state:", scrub(JSON.stringify(state)));
  await browser.saveScreenshot(resolve(artifactRoot, `${phase}-failure.png`)).catch(() => {});
  await writeFile(resolve(artifactRoot, `${phase}-failure.json`), `${JSON.stringify({ ...report, status: "failed", error: String(error) }, null, 2)}\n`).catch(() => {});
  throw error;
} finally {
  await browser.deleteSession().catch(() => {});
}
