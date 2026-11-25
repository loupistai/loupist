/**
 * Proves that what a user creates in the app survives the process and the folder.
 *
 *   seed     Fresh app data plus the e2e knowledge fixture (a sealed World and a
 *            saved Chat turn). Through the UI the user creates a Loupist, a Chat,
 *            a Source, a principle, two assets followed by name as typed (one
 *            then unfollowed, the other with a reason on its page), a Journal
 *            entry, Loupist instructions and global/project settings. Bot's
 *            pre-update hand-off is stopped with its reason, the hand-off sessions
 *            show Bot’s brief and the quoted request, and Look into it on
 *            the retained link is pressed once.
 *   restart  The same app data in a new process shows all of it; the other
 *            Loupist follows none of the folder Loupist's assets.
 *   copy     A fresh app opens a copy of the Loupist folder. Folder data follows
 *            the copy; app data (the other Loupist, each Loupist's follows, global
 *            settings, the launch notice acknowledgment) does not.
 *   fork     A second copy opens on its own fresh app data, like copy.
 *   reconnect
 *            The second copy is put at the first copy's registered path; the
 *            copy's app reconnects it and finds the folder's data.
 */
import assert from "node:assert/strict";
import { mkdir, readFile, realpath, writeFile } from "node:fs/promises";
import { basename, dirname, isAbsolute, resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";
import { remote } from "webdriverio";
import { createWebdriverHelpers } from "./webdriver-helpers.mjs";
import { setWindowSize } from "./window-geometry.mjs";
import { focusNativeWindow } from "./native-window-focus.mjs";
import {
  acknowledgeLaunchNotice,
  clickControl,
  dialogByTitle,
  loupistSwitcherAction,
  openLoupistSwitcher,
  openMainTab,
  openMapLibrary,
  preferEnglish,
  readVisibleMapStats,
  scrub,
  selectedLoupistTitle,
  switcherLoupists,
  waitForLoupistSwitcherClosed,
  waitForShell,
} from "./native-shell.mjs";

const e2eRoot = dirname(fileURLToPath(import.meta.url));
const [sessionPath, phase, stateArg] = process.argv.slice(2);
assert.ok(
  sessionPath && ["seed", "restart", "copy", "fork", "reconnect"].includes(phase) && stateArg && isAbsolute(stateArg),
  "usage: node native-persistence.mjs <session.json> <seed|restart|copy|fork|reconnect> <absolute-phase-state-root>",
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
// Saved by the host's hand-off fixture beside the knowledge fixture: a retained Bot link, a Bot-origin
// session (a request over 2,000 characters with a note), a link-origin session and a receipt-less
// Created hand-off, which the seed launch stops before it runs.
const HANDOFF = {
  link: "New link: Lamppost Test Works and Newpost Test Works",
  botSession: { title: "Sealed supply check", lastSentence: "Answer in one table and skip any forecasts.", noteQuestion: "Who else supplies the sealed part?" },
  linkSession: { title: "Link: Sealed Hub and Lamppost Test Works" },
  legacy: { title: "Pre-update research", reason: "Opened before an update. Ask again to run it." },
};
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
  return switcherLoupists(browser);
}

async function selectLoupist(name) {
  if (await browser.$("#main-panel-settings").isDisplayed().catch(() => false)) await openMainTab(browser, "chat");
  const target = (await sidebarLoupists()).find((item) => item.name === name);
  assert.ok(target?.id, `Loupist was not in the switcher: ${name}`);
  const current = "[data-testid=loupist-header-identity]";
  if ((await browser.$(current).getAttribute("data-loupist-id")) !== target.id) {
    await openLoupistSwitcher(browser);
    await (await displayed(`[data-loupist-option=${JSON.stringify(target.id)}]`)).click();
    await waitForLoupistSwitcherClosed(browser);
  }
  await browser.waitUntil(async () => (await browser.$(current).getAttribute("data-loupist-id")) === target.id
    && (await selectedLoupistTitle(browser)) === name, {
    timeout: 20_000, timeoutMsg: `Loupist was not selected: ${name}`,
  });
}

const chatRow = (title) => `[data-testid=loupist-navigation] button[data-testid=loupist-chat-button][aria-label=${JSON.stringify(title)}]`;

async function chatTitles() {
  return browser.execute(() => [...document.querySelectorAll("[data-testid=loupist-navigation] button[data-testid=loupist-chat-button]")].map((button) => button.querySelector("[data-testid=loupist-chat-title]")?.textContent?.trim() ?? ""));
}

async function createLoupist() {
  await loupistSwitcherAction(browser, "new");
  const dialog = await dialogByTitle(browser, "New Loupist");
  await (await childWithText(dialog, "button[data-testid=loupist-template-card] strong", "Dividend income")).click();
  await setField("#new-loupist-name", NEW_LOUPIST);
  await clickButton("Create Loupist", dialog);
  await waitGone(dialog, "The New Loupist dialog did not close");
  await selectLoupist(NEW_LOUPIST);
}

/** Journal's Look back creates a saved session without sending a model request. */
async function createJournalChat() {
  await openLibrary("Journal");
  await (await visibleWithText("[data-testid=map-view] button[data-testid=journal-library-row]", FIXTURE.journal)).click();
  const reader = await displayed("[data-testid=knowledge-journal]");
  await clickButton("Look back with only what was known then", reader);
  const title = `Looking back: ${FIXTURE.journal}`;
  await displayed(`${chatRow(title)}[aria-current="page"]`);
  return title;
}

async function renameChat(from, to) {
  const trigger = `[data-testid=loupist-navigation] button[data-testid=loupist-chat-menu][aria-label=${JSON.stringify(`Options for ${from}`)}]`;
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
  const dialog = await dialogByTitle(browser, "Rename research session");
  await setField("#rename-thread-name", to);
  await clickButton("Save", dialog);
  await waitGone(dialog, "The Rename research session dialog did not close");
  await browser.$(chatRow(to)).waitForExist({ timeout: 15_000, timeoutMsg: `Chat was not renamed to ${to}` });
}

async function verifyChats() {
  const titles = await chatTitles();
  assert.ok(titles.includes(CHAT) && titles.includes(FIXTURE.chat), `Chats for ${FOLDER_LOUPIST}: ${titles.join(", ")}`);
  await openChat(FIXTURE.chat);
  await waitForText(await displayed("#main-panel-chat"), FIXTURE.reply);
  await (await displayed(chatRow(CHAT))).click();
  await browser.waitUntil(async () => (await browser.$(chatRow(CHAT)).getAttribute("aria-current")) === "page"
    && (await browser.$$("#main-panel-chat [data-message-id]")).length === 0, {
    timeout: 10_000, timeoutMsg: "The empty saved Chat did not open",
  });
}

// Research hand-offs ------------------------------------------------------------

const chatCount = async (title) => (await chatTitles()).filter((item) => item === title).length;

async function openBot() {
  await openMainTab(browser, "chat");
  const selector = "[data-testid=loupist-chat-row][data-home] [data-testid=loupist-chat-button]";
  if (await browser.$(selector).getAttribute("aria-current") !== "page") {
    await clickControl(browser, selector);
  }
  await displayed(`${selector}[aria-current="page"]`);
}

/** Bot's card for the pre-update hand-off: stopped with its reason, Open kept, and no session in its place. */
async function verifyLegacyHandoff() {
  await openBot();
  const card = await displayed(`.thread-delivery[aria-label=${JSON.stringify(`Research: ${HANDOFF.legacy.title}`)}]`, 30_000);
  assert.equal(await card.getAttribute("data-status"), "stopped", "The pre-update hand-off is not stopped");
  await waitForText(card, HANDOFF.legacy.reason);
  assert.ok(await (await card.$("button=Open")).isDisplayed(), "The stopped hand-off lost Open");
  assert.equal(await chatCount(HANDOFF.legacy.title), 1, "The stopped hand-off has a replacement session");
}

/** Opens a research session from the sidebar, or from Search when it is no longer among the recent ones. */
async function openChat(title) {
  const row = await browser.$(chatRow(title));
  if (await row.isDisplayed().catch(() => false)) {
    await row.click();
  } else {
    await clickControl(browser, "button.navigation-search");
    const input = await displayed(".command-palette-input");
    await input.setValue(title);
    const option = await visibleWithText('[role="option"]', title);
    await option.click();
  }
  await browser.waitUntil(async () => (await browser.$(chatRow(title)).getAttribute("aria-current")) === "page", {
    timeout: 10_000, timeoutMsg: `The research session did not open: ${title}`,
  });
}

async function openSession(title) {
  await openChat(title);
  await browser.waitUntil(async () => (await browser.$(chatRow(title)).getAttribute("aria-current")) === "page", {
    timeout: 10_000, timeoutMsg: `The session did not open: ${title}`,
  });
  return displayed("#main-panel-chat [data-message-id]");
}

const isOpen = async (details) => (await details.getAttribute("open")) !== null;

/** Bot's brief opens the session. The closed details preserve the original request in full. */
async function verifyBotHandoffSession() {
  const first = await openSession(HANDOFF.botSession.title);
  await waitForText(first, "From Bot");
  await waitForText(first, "Compare retained evidence and dated filings.");
  assert.equal(await (await first.$("button=Edit")).isExisting(), false);
  const note = await first.$("details.message-handoff-note");
  assert.equal(await isOpen(note), false);
  await (await note.$("summary")).click();
  await waitForText(note, HANDOFF.botSession.noteQuestion);
  await waitForText(note, "You said in Bot");
  const quote = await (await note.$("blockquote")).getText();
  assert.ok(quote.length > 2_000 && quote.endsWith(HANDOFF.botSession.lastSentence), "The quoted request is incomplete");
  await (await note.$("summary")).click();
  await browser.waitUntil(async () => !(await isOpen(note)), { timeout: 5_000, timeoutMsg: "The hand-off details did not close" });
}

/** The link-origin session: the chosen action instead of the host's caption, and the link's details closed. */
async function verifyLinkHandoffSession() {
  const first = await openSession(HANDOFF.linkSession.title);
  await waitForText(first, "You chose “Look into it” on a link in Bot.");
  const note = await first.$("details.message-handoff-note");
  assert.equal(await (await note.$("summary")).getText(), "Link details");
  assert.equal(await isOpen(note), false, "The link's details were not closed");
  assert.equal(await (await first.$("button=Edit")).isExisting(), false, "A chosen action offers Edit");
  await (await note.$("summary")).click();
  await waitForText(note, "Start: Sealed Hub");
  await waitForText(note, "Lamppost Test Works → supplies → Sealed Hub");
  await waitForText(note, "End: Lamppost Test Works");
}

/** Clicking the retained link must create one session in the credential-free profile. */
async function lookIntoSeededLink() {
  await openBot();
  const before = await chatTitles();
  const card = await displayed(`article.thread-link[aria-label=${JSON.stringify(HANDOFF.link)}]`, 30_000);
  await clickButton("Look into it", card);
  const title = "Link: Lamppost Test Works and Newpost Test Works";
  let status;
  await browser.waitUntil(async () => {
    const alert = await card.$("[role=alert]");
    if (await alert.isDisplayed().catch(() => false)) assert.fail(await alert.getText());
    const line = await card.$("[role=status]");
    status = await line.isDisplayed().catch(() => false) ? await line.getText() : null;
    return Boolean(status) && (await chatTitles()).includes(title);
  }, { timeout: 30_000, interval: 250, timeoutMsg: "Look into it showed no outcome" });
  const added = (await chatTitles()).filter((title) => !before.includes(title));
  assert.deepEqual(added, [title], "Look into it must add exactly one session");
  return { status, session: title };
}

/** Everything the hand-off fixture keeps, read again after a launch. */
async function verifyHandoffs(record) {
  await verifyLegacyHandoff();
  await verifyBotHandoffSession();
  await verifyLinkHandoffSession();
  if (record.linkLookInto?.session) assert.equal(await chatCount(record.linkLookInto.session), 1, "The link's session was not kept exactly once");
}

// Map and its Library ---------------------------------------------------------

async function mapStats() {
  let text = "";
  await browser.waitUntil(async () => {
    // The reader and the Library can mount after the Map and cover its status line.
    await closeMapReader();
    await closeLibrary();
    text = await readVisibleMapStats(browser);
    return /^[\d,]+ in World · [\d,]+ relationships$/.test(text);
  }, { timeout: 30_000, interval: 200, timeoutMsg: `World did not report its size (${text})` });
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
  throw new Error(`World size did not settle (${previous})`);
}

async function closeMapReader() {
  const selector = "[data-testid=map-view] aside[data-testid=map-reader]";
  if (!await browser.$(selector).isDisplayed().catch(() => false)) return;
  await clickControl(browser, `${selector} [data-testid=map-reader-actions] button[aria-label]`);
  await browser.waitUntil(async () => !await browser.$(selector).isDisplayed().catch(() => false), {
    timeout: 10_000, timeoutMsg: "World reader did not close",
  });
}

// Below a 720px map width the Library covers the canvas and its status line.
async function closeLibrary() {
  const selector = "[data-testid=map-view] aside[data-testid=map-library]";
  if (!await browser.$(selector).isDisplayed().catch(() => false)) return;
  await clickControl(browser, `${selector} > header button[aria-label="Close Library"]`);
  await browser.waitUntil(async () => !await browser.$(selector).isDisplayed().catch(() => false), {
    timeout: 10_000, timeoutMsg: "World Library did not close",
  });
}

async function openLibrary(tabLabel) {
  await openMainTab(browser, "map");
  await mapStats();
  const library = await openMapLibrary(browser);
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
  const page = await displayed('section[data-testid=memory-page]');
  const tabIndex = { pages: 1, history: 2 }[segment];
  assert.ok(tabIndex, `Unknown Lens segment: ${segment}`);
  await (await displayed(`section[data-testid=memory-page] [role="tablist"] [role="tab"]:nth-child(${tabIndex})`)).click();
  await browser.waitUntil(async () => (await page.getAttribute('data-segment')) === segment, {
    timeout: 5_000, timeoutMsg: `Lens did not open ${segment}`,
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
  const reader = await displayed('[data-testid=map-view] article[aria-label="Source reader"]');
  await waitForText(reader, SOURCE.text);
  const id = await reader.getAttribute("data-source-id");
  assert.match(id ?? "", /^src_[0-9a-f]{32}$/);
  return id;
}

async function verifySource(id) {
  const library = await openLibrary("Sources");
  await waitForText(library, FIXTURE.source);
  const row = await displayed(`[data-testid=map-view] aside[data-testid=map-library] button[data-testid=source-library-row][data-source-id="${id}"]`);
  await waitForText(row, SOURCE.title);
  await row.click();
  const reader = await displayed('[data-testid=map-view] article[aria-label="Source reader"]');
  await waitForText(reader, SOURCE.text);
  assert.equal(await reader.getAttribute("data-source-id"), id, "The saved Source changed identity");
}

async function addPrinciple() {
  await openMainTab(browser, "chat");
  await openChat(FIXTURE.chat);
  const answer = await displayed("article.message.assistant[data-latest]");
  await clickButton("Suggest a correction", answer);
  await setField("[data-testid=answer-correction] textarea", PRINCIPLE.statement);
  await clickButton("Save correction", answer);
  await (await displayed("[data-feedback-principle]")).click();
  const form = await displayed("form[data-testid=principle-form]");
  await (await form.$('button[type="submit"]')).click();
  await waitGone(form, "The principle form did not close after saving");
  await verifyPrinciple();
}

async function verifyPrinciple() {
  const page = await openMemorySegment("pages");
  await waitForText(page, FIXTURE.method);
  const row = await visibleWithText("section[data-testid=memory-page] button[data-testid=memory-row]", PRINCIPLE.statement);
  await row.click();
  await waitForText(await displayed("section[data-testid=memory-page] aside[data-testid=memory-reader]"), PRINCIPLE.statement);
  const back = await browser.$("[data-testid=memory-reader-back]");
  if (await back.isDisplayed().catch(() => false)) await back.click();
  await (await visibleWithText("section[data-testid=memory-page] button[data-testid=memory-row]", FIXTURE.method)).click();
  const method = await displayed("section[data-testid=memory-page] aside[data-testid=memory-reader]");
  await waitForText(method, "Separate observations from assumptions.");
  await displayed("[data-testid=memory-reader] [data-testid=page-history]");
  const history = await displayed("[data-testid=memory-reader] [data-testid=lens-then-now]");
  assert.equal((await history.$$("h4")).length, 2, "The revised method did not show Then and Now");
}

/** Following: one search field over the selected Loupist's own followed list. */
const FOLLOWING = "[data-testid=following-page]";
// Followed by the name as typed: no listing search, so the suite sends nothing over the network.
const ALREADY_FOLLOWED = "Lamppost Test Works";
const TYPED = [
  { name: "Persistence Quarry Works" },
  { name: "Persistence Harbor Fund — Class B", reason: "Distributions covered every capital call so far." },
];
const NAME_ONLY = "Name only";
const NO_PRICE = '[data-price-note="none"]';
const typedPage = `[aria-label=${JSON.stringify(`${TYPED[1].name} asset page`)}]`;
const typedRow = (assetId) => `${FOLLOWING} li.following-row[data-row-key=${JSON.stringify(assetId)}]`;

/** Opens Following and returns the names of its rows once the list has been read. */
async function followedNames() {
  if (!(await browser.$(`${FOLLOWING} .following-input`).isDisplayed().catch(() => false))) await openMainTab(browser, "following");
  await displayed(`${FOLLOWING} .following-input`);
  await browser.waitUntil(async () => !(await browser.$(`${FOLLOWING} [data-following-loading]`).isExisting()), {
    timeout: 10_000, timeoutMsg: "Following did not finish reading what is followed",
  });
  return browser.execute((page) => [...document.querySelectorAll(`${page} li.following-row .following-open .following-name`)].map((name) => name.textContent), FOLLOWING);
}

/** Back from an asset page to the tab under it. */
async function leaveAssetPage(page) {
  await clickButton("Back", page);
  await waitGone(page, "The asset page did not close");
}

/**
 * Opens a fixture asset from its exact-name option, follows two new names as typed,
 * stops following the first, and adds a reason to the second's page.
 */
async function followTypedAssets() {
  await selectLoupist(FOLDER_LOUPIST);
  await followedNames();
  await setField(`${FOLLOWING} [role="combobox"]`, ALREADY_FOLLOWED.toLowerCase());
  const typedOpen = await displayed(`${FOLLOWING} [role="option"][data-kind="typed"][aria-label=${JSON.stringify(`Open ${ALREADY_FOLLOWED}, already followed`)}]`);
  await waitForText(typedOpen, "Already followed");
  await typedOpen.click();
  const fixturePage = await displayed(`[aria-label=${JSON.stringify(`${ALREADY_FOLLOWED} asset page`)}]`);
  await waitForText(fixturePage, "LMPST");
  await leaveAssetPage(fixturePage);

  for (const { name } of TYPED) {
    await setField(`${FOLLOWING} [role="combobox"]`, name);
    // Present before any search: typing sends nothing.
    await (await displayed(`${FOLLOWING} [role="option"][data-kind="typed"]`)).click();
    await browser.waitUntil(async () => (await browser.$(`${FOLLOWING} .following-input`).getValue()) === "" && (await followedNames()).includes(name), {
      timeout: 10_000, timeoutMsg: `${name} was not followed as typed`,
    });
  }
  // Each row has a visible Stop following; it acts at once, with no confirmation.
  await (await displayed(`${FOLLOWING} button[aria-label=${JSON.stringify(`Stop following ${TYPED[0].name}`)}]`)).click();
  await browser.waitUntil(async () => !(await followedNames()).includes(TYPED[0].name), { timeout: 10_000, timeoutMsg: `${TYPED[0].name} stayed in what you follow` });
  const assetId = await browser.execute((page, name) => [...document.querySelectorAll(`${page} li[data-row-key]`)]
    .find((row) => row.querySelector(".following-name")?.textContent === name)?.getAttribute("data-row-key") ?? null, FOLLOWING, TYPED[1].name);
  assert.match(assetId ?? "", /^ast_[0-9a-f]{16}$/, "Following did not list the asset followed as typed");
  await waitForText(await displayed(typedRow(assetId)), NAME_ONLY);
  await (await displayed(`${typedRow(assetId)} .following-open`)).click();
  const page = await displayed(typedPage);
  await displayed(`${typedPage} ${NO_PRICE}`);
  await clickButton("Add a reason", page);
  const reasonForm = await displayed(`${typedPage} form[aria-label="Add a reason"]`);
  await (await reasonForm.$("textarea")).setValue(TYPED[1].reason);
  await clickButton("Save entry", reasonForm);
  await waitGone(reasonForm, "The reason form did not close after saving");
  await waitForText(page, TYPED[1].reason);
  await leaveAssetPage(page);
  assert.equal((await followedNames()).filter(name => name === ALREADY_FOLLOWED).length, 1, "Opening a followed asset by name added a second follow");
  return assetId;
}

/**
 * After a restart Following lists the typed asset by name alone, the unfollowed one stays gone, and the
 * page keeps the reason and its no-price line. Follows are per Loupist, so the other Loupist follows
 * neither. A copied folder keeps the reason (the page's Journal entry) while the follow stays with the
 * original app's record of this Loupist.
 */
async function verifyTypedAsset(assetId, { copied }) {
  if (!copied) {
    await selectLoupist(NEW_LOUPIST);
    const other = await followedNames();
    assert.ok(!other.includes(TYPED[1].name) && !other.includes(ALREADY_FOLLOWED), "Another Loupist listed this Loupist's follows");
  }
  await selectLoupist(FOLDER_LOUPIST);
  const names = await followedNames();
  assert.ok(!names.includes(TYPED[0].name), "An asset that was unfollowed came back");
  assert.equal(names.includes(TYPED[1].name), !copied, copied ? "A follow came with the copied folder" : "The asset followed as typed was not listed after restart");
  if (copied) {
    await waitForText(await openLibrary("Journal"), TYPED[1].reason);
    await closeLibrary();
    return;
  }
  const row = await displayed(typedRow(assetId));
  await waitForText(row, NAME_ONLY);
  await (await row.$(".following-open")).click();
  const page = await displayed(typedPage);
  await waitForText(page, TYPED[1].reason);
  await displayed(`${typedPage} ${NO_PRICE}`);
  await leaveAssetPage(page);
}

async function addJournalEntry() {
  const library = await openLibrary("Journal");
  await clickButton("New entry", library);
  const form = await displayed("[data-testid=map-view] aside[data-testid=map-library] form[data-testid=journal-form]");
  const [judgment, reasoning] = await form.$$("textarea");
  await judgment.setValue(JOURNAL.judgment);
  await reasoning.setValue(JOURNAL.reasoning);
  await clickButton("Save entry", form);
  await waitGone(form, "The Journal form did not close after saving");
  const reader = await displayed("[data-testid=map-view] aside[data-testid=map-reader] article[data-testid=knowledge-journal]");
  await waitForText(reader, JOURNAL.judgment);
  await waitForText(reader, JOURNAL.reasoning);
  await openLibrary("Journal");
  const row = await visibleWithText("[data-testid=map-view] aside[data-testid=map-library] button[data-testid=journal-library-row]", JOURNAL.judgment);
  return row.getAttribute("data-research-id");
}

async function verifyJournal(id) {
  const library = await openLibrary("Journal");
  await waitForText(library, FIXTURE.journal);
  const row = await displayed(`[data-testid=map-view] aside[data-testid=map-library] button[data-testid=journal-library-row][data-research-id="${id}"]`);
  await waitForText(row, JOURNAL.judgment);
  await row.click();
  const reader = await displayed('[data-testid=map-view] aside[data-testid=map-reader] article[aria-label]');
  await waitForText(reader, JOURNAL.judgment);
  await waitForText(reader, JOURNAL.reasoning);
  await openLibrary("Journal");
  await (await visibleWithText("[data-testid=map-view] button[data-testid=journal-library-row]", FIXTURE.journal)).click();
  const frozen = await displayed("[data-testid=knowledge-journal]");
  await waitForText(frozen, "Metric will exceed the threshold");
  await clickButton("Read saved source", frozen);
  await waitForText(await displayed("[data-testid=knowledge-journal-source] [data-testid=knowledge-passage]"), "sealed-world-anchor sentence");
  await waitForText(await openLibrary("Notes"), FIXTURE.note);
}

async function replayMap() {
  await openMainTab(browser, "map");
  await closeLibrary();
  await closeMapReader();
  await clickControl(browser, "[data-testid=map-view] [data-map-more]");
  await (await displayed('[role="menuitemcheckbox"][data-map-replay]')).click();
  const bar = await displayed("[data-testid=map-replay-bar]");
  const play = await displayed("[data-map-play]");
  if (await play.getAttribute("aria-label") === "Play") await play.click();
  await browser.waitUntil(async () => (await play.getAttribute("aria-label")) === "Pause", { timeout: 5_000 });
  if (await play.getAttribute("aria-label") === "Pause") await play.click();
  await clickButton("4×", bar);
  assert.equal(await (await childWithText(bar, "button", "4×")).getAttribute("aria-pressed"), "true");
  const timeline = await displayed('input[aria-label="Replay timeline"]');
  await timeline.click();
  const before = await timeline.getValue();
  await browser.keys("ArrowRight");
  await browser.waitUntil(async () => (await timeline.getValue()) !== before, { timeout: 5_000, timeoutMsg: "Replay did not move when scrubbed" });
  await (await bar.$('button[aria-label="Exit replay"]')).click();
  await browser.waitUntil(() => browser.execute(() => document.querySelector("[data-testid=map-replay-bar]") === null), { timeout: 5_000, timeoutMsg: "Replay did not close" });
}

// Loupist and app settings ---------------------------------------------------------

async function saveInstructions() {
  await openMainTab(browser, "chat");
  await loupistSwitcherAction(browser, "settings");
  const field = "#loupist-profile-instructions";
  await setField(field, INSTRUCTIONS);
  await clickButton("Save", await displayed("[data-testid=loupist-profile-form]"));
  await browser.waitUntil(async () => (await (await browser.$(field)).getValue()) === INSTRUCTIONS
    && (await browser.$("[data-testid=loupist-profile-form] [role='status']").getText().catch(() => "")) === "Saved", {
    timeout: 10_000, timeoutMsg: "Loupist instructions did not save",
  });
}

async function verifyInstructions() {
  await openMainTab(browser, "chat");
  await loupistSwitcherAction(browser, "settings");
  await browser.waitUntil(async () => (await (await displayed("#loupist-profile-instructions")).getValue()) === INSTRUCTIONS, {
    timeout: 10_000, timeoutMsg: "Loupist instructions were not restored",
  });
}

async function settingsSection(id) {
  await openMainTab(browser, "settings");
  await (await displayed(`#settings-section-${id}`)).click();
  await displayed(`#settings-section-${id}[aria-current="page"]`);
}

async function usageRequests(accept = () => true) {
  await openMainTab(browser, "settings");
  await clickButton("Usage", await displayed("nav[aria-label='Settings sections']"));
  let requests;
  await browser.waitUntil(async () => {
    for (const status of await browser.$$("#main-panel-settings [role='status']")) {
      const text = await status.getText();
      const count = text.match(/^(\d[\d,]*) requests?$/u);
      const value = count ? Number(count[1].replaceAll(",", "")) : text === "No model usage in this period." ? 0 : undefined;
      if (value !== undefined && accept(value)) {
        requests = value;
        return true;
      }
    }
    return false;
  }, { timeout: 15_000, timeoutMsg: "Usage did not show the expected request total" });
  return requests;
}

function seedUsageRow(loupistId) {
  // This suite is offline on every host. Native smoke exercises the writer
  // with Pi; this fixture exercises persisted reads and folder portability.
  const db = new DatabaseSync(resolve(stateRoot, "app-data", "catalog.sqlite3"));
  try {
    db.exec("PRAGMA busy_timeout = 3000");
    const now = Date.now();
    const runId = "native-persistence-usage";
    const data = {
      kind: "chat", surface: "research", provider: "openai", credentialKind: "plan",
      model: "fixture-model", effort: null, chatId: null, runId, startedAtMs: now,
      durationMs: 0, status: "ok", failureKind: null, requests: 37,
      input: 200, cachedInput: 100, output: 30, reasoning: null,
      costMicrousd: null, costFromProvider: null,
    };
    db.prepare("INSERT INTO model_usage (id, finished_at_ms, loupist_id, data_json) VALUES (?, ?, ?, ?)")
      .run(`chat:${runId}:${now}`, now, loupistId, JSON.stringify(data));
  } finally {
    db.close();
  }
}

async function expand(selector) {
  const details = await displayed(selector);
  if ((await details.getAttribute("open")) === null) await (await details.$(":scope > summary")).click();
  await browser.waitUntil(async () => (await details.getAttribute("open")) !== null, { timeout: 5_000, timeoutMsg: `${selector} did not open` });
}

/** Chat defaults for the global defaults or the folder Loupist, with the file it edits. */
async function chatPreferences(scope) {
  if (scope === "global") await settingsSection("agent");
  else {
    await openMainTab(browser, "chat");
    await loupistSwitcherAction(browser, "settings");
    await expand('[data-loupist-section="chat"]');
  }
  await expand("[data-testid=agent-config-file-details]");
  const expected = scope === "global" ? globalConfig : projectConfig;
  let observed = "";
  await browser.waitUntil(async () => {
    observed = (await browser.$("[data-testid=agent-config-file] button[title]").getAttribute("title").catch(() => "")) ?? "";
    return Boolean(observed) && (await realFile(observed)) === expected;
  }, { timeout: 10_000, timeoutMsg: `Chat defaults did not switch to ${scope}` }).catch((error) => {
    throw new Error(`${error.message}; actual=${observed}; expected=${expected}`);
  });
  await expand("[data-testid=agent-config-advanced]");
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

/** An on/off setting is a switch: its state is `aria-checked`, not a value. */
async function configSwitch(path, expected) {
  let actual;
  await browser.waitUntil(async () => {
    actual = await (await displayed(`[id="config-${path}"][role="switch"]`)).getAttribute("aria-checked");
    return actual === String(expected);
  }, { timeout: 10_000, timeoutMsg: `${path} was ${actual}, expected ${expected}` }).catch((error) => {
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
  return displayed(`button[title="${label} theme"]`);
}

async function assertPressed(element, message) {
  await browser.waitUntil(async () => (await element.getAttribute("aria-pressed")) === "true", { timeout: 10_000, timeoutMsg: message });
}

const notificationDetails = '[role="switch"][aria-label="Show details in notifications"]';

async function verifyNotificationDetails(expected) {
  await settingsSection("notifications");
  await browser.waitUntil(async () => {
    const details = await displayed(notificationDetails);
    return (await details.isEnabled()) && (await details.getAttribute("aria-checked")) === String(expected);
  }, { timeout: 10_000, timeoutMsg: "Notification details did not match this app profile" });
}

async function seedSettings() {
  await chatPreferences("global");
  await saveRetries(RETRIES.global);
  assert.match(await tomlFile(globalConfig), /max_retries\s*=\s*5\b/, "Global retries did not reach config.toml");
  await chatPreferences(FOLDER_LOUPIST);
  await configValue("pi.retry.max_retries", RETRIES.global);
  assert.equal(await browser.$('button[aria-label="Reset Maximum retries"]').isExisting(), false, "An inherited value has no local reset");
  await configSwitch("pi.retry.enabled", true);
  await (await displayed('[id="config-pi.retry.enabled"][role="switch"]')).click();
  await configSwitch("pi.retry.enabled", false);
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
  // The display currency is a global appearance key; no rate is needed to save it.
  await (await displayed("[data-display-currency-select]")).selectByAttribute("value", "KRW");
  await browser.waitUntil(async () => /display_currency\s*=\s*"KRW"/.test(await tomlFile(globalConfig)), {
    timeout: 10_000, timeoutMsg: "The display currency did not reach the global config.toml",
  });
  await verifyNotificationDetails(true);
  await (await displayed(notificationDetails)).click();
  await verifyNotificationDetails(false);
}

/** The display currency Appearance shows once the global configuration has loaded. */
async function displayCurrency(expected) {
  let actual;
  await browser.waitUntil(async () => {
    const select = await displayed("[data-display-currency-select]");
    actual = (await select.isEnabled()) ? await select.getValue() : undefined;
    return actual === expected;
  }, { timeout: 10_000, timeoutMsg: `The display currency showed ${actual}, expected ${expected || "As listed"}` });
}

/** After a restart global values remain; in a copied folder only the Loupist's own values follow. */
async function verifySettings({ copied }) {
  const retries = copied ? RETRIES.appDefault : RETRIES.global;
  await chatPreferences("global");
  await configValue("pi.retry.max_retries", retries);
  await chatPreferences(FOLDER_LOUPIST);
  await configSwitch("pi.retry.enabled", false);
  await configValue("pi.retry.max_retries", retries);
  await assertPressed(await theme(copied ? "System" : "Dark"), `Theme after ${phase} did not match its owner`);
  // Kept across a restart; a folder copied to a fresh profile does not carry the old profile's choice.
  await displayCurrency(copied ? "" : "KRW");
  if (!copied) {
    const english = await childWithText(await displayed('#main-panel-settings [role="group"][aria-label="Language"]'), "button", "English");
    await assertPressed(english, "The interface language was not restored");
  }
  await verifyNotificationDetails(copied);
}

// Phases ----------------------------------------------------------------------------

/** `restored`: the app data is the seed's, so its saved language and theme apply; another profile only kept English. */
async function startShell({ fresh, restored = !fresh }) {
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
  if (!restored) return;
  await browser.waitUntil(async () => (await browser.$("[data-testid=app-shell]").getAttribute("data-theme")) === "dark", {
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
  await step("create a Chat from the Journal and rename it", async () => {
    await selectLoupist(FOLDER_LOUPIST);
    await renameChat(await createJournalChat(), CHAT);
  });
  await step("Bot stops the pre-update hand-off with its reason; hand-off sessions show Bot’s brief and the quoted request", () => verifyHandoffs(record));
  await step("Look into it on the retained link opens exactly one session", async () => {
    record.linkLookInto = await lookIntoSeededLink();
    report.linkLookInto = record.linkLookInto;
  });
  await step("save a Source from World Library", async () => {
    record.sourceId = await addSource();
  });
  await step("save a principle in Lens", addPrinciple);
  await step("follow two names as typed in Following, stop following one and save a reason on the other's page", async () => {
    record.typedAssetId = await followTypedAssets();
  });
  await step("save a Journal entry in the Library", async () => {
    record.journalId = await addJournalEntry();
    assert.ok(record.journalId, "The Journal entry has no identity");
  });
  await step("save Loupist instructions", saveInstructions);
  await step("save global and Loupist Chat defaults, the theme and the display currency", seedSettings);
  await step("record World with the new Journal entry", async () => {
    await selectLoupist(FOLDER_LOUPIST);
    record.mapStats = await settledMapStats();
  });
  await step("replay the saved World, pause, scrub, change speed and exit", replayMap);
  await browser.saveScreenshot(resolve(artifactRoot, "seed.png"));
  await step("read a nonempty Usage period from the catalog", async () => {
    const before = await usageRequests();
    seedUsageRow(record.folderLoupistId);
    await openMainTab(browser, "chat");
    record.usageRequests = await usageRequests(total => total === before + 37);
  });
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
    await loupistSwitcherAction(browser, "settings");
    assert.equal(await (await displayed("#loupist-profile-focus")).getValue(), "Dividend payers and payout safety");
    await (await displayed("[data-loupist-settings-back]")).click();
  });
  await step("saved and renamed Chats", async () => {
    await selectLoupist(FOLDER_LOUPIST);
    await verifyChats();
  });
  // The launch stops nothing again and starts nothing in its place.
  await step("the stopped hand-off, the hand-off sessions' first messages and the link's session after the launch", () => verifyHandoffs(record));
  await step("World keeps its World and Journal nodes", async () => {
    assert.equal(await settledMapStats(), record.mapStats);
  });
  await step("the Library keeps the Source", () => verifySource(record.sourceId));
  await step("Lens keeps the principle and learned method", verifyPrinciple);
  await step(copied ? "the copied folder keeps the typed asset's reason; its follow stays with the original app" : "the typed asset is listed by name in its own Loupist only and its page keeps the reason",
    () => verifyTypedAsset(record.typedAssetId, { copied }));
  await step("the Library keeps the Journal entry", () => verifyJournal(record.journalId));
  await step("Loupist instructions", verifyInstructions);
  await step(copied ? "Loupist preferences follow the folder; global settings stay with the app" : "global and Loupist preferences", () => verifySettings({ copied }));
  await step(copied ? "Usage stays with the original app" : "Usage request totals survive restart", async () => {
    assert.equal(await usageRequests(), copied ? 0 : record.usageRequests);
  });
  await browser.saveScreenshot(resolve(artifactRoot, `${phase}.png`));
}

/**
 * A second copy opened on its own app data, then put at the copy's registered path: the copy's app
 * reports the folder unavailable until the user reconnects it, and the folder's data comes back.
 */
// Reconnect (App.tsx and app/AppDialogs.tsx): another copy put at the registered path is a new folder
// identity, so the app reports the workspace unavailable until the user reconnects that path.
const UNAVAILABLE = 'section.app-selection-error[role="status"][aria-label="Workspace folder unavailable"]';
const RECONNECT_DIALOG = '[role="dialog"].loupist-workspace-dialog';
const CHOSEN_FOLDER = `${RECONNECT_DIALOG} p.workspace-path[aria-label="Selected workspace folder"]`;

/**
 * Chat › "Workspace folder unavailable" › Reconnect folder, then the dialog: Choose folder (the e2e
 * chooser answers with LOUPIST_E2E_WORKSPACE_DIR, this phase's folder), the chosen path, the trust
 * checkbox and Reconnect folder. Every control is a plain button or a native checkbox, so a click opens it.
 */
async function reconnectFolder() {
  await openMainTab(browser, "chat");
  const status = await displayed(UNAVAILABLE, 30_000);
  await clickButton("Reconnect folder", status);
  const dialog = await displayed(RECONNECT_DIALOG);
  await waitForText(dialog, "Reconnect workspace folder");
  await clickButton("Choose folder", dialog);
  const chosen = await displayed(CHOSEN_FOLDER);
  const expected = await realpath(resolve(stateRoot, "workspace"));
  await browser.waitUntil(async () => (await realpath((await chosen.getText()).trim()).catch(() => "")) === expected, {
    timeout: 10_000, timeoutMsg: `The chosen folder was not this phase's workspace (${expected})`,
  });
  const trust = await dialog.$('label.workspace-trust input[type="checkbox"]');
  if (!(await trust.isSelected())) await trust.click();
  await browser.waitUntil(() => trust.isSelected(), { timeout: 5_000, timeoutMsg: "I trust this folder. did not take" });
  await clickButton("Reconnect folder", dialog);
  await waitGone(dialog, "The reconnect dialog did not close");
  await browser.waitUntil(async () => !(await browser.$(UNAVAILABLE).isDisplayed().catch(() => false)), {
    timeout: 20_000, timeoutMsg: "The workspace stayed unavailable after reconnecting its folder",
  });
}

async function reconnected(record) {
  await step("the copy's app reconnects another copy of the folder put at its path", async () => {
    await startShell({ fresh: false, restored: false });
    await selectLoupist(FOLDER_LOUPIST);
    await reconnectFolder();
    await verifyTypedAsset(record.typedAssetId, { copied: true });
  });
  await browser.saveScreenshot(resolve(artifactRoot, `${phase}.png`));
}

try {
  if (phase === "seed") await seed();
  else if (phase === "reconnect") await reconnected(JSON.parse(await readFile(recordPath, "utf8")));
  else {
    const record = JSON.parse(await readFile(recordPath, "utf8"));
    await verify(record, { copied: phase === "copy" || phase === "fork" });
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
