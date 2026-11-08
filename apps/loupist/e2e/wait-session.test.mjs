import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "node:http";
import { createServer as createNetServer } from "node:net";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { focusNativeWindow } from "./native-window-focus.mjs";
import { captureOwnedWindow } from "./native-snapshot.mjs";
import {
  classifyWebdriverPort,
  clearOwnedSession,
  listenerOwnedBy,
  manualIdentifier,
  manualSessionLooksValid,
  nativeAppPid,
  nativeAppBundlePath,
  nativeAppTarget,
  processIdentity,
  resolveWebdriverPort,
  sessionOwnerIsLive,
  sessionIsLive,
  stopSession,
  webdriverIsReady,
  webdriverStatus,
  liveProfileName,
  liveSessionMatches,
  writeSession,
  writeManualSession,
} from "./wait-session-lib.mjs";

const helper = join(dirname(fileURLToPath(import.meta.url)), "wait-session.mjs");

function listenHttp(handler) {
  const server = createServer(handler);
  server.unref();
  return new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => {
      resolve({
        port: server.address().port,
        close: () => {
          if (typeof server.closeAllConnections === "function") {
            server.closeAllConnections();
          }
          server.close();
        },
      });
    });
  });
}

function listenTcp() {
  const server = createNetServer((socket) => {
    socket.end();
  });
  server.unref();
  return new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => {
      resolve({
        port: server.address().port,
        close: () => {
          if (typeof server.closeAllConnections === "function") {
            server.closeAllConnections();
          }
          server.close();
        },
      });
    });
  });
}

function statusHandler(ready) {
  return (request, response) => {
    if (request.url !== "/status") {
      response.writeHead(404);
      response.end();
      return;
    }
    response.writeHead(200, { "content-type": "application/json" });
    response.end(
      JSON.stringify({
        value: {
          ready,
          message: ready
            ? "tauri-plugin-webdriver is ready"
            : "waiting for webview initialization",
        },
      }),
    );
  };
}

function runWait(args) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, [helper, ...args], {
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stderr = "";
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("exit", (code) => {
      resolve({ code, stderr });
    });
  });
}

const tcp = await listenTcp();
const directory = mkdtempSync(join(tmpdir(), "loupist-wait-"));
try {
  const sessionPath = join(directory, "current-session.json");
  writeSession(sessionPath, process.pid, tcp.port, "development");
  assert.equal(await sessionIsLive(sessionPath), false);
  assert.equal(await webdriverIsReady(tcp.port), false);
  assert.equal(await classifyWebdriverPort(tcp.port), "foreign");
  const resolved = await resolveWebdriverPort(tcp.port);
  assert.notEqual(resolved, tcp.port);
  assert.equal(await classifyWebdriverPort(resolved), "free");
} finally {
  tcp.close();
  rmSync(directory, { recursive: true, force: true });
}

const notReady = await listenHttp(statusHandler(false));
try {
  assert.equal((await webdriverStatus(notReady.port))?.value?.ready, false);
  assert.equal(await webdriverIsReady(notReady.port), false);
} finally {
  notReady.close();
}

const ready = await listenHttp(statusHandler(true));
const readyDirectory = mkdtempSync(join(tmpdir(), "loupist-wait-"));
try {
  const sessionPath = join(readyDirectory, "current-session.json");
  writeSession(sessionPath, process.pid, ready.port, "development");
  assert.equal(await webdriverIsReady(ready.port), true);
  assert.equal(await sessionIsLive(sessionPath), true);
  assert.equal(listenerOwnedBy(ready.port, process.pid), true);
  assert.notEqual(await resolveWebdriverPort(ready.port), ready.port,
    "new launch must not collide with an existing WebDriver endpoint");

  const stranger = spawn(process.execPath, ["-e", "setTimeout(() => {}, 30_000)"], {
    stdio: "ignore",
  });
  try {
    const foreignPath = join(readyDirectory, "foreign-session.json");
    writeSession(foreignPath, stranger.pid, ready.port, "development");
    assert.equal(await sessionIsLive(foreignPath), false,
      "a live launcher cannot claim another process's ready endpoint");
    if (process.platform === "darwin") {
      const foreign = JSON.parse(readFileSync(foreignPath, "utf8"));
      await assert.rejects(focusNativeWindow({}, foreign), /owned WebDriver endpoint/);
      await assert.rejects(captureOwnedWindow(foreignPath, readyDirectory), /not live and owned/);
    }
    assert.equal(await sessionIsLive(sessionPath, stranger.pid), false);
    const wrongSession = await runWait([
      "--pid", String(stranger.pid), "--session", sessionPath,
      "--timeout-ms", "800",
    ]);
    assert.equal(wrongSession.code, 1);
    assert.match(wrongSession.stderr, /session was not ready/);
    const ignored = await runWait([
      "--wait-owned",
      "--pid",
      String(stranger.pid),
      "--port",
      String(ready.port),
      "--timeout-ms",
      "800",
    ]);
    assert.equal(ignored.code, 1);
    assert.match(ignored.stderr, /did not become ready/);
  } finally {
    stranger.kill("SIGTERM");
  }

  const owned = await runWait([
    "--wait-owned",
    "--pid",
    String(process.pid),
    "--port",
    String(ready.port),
    "--timeout-ms",
    "2000",
  ]);
  assert.equal(owned.code, 0, owned.stderr);
} finally {
  ready.close();
  rmSync(readyDirectory, { recursive: true, force: true });
}

