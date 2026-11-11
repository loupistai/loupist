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
