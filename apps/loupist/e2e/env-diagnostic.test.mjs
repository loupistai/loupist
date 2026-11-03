import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, symlinkSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path, { join } from "node:path";
import test from "node:test";
import {
  developmentPiAgentDir,
  inspectDirectory,
  liveStateDir,
} from "./env-diagnostic.mjs";

test("development Pi directory stays under the debug app-data child", () => {
  assert.equal(
    developmentPiAgentDir({
      platform: "darwin",
      home: "/Users/example",
    }),
    "/Users/example/.loupist/development/pi",
  );
  assert.equal(
    developmentPiAgentDir({
      platform: "win32",
      home: "C:\\Users\\example",
    }),
    path.win32.join(
      "C:\\Users\\example",
      ".loupist",
      "development",
      "pi",
    ),
  );
  assert.equal(
    developmentPiAgentDir({
      platform: "linux",
      home: "/home/example",
    }),
    "/home/example/.loupist/development/pi",
  );
});

test("directory inspection reports presence without listing contents", () => {
  const root = mkdtempSync(join(tmpdir(), "loupist-pi-"));
  try {
    const present = join(root, "pi");
    const link = join(root, "link");
    const file = join(root, "file");
    mkdirSync(present);
    writeFileSync(join(present, "auth.json"), "secret");
    symlinkSync(present, link);
    writeFileSync(file, "x");
    assert.equal(inspectDirectory(present), "present");
    assert.equal(inspectDirectory(link), "symlink");
    assert.equal(inspectDirectory(file), "not-directory");
    assert.equal(inspectDirectory(join(root, "missing")), "missing");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("live profiles stay under artifacts/live-profiles", () => {
  assert.equal(liveStateDir("/repo/e2e/artifacts", "qa-example"), join("/repo/e2e/artifacts", "live-profiles", "qa-example"));
  assert.throws(() => liveStateDir("/repo/e2e/artifacts", "../development"), /Live profile/);
});
