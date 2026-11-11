import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const helper = join(dirname(fileURLToPath(import.meta.url)), "detach-launch.mjs");

test("detached launcher stays alive after the parent helper exits", async (t) => {
  const directory = mkdtempSync(join(tmpdir(), "loupist-detach-"));
  t.after(() => {
    rmSync(directory, { recursive: true, force: true });
  });
  const marker = join(directory, "pid");
  const log = join(directory, "log");
  const script = join(directory, "child.sh");
  writeFileSync(
    script,
    `#!/bin/sh
sleep 0.2
printf '%s %s %s\\n' "$$" "$PPID" "$1" > "${marker}"
while true; do sleep 1; done
`,
    { mode: 0o700 },
  );

  const childPid = Number(
    execFileSync(process.execPath, [helper, script, log, "named preview"], {
      encoding: "utf8",
    }).trim(),
  );
  assert.ok(Number.isInteger(childPid) && childPid > 0);
  t.after(async () => {
    try {
      process.kill(process.platform === "win32" ? childPid : -childPid, "SIGTERM");
    } catch (error) {
      if (error.code !== "ESRCH") throw error;
    }
    const deadline = Date.now() + 5_000;
    while (Date.now() < deadline) {
      try { process.kill(childPid, 0); }
      catch (error) { if (error.code === "ESRCH") return; throw error; }
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
    assert.fail("Detached test child did not exit after SIGTERM");
  });

  let launchedPid;
  const deadline = Date.now() + 5_000;
  while (Date.now() < deadline) {
    try {
      const [pid] = readFileSync(marker, "utf8").trim().split(/\s+/);
      launchedPid = Number(pid);
      if (Number.isInteger(launchedPid) && launchedPid > 0) {
        break;
      }
    } catch {
      // The child has not written the marker yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  assert.equal(launchedPid, childPid);
  assert.ok(readFileSync(marker, "utf8").endsWith(" named preview\n"));
  // CI can adopt an orphan under a runner-owned subreaper rather than PID 1.
  // The helper has already exited because execFileSync returned; survival is
  // the portable detached-launch contract.
  process.kill(childPid, 0);
});

async function shellResult(script, env, { bypassLease = false } = {}) {
  const child = spawn("/bin/sh", bypassLease
    ? ["-c", 'export LOUPIST_GUI_LEASE_ENTRY="$$:$1"; exec "$1"', "fixture", script]
    : [script], { env: { ...process.env, ...env }, stdio: ["ignore", "pipe", "pipe"], detached: true });
  let output = "";
  child.stdout.on("data", (part) => { output += part; });
  child.stderr.on("data", (part) => { output += part; });
  const timer = setTimeout(() => { try { process.kill(-child.pid, "SIGTERM"); } catch { /* exited */ } }, 5_000);
  try {
    const code = await new Promise((resolve) => child.once("exit", resolve));
    return { code, output };
  } finally { clearTimeout(timer); }
}

async function waitForExit(pid) {
  const deadline = Date.now() + 3_000;
  while (Date.now() < deadline) {
    try { process.kill(pid, 0); }
    catch (error) { if (error.code === "ESRCH") return; throw error; }
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  assert.fail(`Fixture launcher ${pid} survived startup failure`);
}

test("up forwards its startup timeout and stops its detached launcher on failure", async (t) => {
  const directory = mkdtempSync(join(tmpdir(), "loupist-up-timeout-"));
  const launcherMarker = join(directory, "launcher-pid");
  let launcherPid;
  t.after(() => {
    if (launcherPid) { try { process.kill(-launcherPid, "SIGTERM"); } catch { /* exited */ } }
    rmSync(directory, { recursive: true, force: true });
  });
  for (const name of ["up.sh", "detach-launch.mjs"]) copyFileSync(new URL(name, import.meta.url), join(directory, name));
  writeFileSync(join(directory, "run-dev-app.sh"), `#!/bin/sh
printf '%s' "$$" > "${launcherMarker}"
while true; do sleep 1; done
`, { mode: 0o700 });
  writeFileSync(join(directory, "wait-session.mjs"), `
import { existsSync } from 'node:fs';
const args = process.argv.slice(2);
if (!args.includes('--pid')) process.exit(1);
const timeout = args[args.indexOf('--timeout-ms') + 1];
console.error('forwarded-timeout=' + timeout);
// Full-suite contention can delay the detached shell beyond the simulated timeout.
// Exercise cleanup only after the owned fixture has actually started.
const deadline = Date.now() + 3000;
while (!existsSync(${JSON.stringify(launcherMarker)})) {
  if (Date.now() >= deadline) throw new Error('Fixture launcher did not start');
  await new Promise((resolve) => setTimeout(resolve, 10));
}
setTimeout(() => process.exit(1), Number(timeout));
`);
  const result = await shellResult(join(directory, "up.sh"), { LOUPIST_E2E_STARTUP_TIMEOUT_MS: "123" });
  assert(existsSync(launcherMarker), result.output);
  launcherPid = Number(readFileSync(launcherMarker, "utf8"));
  assert.equal(result.code, 1, result.output);
  await waitForExit(launcherPid);
  launcherPid = null;
  assert.match(result.output, /forwarded-timeout=123/);
});

test("development launcher bounds startup and cleans up its owned CLI stand-in", async (t) => {
  const directory = mkdtempSync(join(tmpdir(), "loupist-dev-timeout-"));
  const e2e = join(directory, "e2e");
  const marker = join(directory, "cli-pid");
  let cliPid;
  t.after(() => {
    if (cliPid) { try { process.kill(cliPid, "SIGTERM"); } catch { /* exited */ } }
    rmSync(directory, { recursive: true, force: true });
  });
  mkdirSync(e2e);
  mkdirSync(join(directory, "node_modules/@tauri-apps/cli"), { recursive: true });
  for (const name of ["run-dev-app.sh", "wait-session.mjs", "wait-session-lib.mjs"]) copyFileSync(new URL(name, import.meta.url), join(e2e, name));
  writeFileSync(join(directory, "node_modules/@tauri-apps/cli/tauri.js"), `
require('node:fs').writeFileSync(${JSON.stringify(marker)}, String(process.pid));
setInterval(() => {}, 1000);
`);
  const result = await shellResult(join(e2e, "run-dev-app.sh"), {
    LOUPIST_E2E_STARTUP_TIMEOUT_MS: "200", TAURI_WEBDRIVER_PORT: "14397",
  }, { bypassLease: true });
  cliPid = Number(readFileSync(marker, "utf8"));
  assert.equal(result.code, 1, result.output);
  assert.match(result.output, /within 200 milliseconds/);
  await waitForExit(cliPid);
  cliPid = null;
});
