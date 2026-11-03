import assert from "node:assert/strict";
import test from "node:test";
import { JSDOM } from "jsdom";
import { markdownIsRendered } from "./capture-readiness.mjs";

test("waits for the renderer even when its readable fallback has no Markdown markers", () => {
  const dom = new JSDOM('<article class="message"><div class="message-content"><p class="message-response-fallback">- What we know: margin fell.</p></div></article>');
  const root = dom.window.document;
  assert.equal(markdownIsRendered(root), false);
  root.querySelector(".message-content").innerHTML = "<ul><li><strong>What we know:</strong> margin fell.</li></ul>";
  assert.equal(markdownIsRendered(root), true);
  dom.window.close();
});

test("also waits for lazy Markdown outside a Chat answer, while non-Markdown views are ready", () => {
  const dom = new JSDOM('<section class="reader"><p class="message-response-fallback">Saved source text</p></section>');
  const root = dom.window.document;
  assert.equal(markdownIsRendered(root), false);
  root.querySelector(".reader").innerHTML = "<p>Saved source text</p>";
  assert.equal(markdownIsRendered(root), true);
  root.body.innerHTML = '<main><button>Start chatting</button></main>';
  assert.equal(markdownIsRendered(root), true);
  dom.window.close();
});
