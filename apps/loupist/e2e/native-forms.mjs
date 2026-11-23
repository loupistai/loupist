import assert from "node:assert/strict";

export class ComposerPreviewUnresponsive extends Error {}

export async function openComposerPreview(browser, url) {
  // The old WebView document can still answer requests after navigation starts.
  await browser.execute(() => { document.documentElement.dataset.previewReady = "stale"; });
  await browser.url(url);
  await browser.waitUntil(() => browser.execute((expected) =>
    location.href === expected && document.documentElement?.dataset.previewReady === "true", url),
  { timeout: 60_000, interval: 250, timeoutMsg: `The composer preview did not become ready: ${url}` });

  let timer;
  try {
    // waitUntil alone cannot bound a WebDriver request that never receives a script result.
    const response = await Promise.race([
      browser.execute(() => 42),
      new Promise((_, reject) => { timer = setTimeout(() => reject(new Error("Script deadline exceeded")), 2_000); }),
    ]);
    assert.equal(response, 42);
  } catch (cause) {
    throw new ComposerPreviewUnresponsive(`Composer preview unresponsive after ready (2 s script deadline): ${url}`, { cause });
  } finally {
    clearTimeout(timer);
  }
}

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

/**
 * Page callback: the composer's message box against its text. The textarea's own
 * text is transparent; the highlight layer under it is what the reader sees, so
 * both are read. Shared by WKWebView and headless Chrome.
 */
export function composerMessageLayout() {
  const textarea = document.querySelector('textarea[data-chat-composer="true"]');
  if (!textarea) return null;
  const style = getComputedStyle(textarea);
  const highlight = textarea.parentElement?.querySelector(".composer-prompt-highlight");
  const shell = textarea.closest(".composer-shell")?.getBoundingClientRect();
  const computedMax = parseFloat(style.maxHeight);
  return {
    height: textarea.offsetHeight,
    clientHeight: textarea.clientHeight,
    scrollHeight: textarea.scrollHeight,
    scrollTop: textarea.scrollTop,
    minHeight: parseFloat(style.minHeight),
    // composer.css caps the message at min(200px, 30dvh).
    maxHeight: Number.isFinite(computedMax) ? computedMax : Math.min(200, innerHeight * 0.3),
    lineHeight: parseFloat(style.lineHeight),
    caretAtEnd: textarea.selectionStart === textarea.value.length,
    highlight: highlight ? { height: highlight.offsetHeight, scrollTop: highlight.scrollTop } : null,
    shellTop: shell?.top ?? null,
    shellBottom: shell?.bottom ?? null,
    viewportHeight: innerHeight,
  };
}

/**
 * Types into the open composer: two lines, then more until it reaches its cap.
 * Up to the cap every line shows (the box is as tall as its text); past it the
 * box holds at the cap and scrolls with the caret's line in view. WKWebView has
 * no `field-sizing: content`, so this is a WebView measurement, not a preview PNG.
 */
export async function assertComposerGrows(browser, { onCapped } = {}) {
  const textarea = await browser.$('textarea[data-chat-composer="true"]');
  await textarea.waitForDisplayed({ timeout: 10_000 });
  await textarea.click();
  const read = () => browser.execute(composerMessageLayout);
  // WebDriver's synthesized Shift+Enter and Backspace do not run WKWebView's default editing
  // actions, so the line break and the clear go through the browser's own editing commands,
  // which fire the same input events as typing.
  const edit = (command, text) => browser.execute((name, value) => {
    const field = document.querySelector('textarea[data-chat-composer="true"]');
    field?.focus();
    if (name === "delete") field?.select();
    return document.execCommand(name, false, value);
  }, command, text ?? null);
  const lineBreak = async () => assert.ok(await edit("insertText", "\n"), "The composer refused a line break");
  const shows = (layout) => layout.scrollHeight <= layout.clientHeight + 1;
  const near = (a, b) => Math.abs(a - b) <= 1.5;
  const empty = await read();
  assert.ok(empty && near(empty.height, empty.minHeight), `An empty composer is one line high: ${JSON.stringify(empty)}`);
  await textarea.addValue("Compare the last four quarters of margins");
  await lineBreak();
  await textarea.addValue("and say which guidance changed");
  let layout = await read();
  assert.ok(shows(layout) && layout.height >= empty.height + layout.lineHeight - 1,
    `Two lines must both show: ${JSON.stringify(layout)}`);
  const samples = [layout];
  for (let line = 3; line <= 24 && shows(layout); line += 1) {
    await lineBreak();
    await textarea.addValue(`Line ${line}: cash flow, capital spending and the dividend against the prior year`);
    layout = await read();
    samples.push(layout);
    if (shows(layout)) {
      assert.ok(layout.height <= layout.maxHeight + 1.5, `The message grew past its cap: ${JSON.stringify(layout)}`);
      assert.ok(layout.highlight && near(layout.highlight.height, layout.height), `The visible text layer is clipped: ${JSON.stringify(layout)}`);
    }
  }
  assert.ok(!shows(layout), `The message never reached its cap: ${JSON.stringify(samples)}`);
  assert.ok(near(layout.height, layout.maxHeight), `Past its cap the message holds at the cap: ${JSON.stringify(layout)}`);
  assert.ok(layout.caretAtEnd && layout.scrollTop + layout.clientHeight >= layout.scrollHeight - layout.lineHeight / 2,
    `The caret's line must stay in view: ${JSON.stringify(layout)}`);
  assert.ok(layout.highlight && near(layout.highlight.scrollTop, layout.scrollTop), `The visible text does not follow the scroll: ${JSON.stringify(layout)}`);
  assert.ok(layout.shellBottom <= layout.viewportHeight + 1 && layout.shellTop >= 0, `The composer left the window: ${JSON.stringify(layout)}`);
  await onCapped?.(layout);
  assert.ok(await edit("delete"), "The composer refused to clear");
  const cleared = await read();
  assert.ok(near(cleared.height, cleared.minHeight), `A cleared composer returns to one line: ${JSON.stringify(cleared)}`);
  return samples;
}

