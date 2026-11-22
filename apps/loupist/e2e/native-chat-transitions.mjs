import assert from "node:assert/strict";
import koreanChat from "../src/i18n/locales/ko/chat.json" with { type: "json" };
import { setWindowSize } from "./window-geometry.mjs";

const waitFor = (browser, callback, arg) => browser.waitUntil(() => browser.execute(callback, arg), {
  timeout: 8_000, interval: 50,
});

const latest = ".message.assistant[data-latest]";
const activityButton = `button[aria-label=${JSON.stringify(koreanChat.bot.openActivity)}]`;

function measurePosition() {
  const thread = document.querySelector(".message-scroll");
  const composer = document.querySelector(".composer-shell");
  const pane = document.querySelector(".chat-reading-pane");
  const answer = thread.querySelector(".message.assistant[data-latest]");
  const footer = answer?.querySelector(".turn-footer")?.getBoundingClientRect();
  const lastLine = [...(answer?.querySelectorAll(".message-content :is(p,li,tr,pre)") ?? [])].at(-1)?.getBoundingClientRect();
  const edge = thread.getBoundingClientRect();
  const anchor = window.__chatLayoutAnchor;
  return {
    gap: thread.scrollHeight - thread.clientHeight - thread.scrollTop,
    anchor: anchor?.isConnected ? anchor.getBoundingClientRect().top : null,
    composerGap: pane.getBoundingClientRect().bottom - composer.getBoundingClientRect().bottom,
    editorHeight: composer.querySelector("textarea").getBoundingClientRect().height,
    actionsTop: footer?.top ?? null,
    actionsBottom: footer?.bottom ?? null,
    lastLineBottom: lastLine?.bottom ?? null,
    threadTop: edge.top,
    threadBottom: edge.bottom,
  };
}

async function readerScroll(browser, where) {
  await browser.execute(({ end, turn }) => {
    const thread = document.querySelector(".message-scroll");
    const anchor = thread.querySelectorAll("[data-turn]")[turn]?.querySelector(".message-content");
    window.__chatLayoutAnchor = end ? null : anchor;
    const target = end ? thread.scrollHeight
      : thread.scrollTop + anchor.getBoundingClientRect().top - thread.getBoundingClientRect().top - 6;
    thread.dispatchEvent(new WheelEvent("wheel", { deltaY: target - thread.scrollTop, bubbles: true }));
    thread.scrollTop = target;
    thread.dispatchEvent(new Event("scroll"));
  }, where);
  await browser.pause(120);
}

function assertPosition(current, before, { end = false, actions = false } = {}) {
  assert.ok(Math.abs(current.composerGap - before.composerGap) <= 1, `Composer bottom moved: ${JSON.stringify({ before, current })}`);
  assert.ok(Math.abs(current.editorHeight - before.editorHeight) <= 1, `Empty composer changed height: ${JSON.stringify({ before, current })}`);
  if (end) {
    assert.ok(Math.abs(current.gap) <= 2, `End of the thread left the view: ${JSON.stringify(current)}`);
    if (actions) {
      assert.ok(current.actionsTop !== null && current.actionsTop >= current.threadTop - 1 && current.actionsBottom <= current.threadBottom + 1,
        `Answer actions left the view: ${JSON.stringify(current)}`);
      assert.ok(current.lastLineBottom > current.threadTop && current.lastLineBottom <= current.threadBottom + 1,
        `The last answer line left the view: ${JSON.stringify(current)}`);
    }
  } else {
    assert.ok(before.anchor !== null && current.anchor !== null && Math.abs(current.anchor - before.anchor) <= 2,
      `The reading element moved: ${JSON.stringify({ before, current })}`);
  }
}

function measureTooltip(selector) {
  const button = document.querySelector(selector);
  const popup = document.querySelector('[data-slot="tooltip-content"][data-state$="open"]');
  if (!button || !popup || getComputedStyle(popup).visibility !== "visible") return null;
  const anchor = button.getBoundingClientRect();
  const tip = popup.getBoundingClientRect();
  const contentBottom = Math.max(...[...button.closest("article").querySelectorAll(".message-content")].map(node => node.getBoundingClientRect().bottom));
  return { below: tip.top - anchor.bottom, overlap: Math.min(anchor.right, tip.right) - Math.max(anchor.left, tip.left), aboveText: contentBottom - tip.top };
}

function assertTooltip(position) {
  assert.ok(position && position.below >= -1 && position.below <= 20 && position.overlap > 0 && position.aboveText <= 1,
    `Tooltip is away from its action or covers the answer: ${JSON.stringify(position)}`);
}

