import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { once } from "node:events";
import { chmod, mkdir, mkdtemp, readdir, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { processIdentity } from "./wait-session-lib.mjs";

const modulePath = fileURLToPath(new URL("./native_ui_lease.py", import.meta.url));
const directory = await mkdtemp(join(tmpdir(), "loupist-ui-lease-"));
const lockPath = join(directory, "native-ui.lock");
const bootstrap = `
import importlib.util, pathlib, sys
spec = importlib.util.spec_from_file_location("lease", sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.run(sys.argv[3:], pathlib.Path(sys.argv[2]))
`;
const python = execFileSync("/bin/sh", ["-c", "command -v python3"], { encoding: "utf8" }).trim();
// A set display keeps these cases off Xvfb; the headless cases below unset it.
const launch = (command, path = lockPath, environment = {}) => spawn(python, ["-B", "-c", bootstrap, modulePath, path, ...command], {
  stdio: ["ignore", "pipe", "pipe"],
  env: { ...process.env, DISPLAY: process.env.DISPLAY || ":lease-test", ...environment },
});
const captured = async (child) => {
  let stdout = "";
  child.stdout.on("data", chunk => { stdout += chunk; });
  return { ...(await result(child)), stdout };
};
async function result(child) {
  let stderr = "";
  child.stderr.on("data", chunk => { stderr += chunk; });
  const [code, signal] = await once(child, "exit");
  return { code, signal, stderr };
}
let owner;
let orphan;
try {
  // Real process contention, with an isolated lock. No window or fixed port.
  owner = launch(["/bin/sh", "-c", "echo ready; exec sleep 30"]);
  await once(owner.stdout, "data");
  const blocked = await result(launch(["/usr/bin/true"]));
  assert.equal(blocked.code, 1);
  assert.match(blocked.stderr, /owns the display/);

  // Kernel releases the lease after cancellation, despite the receipt file.
  owner.kill("SIGKILL");
  await once(owner, "exit");
  owner = null;
  const recovered = await result(launch(["/usr/bin/true"]));
  assert.equal(recovered.code, 0, recovered.stderr);

  // A suite's child reuses the inherited lock, including across exec.
  const nested = launch(["python3", "-B", "-c", bootstrap, modulePath, lockPath, "/usr/bin/true"]);
  const nestedResult = await result(nested);
  assert.equal(nestedResult.code, 0, nestedResult.stderr);

  const link = join(directory, "symlink.lock");
  await symlink(lockPath, link);
  assert.equal((await result(launch(["/usr/bin/true"], link))).code, 1,
    "a lock path cannot follow a symlink");

  // Exercise actual launcher reentry without opening an app. Invalid input
  // must fail once and leave no temporary state behind.
  const scratch = join(directory, "scratch");
  await mkdir(scratch);
  const smoke = fileURLToPath(new URL("./run-native-smoke.sh", import.meta.url));
  const invalid = await result(launch([smoke, "--invalid"], lockPath, { TMPDIR: scratch }));
  assert.equal(invalid.code, 1, invalid.stderr);
  assert.match(invalid.stderr, /usage:/);
  assert.deepEqual(await readdir(scratch), []);

  // Mirror run-app.sh's shell -> env -i -> Node CLI boundary. Sanitizing the
  // environment must preserve the open lease descriptor, even if the shell dies.
  const cli = join(directory, "raw-cli.mjs");
  await writeFile(cli, `
import { fstatSync } from 'node:fs';
if (!fstatSync(Number(process.argv[2])).isFile()) process.exit(1);
console.log(process.pid);
setInterval(() => {}, 1000);
`);
  owner = launch(["/bin/sh", "-c", '/usr/bin/env -i "$1" "$2" "$LOUPIST_GUI_LEASE_FD" & wait', "fixture", process.execPath, cli]);
  const rawOwnerResult = result(owner);
  const [output] = await Promise.race([
    once(owner.stdout, "data"),
    rawOwnerResult.then(value => { throw new Error(`raw CLI failed: ${JSON.stringify(value)}`); }),
  ]);
  orphan = Number(output.toString().trim());
  owner.kill("SIGKILL");
  assert.equal((await rawOwnerResult).signal, "SIGKILL");
  owner = null;
  const rawBlocked = await result(launch(["/usr/bin/true"]));
  assert.equal(rawBlocked.code, 1, rawBlocked.stderr);
  assert.match(rawBlocked.stderr, /owns the display/);
  process.kill(orphan, "SIGTERM");
  const deadline = Date.now() + 3000;
  while (processIdentity(orphan) !== null && Date.now() < deadline) {
    await new Promise(resolve => setTimeout(resolve, 20));
  }
  assert.equal(processIdentity(orphan), null);
  orphan = null;
  assert.equal((await result(launch(["/usr/bin/true"]))).code, 0);

  if (process.platform === "linux") {
    const bin = join(directory, "bin");
    await mkdir(bin);
    const xvfbPid = join(directory, "xvfb.pid");
    const headless = { DISPLAY: "", WAYLAND_DISPLAY: "", PATH: bin };
    const printDisplay = ["/bin/sh", "-c", 'echo "$DISPLAY $XAUTHORITY"'];
    const missing = await result(launch(printDisplay, lockPath, headless));
    assert.equal(missing.code, 1);
    assert.match(missing.stderr, /Xvfb or xauth is missing/);

    await writeFile(join(bin, "xauth"), "#!/bin/sh\nexit 0\n");
    // Announce the display on -displayfd like Xvfb, then stay up until signalled.
    await writeFile(join(bin, "Xvfb"), `#!/bin/sh
echo $$ > ${JSON.stringify(xvfbPid)}
display=\${1#:}
while [ $# -gt 0 ]; do [ "$1" = -displayfd ] && fd=$2; shift; done
eval "echo $display >&$fd"
exec /bin/sleep 30
`);
    await chmod(join(bin, "xauth"), 0o755);
    await chmod(join(bin, "Xvfb"), 0o755);
    const wrapped = await captured(launch(printDisplay, lockPath, headless));
    assert.equal(wrapped.code, 0, wrapped.stderr);
    assert.match(wrapped.stdout.trim(), /^:\d+ \S+\/xvfb-\d+\.auth$/, "a headless Linux launch gets a private display");
    const server = Number((await readFile(xvfbPid, "utf8")).trim());
    const stopBy = Date.now() + 3000;
    while (processIdentity(server) !== null && Date.now() < stopBy) {
      await new Promise(resolve => setTimeout(resolve, 20));
    }
    assert.equal(processIdentity(server), null, "Xvfb exits with its launcher");
    const shown = await captured(launch(["/bin/sh", "-c", 'echo "$DISPLAY"'], lockPath, { ...headless, DISPLAY: ":5" }));
    assert.equal(shown.stdout.trim(), ":5", "an existing display is used as is");
  }
} finally {
  owner?.kill("SIGKILL");
  if (orphan) { try { process.kill(orphan, "SIGTERM"); } catch { /* exited */ } }
  await rm(directory, { recursive: true, force: true });
}
console.log("native GUI contention, cancellation, inheritance, symlink rejection and headless display passed");
