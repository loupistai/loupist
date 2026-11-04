import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");

test("development manifests and locks agree with the CI/release Node LTS pin", () => {
  const pin = read("../../../.nvmrc").trim();
  assert.match(pin, /^\d+\.\d+\.\d+$/);
  const major = Number(pin.split(".")[0]);
  const range = `>=${pin} <${major + 1}`;
  for (const directory of ["../", "../e2e/"]) {
    assert.equal(JSON.parse(read(`${directory}package.json`)).engines.node, range);
    assert.equal(JSON.parse(read(`${directory}package-lock.json`)).packages[""].engines.node, range);
  }
});
