import test from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { copyFile, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

test("the isolated launcher passes Vite's bound URL to Tauri and closes its listener", { timeout: 30_000 }, async (t) => {
  const root = await mkdtemp(join(tmpdir(), "loupist-run-app-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  for (const path of ["e2e", "src-tauri", "node_modules/@tauri-apps/cli"]) await mkdir(join(root, path), { recursive: true });
  await symlink(fileURLToPath(new URL("../node_modules/vite", import.meta.url)), join(root, "node_modules/vite"), "dir");
  for (const name of ["run-app.sh", "dev-server.mjs"]) await copyFile(new URL(name, import.meta.url), join(root, "e2e", name));
  await writeFile(join(root, "package.json"), '{"type":"module"}');
  await writeFile(join(root, "index.html"), "<p>isolated Vite fixture</p>");
  await writeFile(join(root, "src-tauri/tauri.e2e.conf.json"), '{}');
  const captured = join(root, "captured-session.json");
  // Replace only the native boundary; execute the real shell and Vite helper.
  await writeFile(join(root, "e2e/wait-session.mjs"), `
import { writeFileSync } from "node:fs";
const args = process.argv.slice(2);
const value = flag => args[args.indexOf(flag) + 1];
if (args[0] === "--resolve-port") console.log(args[1]);
if (args.includes("--write-session")) writeFileSync(${JSON.stringify(captured)}, JSON.stringify({ devUrl: value("--dev-url") }));
`);
  const evidence = join(root, "native-boundary.json");
  await writeFile(join(root, "node_modules/@tauri-apps/cli/tauri.js"), `
import { writeFileSync } from "node:fs";
import assert from "node:assert/strict";
const args = process.argv.slice(2);
const config = JSON.parse(args[args.indexOf("--config") + 1]);
assert.equal(config.build.beforeDevCommand, "");
assert.match(await (await fetch(config.build.devUrl)).text(), /isolated Vite fixture/);
writeFileSync(${JSON.stringify(evidence)}, JSON.stringify(config.build));
`);
  const script = join(root, "e2e/run-app.sh");
  const child = spawn("/bin/sh", ["-c", 'export LOUPIST_GUI_LEASE_ENTRY="$$:$1"; exec "$1"', "fixture", script], {
    env: { ...process.env, LOUPIST_E2E_STATE_DIR: join(root, "state"),
      LOUPIST_E2E_DEV_PORT: "0", LOUPIST_E2E_PORT: "", LOUPIST_E2E_REUSE_DEV_SERVER: "",
      LOUPIST_LIVE_PI_AGENT_DATA_DIR: "", LOUPIST_E2E_PICK_FILE: "", LOUPIST_E2E_WORKSPACE_DIR: "" },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let output = "";
  child.stdout.on("data", data => { output += data; });
  child.stderr.on("data", data => { output += data; });
  t.after(() => { if (child.exitCode === null) child.kill("SIGTERM"); });
  assert.equal((await once(child, "close"))[0], 0, output);
  const build = JSON.parse(await readFile(evidence, "utf8"));
  assert.equal(JSON.parse(await readFile(captured, "utf8")).devUrl, build.devUrl);
  assert.notEqual(new URL(build.devUrl).port, "1420");
  await assert.rejects(fetch(build.devUrl), /fetch failed/);
});
