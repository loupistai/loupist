#!/usr/bin/env node
import { spawn } from "node:child_process";
import { mkdirSync, unlinkSync, existsSync, readFileSync, fstatSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  resolveWebdriverPort,
  writeSession,
} from "../e2e/wait-session-lib.mjs";

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const cli = resolve(appRoot, "node_modules/@tauri-apps/cli/tauri.js");
const sessionPath = resolve(appRoot, "e2e/artifacts/current-session.json");
const args = process.argv.slice(2);
const isDev = args[0] === "dev";
const cliEnd = args.indexOf("--") < 0 ? args.length : args.indexOf("--");
const cliArgs = args.slice(0, cliEnd);
const hasFeatures = cliArgs.includes("--features");
const preferred = Number(process.env.TAURI_WEBDRIVER_PORT || 14440);

function leaseStdio() {
  const raw = process.env.LOUPIST_GUI_LEASE_FD;
  if (!isDev || process.platform === "win32" || raw === undefined) return "inherit";
  const fd = Number(raw);
  if (!Number.isInteger(fd) || fd < 3 || fd > 4096 || !fstatSync(fd).isFile()) {
    throw new Error("Native GUI lease descriptor was lost before spawning Tauri");
  }
  // Node's 'inherit' shorthand covers only 0..2. Keep the lease open in the
  // CLI too, so killing this wrapper cannot unlock a still-running native app.
  const stdio = Array(fd + 1).fill("ignore");
  stdio.splice(0, 3, "inherit", "inherit", "inherit");
  stdio[fd] = fd;
  return stdio;
}

function supervise(child, onExit = () => {}) {
  const signals = ["SIGINT", "SIGTERM", "SIGHUP"];
  const forward = (signal) => child.kill(signal);
  const handlers = signals.map(signal => [signal, () => forward(signal)]);
  for (const [signal, handler] of handlers) process.on(signal, handler);
  const detach = () => {
    for (const [signal, handler] of handlers) process.off(signal, handler);
  };
  return new Promise((resolveCode, reject) => {
    child.once("error", error => { detach(); onExit(); reject(error); });
    child.once("exit", (code, signal) => {
      detach();
      onExit();
      if (signal) process.kill(process.pid, signal);
      else resolveCode(code ?? 1);
    });
  });
}

async function main() {
  if (isDev && process.platform !== "win32" &&
      process.env.LOUPIST_GUI_LEASE_ENTRY !== `${process.pid}:${process.execPath}`) {
    // Acquire before probing ports or replacing a shared receipt. Python execs
    // this same entrypoint with the lease descriptor inherited; builds bypass it.
    return supervise(spawn("python3", [
      resolve(appRoot, "e2e/native_ui_lease.py"), process.execPath,
      fileURLToPath(import.meta.url), ...args,
    ], { stdio: leaseStdio() }));
  }

  if (isDev && !hasFeatures) args.splice(cliEnd, 0, "--features", "webdriver");
  if (isDev && process.platform === "darwin" && !cliArgs.includes("--release")) {
    let separator = args.indexOf("--");
    if (separator < 0) { separator = args.length; args.push("--"); }
    args.splice(separator + 1, 0, "--config", resolve(appRoot, "e2e/macos-dev-runner.toml"));
  }
  const env = { ...process.env };
  if (isDev) {
    const port = await resolveWebdriverPort(preferred);
    env.TAURI_WEBDRIVER_PORT = String(port);
    mkdirSync(dirname(sessionPath), { recursive: true, mode: 0o700 });
    writeSession(sessionPath, process.pid, port, "development");
  }

  const clearSession = () => {
    if (!isDev || !existsSync(sessionPath)) return;
    try {
      const session = JSON.parse(readFileSync(sessionPath, "utf8"));
      if (session.launcherPid === process.pid) unlinkSync(sessionPath);
    } catch {
      // Leave a session owned by another launcher alone.
    }
  };

  return supervise(spawn(process.execPath, [cli, ...args], {
    stdio: leaseStdio(), env,
  }), clearSession);
}

main().then(code => { process.exitCode = code; }).catch(error => {
  console.error(error.message);
  process.exitCode = 1;
});
