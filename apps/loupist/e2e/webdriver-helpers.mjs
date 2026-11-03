/**
 * Shared WebdriverIO helpers for the native E2E scripts. Each script creates
 * one helper set per browser session with its own timeout profile.
 */
export function createWebdriverHelpers(browser, options = {}) {
  const {
    defaultTimeout = 10_000,
    bodyTextLimit = 16_000,
    interval = 100,
  } = options;

  const bodyText = async () =>
    (await browser.$("body").getText()).slice(0, bodyTextLimit);

  async function displayed(selector, timeout = defaultTimeout) {
    await browser.waitUntil(
      async () => {
        for (const element of await browser.$$(selector)) {
          if (await element.isDisplayed()) return true;
        }
        return false;
      },
      {
        timeout,
        interval,
        timeoutMsg: `Timed out waiting for visible ${selector}\n${await bodyText()}`,
      },
    );
    for (const element of await browser.$$(selector)) {
      if (await element.isDisplayed()) return element;
    }
    throw new Error(`Visible element disappeared: ${selector}`);
  }

  // Ghost add actions read "+ Add source"; the leading plus is decoration, not part of the name.
  const visibleLabel = async (element) => (await element.getText()).trim().replace(/^\+\s*/u, "");

  // React may replace a node between finding it and reading it; a detached node is
  // simply not a match this round.
  const detached = (error) => /no longer attached|stale element/i.test(String(error?.message ?? error));
  const matches = async (element, text) => {
    try {
      return (await element.isDisplayed()) && (await visibleLabel(element)) === text;
    } catch (error) {
      if (detached(error)) return false;
      throw error;
    }
  };

  async function childWithText(root, selector, text, timeout = defaultTimeout) {
    await browser.waitUntil(
      async () => {
        for (const element of await root.$$(selector)) {
          if (await matches(element, text)) return true;
        }
        return false;
      },
      {
        timeout,
        interval,
        timeoutMsg: `Timed out waiting for ${selector} with text “${text}”\n${await bodyText()}`,
      },
    );
    for (const element of await root.$$(selector)) {
      if (await matches(element, text)) return element;
    }
    throw new Error(`Element disappeared: ${selector} with text “${text}”`);
  }

  async function clickButton(text, root = browser) {
    // One retry when the button re-renders between lookup and click.
    for (let attempt = 0; ; attempt += 1) {
      const button = await childWithText(root, "button", text);
      try {
        await button.click();
        return button;
      } catch (error) {
        if (attempt > 0 || !detached(error)) throw error;
      }
    }
  }

  async function waitForText(root, text, timeout = defaultTimeout) {
    await browser.waitUntil(async () => (await root.getText()).includes(text), {
      timeout,
      interval,
      timeoutMsg: `Timed out waiting for text “${text}”\n${await bodyText()}`,
    });
  }

  async function waitForTextGone(root, text, timeout = defaultTimeout) {
    await browser.waitUntil(
      async () => !(await root.getText()).includes(text),
      {
        timeout,
        interval,
        timeoutMsg: `Timed out waiting for text to disappear: “${text}”`,
      },
    );
  }

  async function waitForAppReady(timeout = 90_000) {
    await browser.waitUntil(
      async () => !(await bodyText()).includes("Opening"),
      {
        timeout,
        interval,
        timeoutMsg: `Timed out waiting for Loupist to finish opening\n${await bodyText()}`,
      },
    );
  }

  return {
    bodyText,
    displayed,
    childWithText,
    clickButton,
    waitForText,
    waitForTextGone,
    waitForAppReady,
  };
}
