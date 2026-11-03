export const desktopWindowSize = Object.freeze({ width: 1280, height: 820 });
export const minimumWindowSize = Object.freeze({ width: 700, height: 560 });

export async function readWindowGeometry(browser) {
  const [window, viewport] = await Promise.all([
    browser.getWindowSize(),
    browser.execute(() => ({ width: window.innerWidth, height: window.innerHeight })),
  ]);
  return { window, viewport, units: "logical CSS pixels" };
}

async function waitForWindowSize(browser, requested, timeout) {
  let geometry;
  try {
    await browser.waitUntil(async () => {
      geometry = await readWindowGeometry(browser);
      const { window, viewport } = geometry;
      // WebDriver measures the outer window. Allow native frame decorations,
      // but reject a stale WebView or physical-pixel size on a scaled display.
      return Math.abs(window.width - requested.width) <= 1
        && Math.abs(window.height - requested.height) <= 1
        && viewport.width >= requested.width - 32 && viewport.width <= requested.width + 1
        && viewport.height >= requested.height - 60 && viewport.height <= requested.height + 1;
    }, { timeout, interval: 100, timeoutMsg: "Window and CSS viewport did not settle at the requested size" });
  } catch (error) {
    throw new Error(
      `Native window size could not be verified: ${JSON.stringify({ requested, ...geometry })}. Check the display work area and WebDriver logical-pixel support.`,
      { cause: error },
    );
  }
  return geometry;
}

export async function setWindowSize(browser, requested = desktopWindowSize, { timeout = 5_000 } = {}) {
  await browser.setWindowSize(requested.width, requested.height);
  // Native resize can complete before the WebView applies its CSS viewport.
  await browser.pause(250);
  return waitForWindowSize(browser, requested, timeout);
}

export async function withWindowSize(browser, requested, action) {
  const original = await browser.getWindowRect();
  try {
    const geometry = await setWindowSize(browser, requested);
    return await action(geometry);
  } finally {
    await browser.setWindowRect(original.x, original.y, original.width, original.height);
    await browser.pause(250);
    await waitForWindowSize(browser, original, 5_000);
  }
}
