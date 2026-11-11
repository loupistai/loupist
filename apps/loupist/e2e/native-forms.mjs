import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";

/**
 * Read actual legend geometry. Text presence alone misses a collapsed heading.
 * WKWebView shrinks a grid or flex fieldset legend to about one character;
 * Chrome does not. A design-preview PNG is not this measurement.
 */
export function formLegendLayout(selector = "fieldset") {
  const context = document.createElement("canvas").getContext("2d");
  return [...document.querySelectorAll(selector)].flatMap((fieldset) => {
    const legend = fieldset.querySelector(":scope > legend");
    if (!legend) return [];
    const style = getComputedStyle(legend);
    const box = legend.getBoundingClientRect();
    if (!box.width || !box.height || style.visibility === "hidden"
      || style.clip === "rect(0px, 0px, 0px, 0px)" || style.clipPath === "inset(50%)") return [];
    const parent = fieldset.getBoundingClientRect();
    const parentStyle = getComputedStyle(fieldset);
    const availableWidth = fieldset.clientWidth - parseFloat(parentStyle.paddingLeft) - parseFloat(parentStyle.paddingRight);
    const text = legend.textContent.trim();
    context.font = `${style.fontStyle} ${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
    const letterSpacing = parseFloat(style.letterSpacing) || 0;
    const textWidth = context.measureText(text).width + letterSpacing * Math.max(0, [...text].length - 1);
    return [{
      text,
      width: box.width,
      height: box.height,
      availableWidth,
      textWidth,
      requiredWidth: Math.min(availableWidth, textWidth),
      withinField: box.left >= parent.left - 1 && box.right <= parent.right + 1,
    }];
  });
}

export async function assertFormLegendsFit(browser, { selector = "fieldset", minimum = 1 } = {}) {
  await browser.waitUntil(() => browser.execute(() => document.fonts.status === "loaded"), {
    timeout: 10_000, interval: 100, timeoutMsg: "Form fonts did not load",
  });
  const legends = await browser.execute(formLegendLayout, selector);
  assert.ok(legends.length >= minimum, `Expected ${minimum} visible form headings, found ${legends.length}`);
  for (const legend of legends) {
    assert.ok(legend.withinField, `Form heading overflows its field: ${JSON.stringify(legend)}`);
    assert.ok(legend.width + 2 >= legend.requiredWidth,
      `Form heading collapsed instead of using the available width: ${JSON.stringify(legend)}`);
  }
  return legends;
}

/** Reproduce the former DOM/CSS in the actual WebKit, then reload the fixed component. */
export async function verifyNativeToneLayoutFix(browser, devUrl, { beforePath, afterPath } = {}) {
  const target = new URL("preview.html", `${String(devUrl).replace(/\/$/, "")}/`);
  target.search = new URLSearchParams({ scenario: "onboarding-loupist-tone", locale: "ko", theme: "dark" }).toString();
  const ready = async () => {
    // A new URL prevents the previous document's ready marker from satisfying
    // the wait while WKWebView is still scheduling its navigation.
    target.searchParams.set("probe", randomUUID());
    await browser.url(target.href);
    await browser.waitUntil(() => browser.execute((expected) =>
      location.href === expected && document.documentElement.dataset.previewReady === "true", target.href), {
      timeout: 30_000, interval: 100, timeoutMsg: "Native tone preview did not become ready",
    });
    await browser.execute(() => {
      const style = document.createElement("style");
      style.textContent = "*,*::before,*::after{animation-duration:0s!important;transition-duration:0s!important;scroll-behavior:auto!important}";
      document.head.append(style);
    });
    await browser.waitUntil(() => browser.execute(() => {
      const field = document.querySelector('main[data-onboarding="4"] [data-loupist-tone]');
      return field && field.getBoundingClientRect().height > 0;
    }), { timeout: 5_000, interval: 100 });
    await browser.pause(250);
  };
  await ready();
  const fixedBefore = await assertFormLegendsFit(browser, { selector: "[data-loupist-tone]" });
  let before;
  try {
    await browser.execute(() => {
      const field = document.querySelector("[data-loupist-tone]");
      const options = field.querySelector("[data-testid=loupist-tone-options]");
      while (options.firstChild) field.insertBefore(options.firstChild, options);
      options.remove();
      field.style.display = "grid";
      field.style.gap = "var(--gg-space-2)";
      const legend = field.querySelector("legend");
      legend.style.width = "auto";
      legend.style.lineHeight = "normal";
      legend.style.overflowWrap = "normal";
      legend.scrollIntoView({ block: "center", behavior: "instant" });
    });
    before = await browser.execute(formLegendLayout, "[data-loupist-tone]");
    if (beforePath) await browser.saveScreenshot(beforePath);
  } finally {
    await ready();
  }
  const after = await assertFormLegendsFit(browser, { selector: "[data-loupist-tone]" });
  if (afterPath) await browser.saveScreenshot(afterPath);
  const reproduced = before.some((legend) => legend.width + 2 < legend.requiredWidth);
  assert.ok(reproduced, `The old layout did not reproduce a collapsed heading in this WebKit: ${JSON.stringify(before)}`);
  return { reproduced, before, after, fixedBefore };
}