console.log("wait-session readiness contract passed");

// Exercise real processes and ephemeral listeners. Never start the app, use
// port 1420, or touch the repository's current-session.json in these tests.
const lifecycleDirectory = mkdtempSync(join(tmpdir(), "loupist-owner-"));
const appStandIn = join(lifecycleDirectory, "loupist");
// A script, not a link to /bin/sleep: a multicall coreutils (uutils on Ubuntu
// 26.04) picks the utility from argv[0] and exits at once when it is "loupist".
writeFileSync(appStandIn, '#!/bin/sh\nsleep "$1"\n', { mode: 0o755 });
const launcher = spawn(process.execPath, ["--input-type=module", "-e", `
  import { spawn } from "node:child_process";
  import { createServer } from "node:http";
  const child = spawn(process.argv[1], ["30"], { stdio: "ignore" });
  const server = createServer((_request, response) => {
    response.setHeader("content-type", "application/json");
    response.end(JSON.stringify({ value: { ready: true } }));
  });
  server.listen(0, "127.0.0.1", () => {
    console.log(JSON.stringify({ nativePid: child.pid, port: server.address().port }));
  });
`, appStandIn], { stdio: ["ignore", "pipe", "inherit"] });
const unrelated = spawn(process.execPath, ["-e", "setTimeout(() => {}, 30_000)"], { stdio: "ignore" });
let nativePid;
try {
  const output = await new Promise((resolveOutput, reject) => {
    let text = "";
    launcher.stdout.on("data", (chunk) => {
      text += chunk;
      if (text.includes("\n")) resolveOutput(JSON.parse(text.trim()));
    });
    launcher.once("error", reject);
    launcher.once("exit", (code) => reject(new Error(`fixture exited: ${code}`)));
  });
  nativePid = output.nativePid;
  const sessionPath = join(lifecycleDirectory, "current-session.json");
  const dataPath = join(lifecycleDirectory, "persistent-data");
  writeFileSync(dataPath, "preserve user data");
  assert.equal(nativeAppPid(launcher.pid), nativePid);
  assert.equal(nativeAppBundlePath(nativePid), null, "a raw process is not a macOS app target");
  assert.equal(manualIdentifier("knowledge-preview"), "com.monarchjuno.loupist.knowledge-preview");
  for (const invalid of ["", "e2e", "development", "../old", "name.with.dot", "Uppercase"]) {
    assert.throws(() => manualIdentifier(invalid));
  }
  writeManualSession(sessionPath, launcher.pid, launcher.pid, "knowledge-preview");
  const manual = JSON.parse(readFileSync(sessionPath, "utf8"));
  assert.equal(manualSessionLooksValid(manual), true);
  assert.equal(sessionOwnerIsLive(manual), true);
  assert.throws(() => writeManualSession(sessionPath, launcher.pid, unrelated.pid, "preview"), /owned Tauri process/);
  await clearOwnedSession(sessionPath, unrelated.pid);
  assert.equal(existsSync(sessionPath), true);

  // Recovery attaches to the live recorded endpoint without starting a second
  // process; the same ownership also gates the ordinary manual profile.
  writeSession(sessionPath, launcher.pid, output.port, "development");
  await assert.rejects(nativeAppTarget(sessionPath), /unbundled executable/);
  const livePath = join(lifecycleDirectory, "live-session.json");
  writeSession(livePath, launcher.pid, output.port, "e2e", "qa-20260930");
  const live = JSON.parse(readFileSync(livePath, "utf8"));
  assert.equal(liveProfileName("qa-20260930"), "qa-20260930");
  assert.throws(() => liveProfileName("../development"), /Live profile/);
  assert.equal(liveSessionMatches(live, "qa-20260930"), true);
  assert.equal(liveSessionMatches(live, "live"), false);
  assert.throws(
    () => writeSession(livePath, launcher.pid, output.port, "development", "qa-20260930"),
    /only recorded/,
  );
  const originalIdentity = processIdentity(launcher.pid);
  const reused = await runWait(["--recover", sessionPath, "--timeout-ms", "1000"]);
  assert.equal(reused.code, 0, reused.stderr);
  assert.equal(processIdentity(launcher.pid), originalIdentity);

  // A stale PID receipt must not kill a different live process, even if its
  // endpoint is responsive. Removing the stale receipt preserves all data.
  const stale = JSON.parse(readFileSync(sessionPath, "utf8"));
  stale.launcherStartedAt = "a previous process with the same PID";
  writeFileSync(sessionPath, JSON.stringify(stale));
  assert.equal(await sessionIsLive(sessionPath), false);
  await assert.rejects(nativeAppTarget(sessionPath), /not live and owned/);
  await stopSession(sessionPath);
  assert.equal(existsSync(sessionPath), false);
  assert.equal(processIdentity(launcher.pid), originalIdentity);
  assert.equal(await webdriverIsReady(output.port), true);

  writeManualSession(sessionPath, launcher.pid, launcher.pid, "knowledge-preview");
  await stopSession(sessionPath);
  assert.equal(processIdentity(launcher.pid), null);
  assert.equal(processIdentity(nativePid), null);
  assert.notEqual(processIdentity(unrelated.pid), null);
  assert.equal(readFileSync(dataPath, "utf8"), "preserve user data");
  assert.equal(existsSync(sessionPath), false);

  // Exited-owner receipts clear immediately without waiting for port 1420 or
  // stopping a foreign listener/process.
  writeFileSync(sessionPath, JSON.stringify(manual));
  await stopSession(sessionPath);
  assert.equal(existsSync(sessionPath), false);
  assert.notEqual(processIdentity(unrelated.pid), null);
} finally {
  for (const pid of [launcher.pid, nativePid, unrelated.pid]) {
    try { if (pid) process.kill(pid, "SIGTERM"); } catch { /* already exited */ }
  }
  rmSync(lifecycleDirectory, { recursive: true, force: true });
}