/** Runs in the owned native WebView, using deterministic preview data and the production renderer. */
export async function verifyChatTransitions(browser, { open, capture, cases }) {
  const record = async (name, measure = measurePosition, arg) => {
    const value = await browser.execute(measure, arg);
    const file = await capture(name);
    cases.push({ name, file, value });
    return value;
  };
  const placement = (value) => waitFor(browser,
    (value) => (document.querySelector("#activity-sheet")?.dataset.placement ?? "closed") === value, value);

  await setWindowSize(browser, { width: 1280, height: 820 });
  await open("bot-activity-layout", "ko");
  await readerScroll(browser, { turn: 1 });
  await readerScroll(browser, { end: true });
  let before = await record("bot-at-end");
  assertPosition(before, before, { end: true, actions: true });
  await browser.$(`${latest} ${activityButton}`).click();
  await placement("docked");
  assertPosition(await record("bot-at-end-docked"), before, { end: true, actions: true });
  await browser.$("[data-activity-close]").click();
  await placement("closed");
  assertPosition(await record("bot-at-end-undocked"), before, { end: true, actions: true });

  await readerScroll(browser, { turn: 1 });
  before = await record("bot-reading-history");
  assert.ok(before.anchor >= 0 && before.anchor < 100, `The anchor is not at the top: ${JSON.stringify(before)}`);
  const earlierActivity = `[data-turn]:nth-child(2) ${activityButton}`;
  await browser.$(earlierActivity).click();
  await placement("docked");
  assertPosition(await record("bot-reading-history-docked"), before);
  await browser.$('[data-activity-tab="using"]').click();
  await waitFor(browser, () => document.querySelector('[data-activity-tab="using"]')?.getAttribute("aria-selected") === "true");
  assertPosition(await record("bot-reading-history-using"), before);
  await setWindowSize(browser, { width: 800, height: 820 });
  await placement("overlay");
  assertPosition(await record("bot-reading-history-overlay"), before);
  await setWindowSize(browser, { width: 1280, height: 820 });
  await placement("docked");
  await browser.$("[data-activity-close]").click();
  await placement("closed");
  assertPosition(await record("bot-reading-history-undocked"), before);

  await readerScroll(browser, { end: true });
  await browser.execute((selector) => {
    const opener = document.querySelector(selector);
    opener.focus({ preventScroll: true });
    opener.click();
  }, `${latest} ${activityButton}`);
  await placement("docked");
  await readerScroll(browser, { turn: 1 });
  before = await browser.execute(measurePosition);
  await browser.execute(() => {
    const close = document.querySelector("[data-activity-close]");
    close.focus({ preventScroll: true });
    close.click();
  });
  await placement("closed");
  assertPosition(await record("bot-closes-away-from-the-opener"), before);
  assert.equal(await browser.execute(() => document.activeElement?.matches("[data-header-activity]")), true);

  for (const end of [true, false]) {
    await setWindowSize(browser, { width: 1150, height: 780 });
    await open("chat-completion-layout", "ko");
    await browser.execute(() => document.querySelector("[data-header-activity]").click());
    await placement("docked");
    await readerScroll(browser, end ? { end } : { turn: 1 });
    const name = end ? "research-following" : "research-reading-history";
    before = await record(`${name}-streaming`);
    assertPosition(before, before, { end });

    const advance = (event) => browser.execute(event => window.dispatchEvent(new Event(event)), event);
    await advance("preview:save-layout-answer");
    await waitFor(browser, () => !!document.querySelector('[data-turn-latest] .turn-footer[data-state="saving"]'));
    assertPosition(await record(`${name}-saving`), before, { end });
    await advance("preview:complete-layout-answer");
    await waitFor(browser, () => !!document.querySelector('[data-message-id="layout-complete"] .turn-footer[data-state="settled"]')
      && !!document.querySelector('[data-message-id="layout-complete"] .number-chip')
      && !!document.querySelector('[data-message-id="layout-complete"] table'));
    assertPosition(await record(`${name}-complete`), before, { end, actions: end });
    await advance("preview:read-layout-chart");
    await waitFor(browser, () => !!document.querySelector('[data-message-id="layout-complete"] .artifact-chart-shell'));
    assertPosition(await record(`${name}-chart-loaded`), before, { end, actions: end });
  }

  for (const locale of ["ko", "en"]) for (const nearTop of [false, true]) {
    await setWindowSize(browser, { width: 1120, height: 780 });
    await open(nearTop ? "bot-quick-answer" : "bot-activity-layout", locale);
    const correct = `${latest} .answer-feedback button[aria-expanded]`;
    const actions = [correct, `${latest} .turn-footer .message-action-bar > .message-actions button`,
      `${latest} .answer-feedback button[aria-pressed]:nth-of-type(1)`, `${latest} .answer-feedback button[aria-pressed]:nth-of-type(2)`];
    for (const [index, action] of actions.entries()) {
      await browser.execute((selector) => document.querySelector(selector).focus({ preventScroll: true }), action);
      await browser.waitUntil(async () => Boolean(await browser.execute(measureTooltip, action)), { timeout: 5_000, interval: 50 });
      assertTooltip(await record(`answer-tooltip-${locale}-${nearTop ? "near-top" : "at-end"}-${index}`, measureTooltip, action));
    }
    await setWindowSize(browser, { width: 880, height: 780 });
    const resized = await record(`answer-tooltip-${locale}-${nearTop ? "near-top" : "at-end"}-resized`, measureTooltip, actions.at(-1));
    if (resized) assertTooltip(resized);
  }
}
