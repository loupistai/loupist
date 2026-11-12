import assert from "node:assert/strict";

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