console.log("manual profiles, live reuse, stale receipts and owned-tree cleanup passed");

// App tools need the exact bundle, including spaces, rather than a product name
// that could select or launch an installed release. No GUI is opened here.
test("macOS native discovery returns the owned bundle without changing its receipt", {
  skip: process.platform !== "darwin" ? "macOS app discovery is not a Linux capability" : false,
}, async () => {
  const bundleRoot = mkdtempSync(join(tmpdir(), "loupist bundle target "));
  const bundle = join(bundleRoot, "Loupist Dev.app");
  mkdirSync(join(bundle, "Contents/MacOS"), { recursive: true });
  writeFileSync(join(bundle, "Contents/Info.plist"), "fixture");
  symlinkSync("/bin/sleep", join(bundle, "Contents/MacOS/loupist"));
  const bundledChild = spawn(join(bundle, "Contents/MacOS/loupist"), ["30"], { stdio: "ignore" });
  try {
    await new Promise((resolveStarted, reject) => { bundledChild.once("spawn", resolveStarted); bundledChild.once("error", reject); });
    assert.equal(nativeAppBundlePath(bundledChild.pid), bundle);
    const receipt = join(bundleRoot, "manual-session.json");
    writeManualSession(receipt, process.pid, bundledChild.pid, "window-proof");
    const before = readFileSync(receipt, "utf8");
    assert.deepEqual(await nativeAppTarget(receipt), {
      nativePid: bundledChild.pid,
      nativeStartedAt: processIdentity(bundledChild.pid),
      appBundlePath: bundle,
      visibleWindowVerified: false,
    });
    assert.equal(readFileSync(receipt, "utf8"), before, "native discovery must not rewrite the session receipt");
  } finally {
    bundledChild.kill("SIGTERM");
    rmSync(bundleRoot, { recursive: true, force: true });
  }
});
