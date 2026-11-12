import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { checkInstalled, dependencyIssues } from "./check-installed-dependencies.mjs";

const packages = { "": { name: "app" }, "node_modules/required": { version: "1", integrity: "sha512-one", resolved: "https://example.test/one.tgz" }, "node_modules/platform-only": { version: "2", optional: true } };
const locked = { packages };
const installed = { packages: { "node_modules/required": { ...packages["node_modules/required"] } } };

test("installed inventory accepts omitted platform packages and rejects stale or missing dependencies", () => {
  assert.deepEqual(dependencyIssues(locked, installed), []);
  for (const field of ["version", "resolved", "integrity"]) {
    const stale = structuredClone(installed);
    stale.packages["node_modules/required"][field] = "changed";
    assert.deepEqual(dependencyIssues(locked, stale), ["stale node_modules/required"]);
  }
  assert.deepEqual(dependencyIssues(locked, { packages: {} }), ["missing node_modules/required"]);
  assert.deepEqual(dependencyIssues(locked, { packages: { ...installed.packages, "node_modules/retired": { version: "1" } } }), ["stale node_modules/retired"]);
  assert.deepEqual(dependencyIssues({}, installed), ["missing package inventory"]);
});

test("a hidden lock cannot keep an absent installed directory passing", (t) => {
  const root = mkdtempSync(join(tmpdir(), "loupist-dependencies-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, "node_modules/required"), { recursive: true });
  writeFileSync(join(root, "package-lock.json"), JSON.stringify(locked));
  writeFileSync(join(root, "node_modules/.package-lock.json"), JSON.stringify(installed));
  assert.doesNotThrow(() => checkInstalled(root));
  rmSync(join(root, "node_modules/required"), { recursive: true });
  assert.throws(() => checkInstalled(root), /missing node_modules\/required/);
});