/**
 * Page callback: the open New Loupist dialog's preview card against its scrolling form.
 * At the minimum window the template cards show their titles alone, so the preview card
 * stands whole above the fold (sharing.css) instead of a sliver at the fade.
 */
export function newLoupistPreviewLayout() {
  const body = document.querySelector('[role="dialog"] [data-slot="dialog-body"]:has(.loupist-identity-preview)');
  const preview = body?.querySelector(".loupist-identity-preview");
  if (!body || !preview) return null;
  const view = body.getBoundingClientRect();
  const card = preview.getBoundingClientRect();
  return {
    viewportHeight: innerHeight, scrollTop: body.scrollTop, fade: parseFloat(getComputedStyle(body).paddingBottom) || 0,
    bodyTop: view.top, bodyBottom: view.bottom, cardTop: card.top, cardBottom: card.bottom,
  };
}

export async function assertNewLoupistPreviewShows(browser) {
  const layout = await browser.execute(newLoupistPreviewLayout);
  assert.ok(layout, "The New Loupist dialog has no preview card");
  assert.ok(layout.scrollTop === 0 && layout.cardTop >= layout.bodyTop - 1 && layout.cardBottom <= layout.bodyBottom - layout.fade + 1,
    `The preview card must show whole before the form scrolls: ${JSON.stringify(layout)}`);
  return layout;
}

/**
 * ⌘A in the composer: on macOS the Edit menu's Select All (selectAll:) is what the key reaches, so the
 * native menu must carry it enabled; WebKit's own SelectAll command then selects the whole message, and
 * the selection must paint its text. The composer's glyphs are transparent over its highlight layer, so a
 * transparent selected text once left a selection that read as greyed-out text and nothing selected.
 */
export async function assertComposerSelectAll(browser, { mac = process.platform === "darwin" } = {}) {
  if (mac) {
    const menu = await browser.execute(() => window.__TAURI_INTERNALS__.invoke("e2e_native_menu_snapshot"));
    const edit = menu.find((group) => group.items?.some((item) => item.action === "selectAll:"));
    assert.ok(edit?.enabled, `The app menu has no enabled Edit menu with Select All: ${JSON.stringify(menu.map((group) => group.title))}`);
    const selectAll = edit.items.find((item) => item.action === "selectAll:");
    assert.ok(selectAll.enabled, "Edit › Select All is disabled");
    for (const action of ["undo:", "redo:", "cut:", "copy:", "paste:"]) {
      assert.ok(edit.items.some((item) => item.action === action && item.enabled), `Edit has no enabled ${action}`);
    }
  }
  const textarea = await browser.$('textarea[data-chat-composer="true"]');
  await textarea.waitForDisplayed({ timeout: 10_000 });
  await textarea.click();
  await textarea.addValue("Select every word of this message");
  const state = await browser.execute(() => {
    const field = document.querySelector('textarea[data-chat-composer="true"]');
    field.focus();
    field.setSelectionRange(field.value.length, field.value.length);
    const ran = document.execCommand("selectAll");
    const selection = getComputedStyle(field, "::selection");
    const alpha = (color) => {
      const match = /rgba?\(([^)]+)\)/.exec(color);
      if (!match) return 1;
      const parts = match[1].split(/[,\s/]+/).filter(Boolean);
      return parts.length > 3 ? Number(parts[3]) : 1;
    };
    return {
      ran, start: field.selectionStart, end: field.selectionEnd, length: field.value.length,
      glyphs: selection.webkitTextFillColor, glyphAlpha: alpha(selection.webkitTextFillColor),
      background: selection.backgroundColor, sameAsGlyphs: selection.backgroundColor === selection.webkitTextFillColor,
    };
  });
  assert.ok(state.ran && state.start === 0 && state.end === state.length && state.length > 0, `Select All did not select the whole message: ${JSON.stringify(state)}`);
  assert.ok(state.glyphAlpha > 0.5 && !state.sameAsGlyphs, `A composer selection must paint its text over a distinct colour: ${JSON.stringify(state)}`);
  await browser.execute(() => {
    const field = document.querySelector('textarea[data-chat-composer="true"]');
    field.focus();
    field.select();
    document.execCommand("delete");
  });
  return state;
}
