/**
 * Work-progress dump shared by the agent driver and the native finance eval adapter.
 * Rows come from the shipped DOM (`data-progress-*`), not a reimplemented oracle.
 */

async function isVisible(element) {
  try {
    return await element.isDisplayed();
  } catch {
    return false;
  }
}

async function revealAndExpand(button, { reveal = false } = {}) {
  if (!(await button.isExisting())) return;
  if (reveal) {
    try {
      await button.scrollIntoView();
    } catch {
      // Attribute dump still works if the row is already in the DOM.
    }
  }
  if (!(await isVisible(button))) return;
  if ((await button.getAttribute("aria-expanded")) !== "true") {
    await button.click();
  }
}

async function expandProgress(section, { reveal = false } = {}) {
  await revealAndExpand(await section.$("button[data-testid=chat-progress-toggle]"), { reveal });
}

async function progressRows(section) {
  const rows = [];
  for (const row of await section.$$("[data-testid=chat-progress-row]")) {
    const target = await row.$("[data-testid=chat-progress-target]");
    rows.push({
      category: await row.getAttribute("data-progress-category"),
      operation: await row.getAttribute("data-progress-operation"),
      status: await row.getAttribute("data-progress-status"),
      action: (await (await row.$("[data-testid=chat-progress-action]")).getText()).trim(),
      target: (await target.isExisting())
        ? (await target.getText()).trim() || null
        : null,
    });
  }
  return rows;
}

/**
 * The activity sheet lists each reply's steps (`data-progress-*` on every step line).
 * It is opened per reply through its own event, every collapsed kind is expanded,
 * and it is closed again so the page is left as it was.
 */
async function sheetProgress(browser, { latestOnly }) {
  const ids = await browser.execute((latest) => {
    const done = [...document.querySelectorAll("article.message.assistant[data-message-id]")]
      .filter((article) => ["complete", "aborted", "error"].some((state) => article.classList.contains(state)))
      .map((article) => article.dataset.messageId);
    return latest ? done.slice(-1) : done;
  }, latestOnly);
  const progress = [];
  for (const messageId of ids) {
    await browser.execute((id) => window.dispatchEvent(new CustomEvent("loupist:open-activity", { detail: { messageId: id, tab: "steps" } })), messageId);
    const sheet = await browser.$("[data-testid=activity-sheet]");
    await sheet.waitForExist({ timeout: 10_000 });
    for (const button of await sheet.$$("[data-activity-expand][aria-expanded=false]")) await button.click();
    const heading = await sheet.$('[data-activity-section="steps"] h3');
    const rows = [];
    for (const row of await sheet.$$('[data-activity-section="steps"] [data-testid=activity-step]')) {
      const target = await row.$("[data-testid=activity-step-target]");
      rows.push({
        category: await row.getAttribute("data-progress-category"),
        operation: await row.getAttribute("data-progress-operation"),
        status: await row.getAttribute("data-progress-status"),
        action: await row.getAttribute("data-progress-action"),
        target: (await target.isExisting()) ? (await target.getText()).trim() || null : null,
      });
    }
    progress.push({ heading: (await heading.isExisting()) ? (await heading.getText()).trim() : null, rows });
    await browser.execute(() => document.querySelector("[data-activity-close]")?.click());
  }
  return progress;
}

export async function collectWorkProgress(
  browser,
  { latestOnly = false, requireVisible = false } = {},
) {
  const selector = '[data-testid=chat-progress][data-turn-item="tools"]';
  const sections = latestOnly
    ? await (async () => {
        const messages = await browser.$$(
          [
            "article.message.assistant.complete",
            "article.message.assistant.aborted",
            "article.message.assistant.error",
          ].join(", "),
        );
        const latest = messages.at(-1);
        if (!latest) return [];
        return await latest.$$(selector);
      })()
    : await browser.$$(selector);
  // Once the thread no longer draws its steps inline, they are read from the activity sheet.
  if (!sections.length) return await sheetProgress(browser, { latestOnly });
  const progress = [];
  for (const section of sections) {
    if (requireVisible && !(await isVisible(section))) continue;
    if (requireVisible) {
      try {
        await section.scrollIntoView();
      } catch {
        // Off-screen progress still has attributes once expanded.
      }
    }
    await expandProgress(section, { reveal: requireVisible });
    const heading = await section.$("[data-testid=chat-progress-heading]");
    progress.push({
      heading: (await heading.isExisting())
        ? (await heading.getText()).trim()
        : null,
      rows: await progressRows(section),
    });
  }
  return progress;
}
