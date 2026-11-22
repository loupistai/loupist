import assert from "node:assert/strict";
import { test } from "node:test";
import { ComposerPreviewUnresponsive, openComposerPreview } from "./native-forms.mjs";

const url = "http://127.0.0.1:5000/preview.html?scenario=composer-compact&composerProbe=minimum";

test("composer preview ignores a ready marker from the previous URL", async () => {
  let expectsUrl;
  let stale = false;
  const browser = {
    async execute(script, expected) {
      if (expected) {
        expectsUrl = expected;
        // A previous ready marker survives navigation in a different document.
        const oldDocument = { documentElement: { dataset: { previewReady: "true" } } };
        const check = new Function("document", "location", "expected", `return (${script})(expected)`);
        assert.equal(check(oldDocument, { href: `${url}x` }, expected), false);
        return check(oldDocument, { href: url }, expected);
      }
      if (!stale) { stale = true; return; }
      return script();
    },
    async url(requested) {
      assert.ok(stale);
      assert.equal(requested, url);
    },
    async waitUntil(check) { assert.equal(await check(), true); },
  };
  await openComposerPreview(browser, url);
  assert.equal(expectsUrl, url);
});

test("composer preview reports a post-ready hang after two seconds", async ({ mock }) => {
  mock.timers.enable({ apis: ["setTimeout"] });
  let beganPing;
  const pingStarted = new Promise((resolve) => { beganPing = resolve; });
  const browser = {
    async execute(_script, expected) {
      if (expected) return true;
      if (!this.navigated) return;
      beganPing();
      return new Promise(() => {});
    },
    async url() { this.navigated = true; },
    async waitUntil(check) { assert.equal(await check(), true); },
  };
  const opening = openComposerPreview(browser, url);
  const rejection = assert.rejects(opening, (error) => {
    assert.ok(error instanceof ComposerPreviewUnresponsive);
    assert.equal(error.message, `Composer preview unresponsive after ready (2 s script deadline): ${url}`);
    return true;
  });
  await pingStarted;
  mock.timers.tick(2_000);
  await rejection;
});
