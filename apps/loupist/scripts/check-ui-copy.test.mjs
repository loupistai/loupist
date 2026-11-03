import test from "node:test";
import assert from "node:assert/strict";
import { inspectCatalog, validateAllowlist } from "./check-ui-copy.mjs";

const inspect = (value, locale = "en", entries = []) => inspectCatalog("catalog.json", JSON.stringify(value, null, 2), locale, "app", entries);

test("every English and Korean advice phrase is an error, case-insensitively for English", () => {
  const english = ["you should buy", "you should sell", "you should hold", "we recommend", "our recommendation", "buy now", "sell now", "guaranteed return", "guaranteed returns"];
  const korean = ["매수하세요", "매도하세요", "추천 종목", "수익 보장", "꼭 사세요", "꼭 파세요"];
  for (const [locale, values] of [["en", english], ["ko", korean]]) {
    for (const phrase of values) {
      const [hit] = inspect({ nested: { text: phrase.toUpperCase() } }, locale);
      assert.equal(hit.rule, "ui-copy/advice");
      assert.equal(hit.key, `${locale}.app.nested.text`);
      assert.equal(hit.allowed, false);
      assert.equal(hit.line, 3);
    }
  }
});

test("jargon is matched in values, including arrays, but not key names or larger words", () => {
  const terms = ["revision id", "digest", "receipt", "envelope", "byte offset", "span selector", "worker", "sidecar", "stdio", "JSON-RPC"];
  assert.equal(inspect({ values: terms.map((term) => term.toUpperCase()) }).length, terms.length);
  assert.deepEqual(inspect({ worker: "workership and receipts are outside the exact pattern", digest: "Research overview" }), []);
  assert.equal(inspect({ text: "worker worker" }).length, 2);
});

test("justified allowlists suppress only an exact locale, namespace, key and jargon term", () => {
  const entries = [{ key: "en.app.label", term: "WORKER", reason: "Fixture technical label" }];
  assert.equal(inspect({ label: "Worker" }, "en", entries)[0].allowed, true);
  assert.equal(inspect({ other: "Worker" }, "en", entries)[0].allowed, false);
  assert.equal(inspect({ label: "Worker" }, "ko", entries)[0].allowed, false);
  const hits = inspect({ label: "Worker: we recommend" }, "en", entries);
  assert.equal(hits.find((hit) => hit.rule === "ui-copy/advice").allowed, false);
  assert.throws(() => validateAllowlist([{ key: "en.app.label", term: "we recommend", reason: "No bypass" }]));
  assert.throws(() => validateAllowlist([{ key: "en.app.label", term: "worker", reason: " " }]));
  assert.throws(() => inspectCatalog("broken.json", "{broken}", "en", "app"));
});
