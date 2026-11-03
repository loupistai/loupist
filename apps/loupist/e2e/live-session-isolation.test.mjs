import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = await mkdtemp(join(tmpdir(), "loupist-live-isolation-"));
try {
  const script = join(root, "run-native-live-journeys.sh");
  await copyFile(new URL("./run-native-live-journeys.sh", import.meta.url), script);
  await mkdir(join(root, "artifacts"));
  await mkdir(join(root, "fixtures/journeys"), { recursive: true });
  await writeFile(join(root, "fixtures/journeys/holdings.csv"), "symbol,market\n");
  const sharedReceipt = join(root, "artifacts/current-session.json");
  await writeFile(sharedReceipt, "shared window must survive");
  // Stand-ins at the native boundary: this exercises the actual live runner's
  // environment, receipt and failure cleanup without a window or paid call.
  await writeFile(join(root, "run-app.sh"), `#!/bin/sh
set -eu
printf '%s' '{"fixture":true}' > "$LOUPIST_E2E_SESSION_INFO"
exec sleep 30
`, { mode: 0o700 });
  await writeFile(join(root, "wait-session.mjs"), `
import { access } from 'node:fs/promises';
const file = process.argv[process.argv.indexOf('--session') + 1];
const until = Date.now() + 2000;
while (true) {
  try { await access(file); break; } catch {
    if (Date.now() >= until) process.exit(1);
    await new Promise(resolve => setTimeout(resolve, 20));
  }
}
`);
  await writeFile(join(root, "native-live-journeys.mjs"), `
import { readFile, writeFile } from 'node:fs/promises';
await writeFile(${JSON.stringify(join(root, "observed.json"))}, JSON.stringify({
  sessionPath: process.argv[2], content: await readFile(process.argv[2], 'utf8')
}));
process.exit(19); // controlled scenario failure must not become a pass
`);
  // The GUI lease has its own process tests; skip its entry wrapper here.
  const child = spawn("/bin/sh", ["-c", 'export LOUPIST_GUI_LEASE_ENTRY="$$:$1"; exec "$1"', "fixture", script], {
    env: { ...process.env, LOUPIST_LIVE_PI_AGENT_DATA_DIR: root, LOUPIST_E2E_DEV_PORT: "1421" },
    stdio: ["ignore", "ignore", "pipe"],
  });
  let stderr = "";
  child.stderr.on("data", chunk => { stderr += chunk; });
  const [code] = await once(child, "exit");
  assert.equal(code, 19, stderr);
  assert.equal(await readFile(sharedReceipt, "utf8"), "shared window must survive");
  const observed = JSON.parse(await readFile(join(root, "observed.json"), "utf8"));
  assert.notEqual(observed.sessionPath, sharedReceipt);
  assert.equal(observed.content, '{"fixture":true}');
  await assert.rejects(readFile(observed.sessionPath), { code: "ENOENT" });

  // Reusing the current window must not send a baseline resize behind the
  // user's back. The real launch command runs against a ready session stand-in.
  const up = join(root, "up.sh");
  await copyFile(new URL("./up.sh", import.meta.url), up);
  await writeFile(join(root, "wait-session.mjs"), "process.exit(0);\n");
  const driverEvidence = join(root, "driver-used");
  await writeFile(join(root, "agent-driver.mjs"), `
import { writeFileSync } from 'node:fs';
writeFileSync(${JSON.stringify(driverEvidence)}, process.argv.slice(2).join(' '));
`);
  const reuse = spawn(up, [], { stdio: "ignore" });
  assert.equal((await once(reuse, "exit"))[0], 0);
  await assert.rejects(readFile(driverEvidence), { code: "ENOENT" });
} finally {
  await rm(root, { recursive: true, force: true });
}
console.log("live runner preserves shared receipt and cleans private state on scenario failure");
