import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { copyFile, mkdir, mkdtemp, readFile, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { allocateFreePort, processIdentity } from "./wait-session-lib.mjs";

const root = await realpath(await mkdtemp(join(tmpdir(), "loupist-tauri-launcher-")));
let owner;
let orphan;
async function result(child) {
  let stderr = "";
  child.stderr.on("data", chunk => { stderr += chunk; });
  const timeout = setTimeout(() => child.kill("SIGTERM"), 15_000);
  try {
    const [code, signal] = await once(child, "exit");
    return { code, signal, stderr };
  } finally {
    clearTimeout(timeout);
  }
}
try {
  for (const dir of ["scripts", "e2e", "node_modules/@tauri-apps/cli"]) {
    await mkdir(join(root, dir), { recursive: true });
  }
  await copyFile(new URL("../scripts/tauri.mjs", import.meta.url), join(root, "scripts/tauri.mjs"));
  await copyFile(new URL("./wait-session-lib.mjs", import.meta.url), join(root, "e2e/wait-session-lib.mjs"));
  // Adapt only the lock location; run the production flock/exec implementation.
  await writeFile(join(root, "e2e/native_ui_lease.py"), `
import importlib.util, pathlib, sys
spec = importlib.util.spec_from_file_location("lease", ${JSON.stringify(fileURLToPath(new URL("./native_ui_lease.py", import.meta.url)))})
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.run(sys.argv[1:], pathlib.Path(${JSON.stringify(join(root, "native-ui.lock"))}))
`);
  const evidence = join(root, "dev-args.json");
  await writeFile(join(root, "node_modules/@tauri-apps/cli/tauri.js"), `
const fs = require('node:fs');
if (process.argv[2] === 'build') process.exit(23);
fs.writeFileSync(${JSON.stringify(evidence)}, JSON.stringify(process.argv.slice(2)));
fs.writeFileSync(${JSON.stringify(join(root, "cli-pid"))}, String(process.pid));
console.log('fixture-ready');
setInterval(() => {}, 1000);
`);
  const port = await allocateFreePort();
  const launch = args => spawn(process.execPath, [join(root, "scripts/tauri.mjs"), ...args], {
    env: { ...process.env, TAURI_WEBDRIVER_PORT: String(port), PYTHONDONTWRITEBYTECODE: "1" },
    stdio: ["ignore", "pipe", "pipe"],
  });
  owner = launch(["dev", "--no-watch"]);
  const ownerResult = result(owner);
  await Promise.race([
    once(owner.stdout, "data"),
    ownerResult.then(value => { throw new Error(`owner exited before ready: ${JSON.stringify(value)}`); }),
  ]);
  assert.deepEqual(JSON.parse(await readFile(evidence, "utf8")), ["dev", "--no-watch", "--features", "webdriver",
    ...(process.platform === "darwin" ? ["--", "--config", join(root, "e2e/macos-dev-runner.toml")] : []),
  ]);
  const receipt = join(root, "e2e/artifacts/current-session.json");
  const before = await readFile(receipt, "utf8");
  const blocked = await result(launch(["dev"]));
  assert.equal(blocked.code, 1, blocked.stderr);
  assert.match(blocked.stderr, /owns the display/);
  assert.equal(await readFile(receipt, "utf8"), before);

  // Build does not consume the GUI lease and retains the real CLI exit code.
  assert.equal((await result(launch(["build"]))).code, 23);
  owner.kill("SIGTERM");
  assert.equal((await ownerResult).signal, "SIGTERM");
  owner = null;
  await assert.rejects(readFile(receipt), { code: "ENOENT" });

  // SIGKILL bypasses the wrapper's traps. The surviving CLI must keep the GUI
  // lease until it exits; a receipt alone is not a lock or proof of ownership.
  owner = launch(["dev"]);
  const killedOwnerResult = result(owner);
  await Promise.race([
    once(owner.stdout, "data"),
    killedOwnerResult.then(value => { throw new Error(`owner exited before ready: ${JSON.stringify(value)}`); }),
  ]);
  orphan = Number(await readFile(join(root, "cli-pid"), "utf8"));
  const guardedOwner = JSON.parse(await readFile(receipt, "utf8")).launcherPid;
  process.kill(guardedOwner, "SIGKILL");
  assert.equal((await killedOwnerResult).signal, "SIGKILL");
  owner = null;
  assert.notEqual(processIdentity(orphan), null, "the CLI stand-in survives the wrapper's SIGKILL");
  const blockedByChild = await result(launch(["dev"]));
  assert.equal(blockedByChild.code, 1, blockedByChild.stderr);
  assert.match(blockedByChild.stderr, /owns the display/);
  process.kill(orphan, "SIGTERM");
  const deadline = Date.now() + 3000;
  while (processIdentity(orphan) !== null && Date.now() < deadline) {
    await new Promise(resolve => setTimeout(resolve, 20));
  }
  assert.equal(processIdentity(orphan), null);
  orphan = null;

  owner = launch(["dev"]);
  const recoveredResult = result(owner);
  await Promise.race([
    once(owner.stdout, "data"),
    recoveredResult.then(value => { throw new Error(`recovery failed: ${JSON.stringify(value)}`); }),
  ]);
  owner.kill("SIGTERM");
  assert.equal((await recoveredResult).signal, "SIGTERM");
  owner = null;
} finally {
  owner?.kill("SIGTERM");
  if (orphan) { try { process.kill(orphan, "SIGTERM"); } catch { /* exited */ } }
  await rm(root, { recursive: true, force: true });
}
console.log("npm Tauri dev lease, receipt protection, signal cleanup and build passthrough passed");
