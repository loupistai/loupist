import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

test("the bilingual Beta notice passes catalog parity with Apple as a company name", () => {
  const script = fileURLToPath(new URL("./check-i18n-parity.mjs", import.meta.url));
  assert.match(execFileSync(process.execPath, [script], { encoding: "utf8" }), /i18n parity: .* — OK/);
});
