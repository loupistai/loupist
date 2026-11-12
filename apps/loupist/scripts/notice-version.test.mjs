import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

test("the first-run notice acknowledged by React is the version persisted by Rust", () => {
  const renderer = readFileSync(new URL("../src/bridge/notice.ts", import.meta.url), "utf8");
  const host = readFileSync(new URL("../src-tauri/src/store/app_settings.rs", import.meta.url), "utf8");
  const rendererVersion = renderer.match(/export const CURRENT_NOTICE_VERSION = (\d+);/u);
  const hostVersion = host.match(/const CURRENT_NOTICE_VERSION: u32 = (\d+);/u);
  assert.ok(rendererVersion, "renderer notice version missing");
  assert.ok(hostVersion, "Rust notice version missing");
  assert.equal(Number(rendererVersion[1]), Number(hostVersion[1]), "the UI must acknowledge the notice version Rust requires");
});
