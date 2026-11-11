import { existsSync, unlinkSync, writeFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import net from "node:net";
import { dirname, isAbsolute } from "node:path";

export const DEV_WEBDRIVER_PORT = 14440;
export const DEV_VITE_PORT = 1420;

const usage = `Usage:
  node wait-session.mjs --owner-live <current-session.json>
  node wait-session.mjs --native-app <current-session.json>
  node wait-session.mjs --is-manual-session <current-session.json> --manual-profile <name>
  node wait-session.mjs --manual-identifier <name>
  node wait-session.mjs --live-profile-name <name>
  node wait-session.mjs --stop-session <current-session.json>
  node wait-session.mjs --clear-session <current-session.json> --pid <pid>
  node wait-session.mjs --check-port <port>
  node wait-session.mjs --is-dev-session <current-session.json>
  node wait-session.mjs --is-live-session <current-session.json> --live-profile <name>
  node wait-session.mjs --recover <current-session.json>
  node wait-session.mjs --adopt-dev <current-session.json>
  node wait-session.mjs --wait-owned --pid <launcher-pid> --port <port> [--timeout-ms <ms>]
  node wait-session.mjs --resolve-port <preferred-port>
  node wait-session.mjs --write-session <current-session.json> --pid <pid> --port <port> --profile <development|e2e> [--live-profile <name>] [--spawned-at-ms <ms>] [--dev-url <http://127.0.0.1:port>]
  node wait-session.mjs --write-manual <current-session.json> --pid <pid> --app-pid <pid> --manual-profile <name>
  node wait-session.mjs --pid <launcher-pid> --session <current-session.json> [--timeout-ms <ms>]`;

export function integerFlag(name, fallback, argv = process.argv) {
  const index = argv.indexOf(name);
  if (index < 0) return fallback;
  const value = Number(argv[index + 1]);
  if (!Number.isInteger(value) || value <= 0) {
    throw new Error(`${name} must be a positive integer\n${usage}`);
  }
  return value;
}

export function stringFlag(name, argv = process.argv) {
  const index = argv.indexOf(name);
  if (index < 0) return null;
  return argv[index + 1] ?? null;
}

export function pidIsRunning(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

// PID alone is unsafe after a launcher exits and the OS reuses its number.
export function processIdentity(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return null;
  try {
    const identity = execFileSync("ps", ["-p", String(pid), "-o", "lstart=", "-o", "stat="], {
      encoding: "utf8", env: { ...process.env, LC_ALL: "C" },
    }).trim();
    const match = /^(.*?)\s+(\S+)$/.exec(identity);
    return match && !match[2].startsWith("Z") ? match[1].trim() : null;
  } catch {
    return null;
  }
}

export function sessionOwnerIsLive(session) {
  return typeof session?.launcherStartedAt === "string" &&
    processIdentity(session.launcherPid) === session.launcherStartedAt;
}

export function manualIdentifier(profile) {
  if (!/^[a-z][a-z0-9-]{0,47}$/.test(profile ?? "") ||
      ["development", "e2e"].includes(profile)) {
    throw new Error("Manual profile must be 1–48 lowercase letters, digits or hyphens, start with a letter, and cannot be development or e2e.");
  }
  return `com.monarchjuno.loupist.${profile}`;
}

export function liveProfileName(profile) {
  if (!/^[a-z][a-z0-9-]{0,47}$/.test(profile ?? "") ||
      ["development", "e2e", "manual"].includes(profile)) {
    throw new Error("Live profile must be 1–48 lowercase letters, digits or hyphens, start with a letter, and cannot be development, e2e, or manual.");
  }
  return profile;
}

export function liveSessionMatches(session, profile) {
  try {
    return sessionLooksValid(session) &&
      session.profile === "e2e" &&
      session.liveProfile === liveProfileName(profile);
  } catch {
    return false;
  }
}

export function manualSessionLooksValid(session) {
  if (session?.profile !== "manual") return false;
  try {
    return session.identifier === manualIdentifier(session.manualProfile) &&
      Number.isInteger(session.appPid) && typeof session.appStartedAt === "string";
  } catch {
    return false;
  }
}

export function nativeAppPid(rootPid) {
  return [...descendantPids(rootPid)].find((pid) => {
    try {
      const executable = execFileSync("ps", ["-p", String(pid), "-o", "comm="], { encoding: "utf8" }).trim();
      return /(^|\/)loupist$/.test(executable) && processIdentity(pid) !== null;
    } catch {
      return false;
    }
  }) ?? null;
}

export function nativeAppBundlePath(pid) {
  if (!processIdentity(pid)) return null;
  const executable = execFileSync("ps", ["-p", String(pid), "-o", "comm="], { encoding: "utf8" }).trim();
  if (!isAbsolute(executable) || !executable.endsWith(".app/Contents/MacOS/loupist")) return null;
  const bundle = dirname(dirname(dirname(executable)));
  return existsSync(`${bundle}/Contents/Info.plist`) ? bundle : null;
}

export async function nativeAppTarget(sessionPath) {
  const session = await readSession(sessionPath);
  // Native inspection must work even when the renderer's dev server is down.
  // Validate process ownership without rewriting a receipt or claiming pixels
  // are visible. WebDriver sessions still require their owned ready endpoint.
  const owned = sessionOwnerIsLive(session) && (manualSessionLooksValid(session)
    ? processIdentity(session.appPid) === session.appStartedAt &&
      descendantPids(session.launcherPid).has(session.appPid)
    : await sessionIsLive(sessionPath));
  if (!owned) throw new Error("The recorded app session is not live and owned; no application was opened.");
  const nativePid = nativeAppPid(session.appPid ?? session.launcherPid);
  const appBundlePath = nativePid ? nativeAppBundlePath(nativePid) : null;
  if (!appBundlePath) throw new Error("This session runs an unbundled executable. Its owner must restart it with the macOS development runner when ready; do not open the installed app instead.");
  return { nativePid, nativeStartedAt: processIdentity(nativePid), appBundlePath, visibleWindowVerified: false };
}

export function portIsOpen(port, host = "127.0.0.1") {
  return new Promise((resolveOpen) => {
    const socket = net.createConnection({ host, port });
    socket.setTimeout(250);
    socket.once("connect", () => {
      socket.destroy();
      resolveOpen(true);
    });
    const closed = () => {
      socket.destroy();
      resolveOpen(false);
    };
    socket.once("error", closed);
    socket.once("timeout", closed);
  });
}

export async function webdriverStatus(port, host = "127.0.0.1") {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 400);
  try {
    const response = await fetch(`http://${host}:${port}/status`, {
      signal: controller.signal,
    });
    if (!response.ok) return null;
    const body = await response.json();
    if (!body || typeof body !== "object" || !("value" in body)) return null;
    return body;
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}

export function webdriverValueIsReady(status) {
  return status?.value?.ready === true;
}

export async function webdriverIsReady(port, host = "127.0.0.1") {
  return webdriverValueIsReady(await webdriverStatus(port, host));
}

export async function readSession(sessionPath) {
  if (!existsSync(sessionPath)) return null;
  try {
    return JSON.parse(await readFile(sessionPath, "utf8"));
  } catch {
    return null;
  }
}

export function sessionLooksValid(session) {
  const port = session?.webdriverConfig?.port;
  return (
    (session?.profile === "development" || session?.profile === "e2e") &&
    session?.webdriverConfig?.protocol === "http" &&
    session?.webdriverConfig?.hostname === "127.0.0.1" &&
    session?.capabilities?.browserName === "tauri" &&
    Number.isInteger(port) && port >= 1024 && port <= 65535
  );
}

export async function sessionIsLive(sessionPath, expectedLauncherPid = null) {
  const session = await readSession(sessionPath);
  if (expectedLauncherPid !== null && session?.launcherPid !== expectedLauncherPid) return false;
  if (!sessionOwnerIsLive(session)) return false;
  if (manualSessionLooksValid(session)) {
    if (processIdentity(session.appPid) !== session.appStartedAt ||
        !listenerOwnedBy(DEV_VITE_PORT, session.appPid)) return false;
    const nativePid = nativeAppPid(session.appPid);
    if (!nativePid) return false;
    if (session.nativePid !== nativePid) {
      writeFileSync(sessionPath, `${JSON.stringify({ ...session, nativePid }, null, 2)}\n`, { mode: 0o600 });
    }
    return true;
  }
  if (!sessionEndpointIsOwned(session)) return false;
  return webdriverIsReady(
    session.webdriverConfig.port,
    session.webdriverConfig.hostname,
  );
}

export function sessionEndpointIsOwned(session) {
  return sessionLooksValid(session) && sessionOwnerIsLive(session) &&
    listenerOwnedBy(session.webdriverConfig.port, session.launcherPid);
}

export function writeSession(sessionPath, launcherPid, port, profile, liveProfile = null, extras = null) {
  if (profile !== "development" && profile !== "e2e") {
    throw new Error("session profile must be development or e2e");
  }
  if (liveProfile != null && profile !== "e2e") {
    throw new Error("live profile is only recorded for an e2e WebDriver session");
  }
  const session = {
    launcherPid,
    launcherStartedAt: processIdentity(launcherPid),
    profile,
    webdriverConfig: {
      protocol: "http",
      hostname: "127.0.0.1",
      port,
      path: "/",
    },
    capabilities: {
      browserName: "tauri",
      "wdio:tauriServiceOptions": { windowLabel: "main" },
    },
  };
  if (liveProfile != null) session.liveProfile = liveProfileName(liveProfile);
  if (extras != null) {
    if (extras.spawnedAtMs != null) {
      if (!Number.isInteger(extras.spawnedAtMs) || extras.spawnedAtMs <= 0) {
        throw new Error("spawnedAtMs must be a positive integer");
      }
      session.spawnedAtMs = extras.spawnedAtMs;
    }
    if (extras.devUrl != null) {
      if (!/^http:\/\/(?:127\.0\.0\.1|localhost):\d{2,5}$/.test(String(extras.devUrl))) {
        throw new Error("devUrl must be a loopback http URL");
      }
      session.devUrl = extras.devUrl;
    }
  }
  writeFileSync(sessionPath, `${JSON.stringify(session, null, 2)}\n`, { mode: 0o600 });
}

export function writeManualSession(sessionPath, launcherPid, appPid, profile) {
  if (!processIdentity(launcherPid) || !processIdentity(appPid) || !descendantPids(launcherPid).has(appPid)) {
    throw new Error("Manual session requires a live launcher and its owned Tauri process.");
  }
  writeFileSync(sessionPath, `${JSON.stringify({
    launcherPid,
    launcherStartedAt: processIdentity(launcherPid),
    appPid,
    appStartedAt: processIdentity(appPid),
    profile: "manual",
    manualProfile: profile,
    identifier: manualIdentifier(profile),
  }, null, 2)}\n`, { mode: 0o600 });
}

export async function clearOwnedSession(sessionPath, launcherPid) {
  const session = await readSession(sessionPath);
  if (session?.launcherPid === launcherPid) {
    try { unlinkSync(sessionPath); } catch (error) { if (error.code !== "ENOENT") throw error; }
  }
}

export async function stopSession(sessionPath) {
  const session = await readSession(sessionPath);
  if (!session) return;
  if (!session.launcherStartedAt) {
    throw new Error("Session has no process identity; refusing to stop an unverified PID. Inspect the owning process before closing it.");
  }
  if (!sessionOwnerIsLive(session)) {
    await clearOwnedSession(sessionPath, session.launcherPid);
    return;
  }
  const owned = [...descendantPids(session.launcherPid)]
    .map((pid) => ({ pid, identity: processIdentity(pid) }));
  for (const { pid, identity } of owned) {
    if (identity && processIdentity(pid) === identity) {
      try { process.kill(pid, "SIGTERM"); } catch (error) { if (error.code !== "ESRCH") throw error; }
    }
  }
  const deadline = Date.now() + 15_000;
  while (owned.some(({ pid, identity }) => identity && processIdentity(pid) === identity)) {
    if (Date.now() >= deadline) throw new Error("Timed out stopping the recorded Loupist process tree; no unrelated process was stopped.");
    await new Promise((resolveWait) => setTimeout(resolveWait, 100));
  }
  await clearOwnedSession(sessionPath, session.launcherPid);
}

export async function classifyWebdriverPort(port, host = "127.0.0.1") {
  if (await webdriverStatus(port, host)) return "webdriver";
  if (await portIsOpen(port, host)) return "foreign";
  return "free";
}

export function allocateFreePort() {
  return new Promise((resolvePort, reject) => {
    const server = net.createServer();
    server.unref();
    server.once("error", (error) => {
      reject(error);
    });
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = address?.port;
      server.close((error) => {
        if (error) {
          reject(error);
          return;
        }
        if (!Number.isInteger(port) || port <= 0) {
          reject(new Error("failed to allocate a loopback WebDriver port"));
          return;
        }
        resolvePort(port);
      });
    });
  });
}

export async function resolveWebdriverPort(preferred) {
  if (!Number.isInteger(preferred) || preferred < 1024 || preferred > 65535) {
    throw new Error("TAURI_WEBDRIVER_PORT must be an integer from 1024 to 65535.");
  }
  const classification = await classifyWebdriverPort(preferred);
  if (classification === "free") return preferred;
  const port = await allocateFreePort();
  console.error(
    `Loupist WebDriver port ${preferred} is occupied (${classification}); using ${port}.`,
  );
  return port;
}

export function listeningPids(port) {
  try {
    const output = execFileSync(
      "lsof",
      ["-nP", `-iTCP:${port}`, "-sTCP:LISTEN", "-t"],
      { encoding: "utf8" },
    );
    return output
      .trim()
      .split(/\s+/)
      .map(Number)
      .filter((pid) => Number.isInteger(pid) && pid > 0);
  } catch {
    return [];
  }
}

export function listeningPid(port) {
  return listeningPids(port)[0] ?? null;
}

function childPids(pid) {
  try {
    return execFileSync("pgrep", ["-P", String(pid)], { encoding: "utf8" })
      .trim()
      .split(/\s+/)
      .map(Number)
      .filter((child) => Number.isInteger(child) && child > 0);
  } catch {
    return [];
  }
}

export function descendantPids(rootPid) {
  const seen = new Set();
  const stack = [rootPid];
  while (stack.length > 0) {
    const pid = stack.pop();
    if (seen.has(pid)) continue;
    seen.add(pid);
    stack.push(...childPids(pid));
  }
  return seen;
}

export function listenerOwnedBy(port, rootPid) {
  const tree = descendantPids(rootPid);
  return listeningPids(port).some((pid) => tree.has(pid));
}

async function waitUntil(predicate, { pid, timeoutMs, missingPidMessage, timeoutMessage }) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (!pidIsRunning(pid)) {
      throw new Error(missingPidMessage);
    }
    if (await predicate()) return;
    await new Promise((resolveWait) => setTimeout(resolveWait, 200));
  }
  throw new Error(timeoutMessage);
}

export async function main(argv = process.argv) {
  const manualProfile = stringFlag("--manual-identifier", argv);
  if (manualProfile) {
    process.stdout.write(manualIdentifier(manualProfile));
    return;
  }
  const liveName = stringFlag("--live-profile-name", argv);
  if (liveName) {
    process.stdout.write(`${liveProfileName(liveName)}\n`);
    return;
  }
  const ownerPath = stringFlag("--owner-live", argv);
  if (ownerPath) process.exit(sessionOwnerIsLive(await readSession(ownerPath)) ? 0 : 1);

  const nativePath = stringFlag("--native-app", argv);
  if (nativePath) {
    process.stdout.write(`${JSON.stringify(await nativeAppTarget(nativePath), null, 2)}\n`);
    return;
  }

  const manualPath = stringFlag("--is-manual-session", argv);
  if (manualPath) {
    const session = await readSession(manualPath);
    process.exit(manualSessionLooksValid(session) && session.manualProfile === stringFlag("--manual-profile", argv) ? 0 : 1);
  }

  const stopPath = stringFlag("--stop-session", argv);
  if (stopPath) { await stopSession(stopPath); return; }
  const clearPath = stringFlag("--clear-session", argv);
  if (clearPath) { await clearOwnedSession(clearPath, integerFlag("--pid", 0, argv)); return; }

  const writeManualPath = stringFlag("--write-manual", argv);
  if (writeManualPath) {
    writeManualSession(writeManualPath, integerFlag("--pid", 0, argv), integerFlag("--app-pid", 0, argv), stringFlag("--manual-profile", argv));
    return;
  }

  const isDevSession = stringFlag("--is-dev-session", argv);
  if (isDevSession) {
    const session = await readSession(isDevSession);
    process.exit(sessionLooksValid(session) && session.profile === "development" ? 0 : 1);
  }

  const isLiveSession = stringFlag("--is-live-session", argv);
  if (isLiveSession) {
    const session = await readSession(isLiveSession);
    process.exit(liveSessionMatches(session, stringFlag("--live-profile", argv)) ? 0 : 1);
  }

  const checkPort = integerFlag("--check-port", 0, argv);
  if (checkPort) {
    const open =
      (await portIsOpen(checkPort, "127.0.0.1")) ||
      (await portIsOpen(checkPort, "::1"));
    process.exit(open ? 0 : 1);
  }

  const recoverPath = stringFlag("--recover", argv);
  if (recoverPath) {
    const session = await readSession(recoverPath);
    if (!(manualSessionLooksValid(session) || (sessionLooksValid(session) && (session.profile === "development" || session.profile === "e2e")))) {
      process.exit(1);
    }
    if (await sessionIsLive(recoverPath)) process.exit(0);
    const launcherPid = session.launcherPid;
    if (!sessionOwnerIsLive(session)) {
      process.exit(1);
    }
    await waitUntil(() => sessionIsLive(recoverPath), {
      pid: launcherPid,
      timeoutMs: integerFlag("--timeout-ms", 300_000, argv),
      missingPidMessage:
        "Loupist development launcher exited before the app returned",
      timeoutMessage:
        "Loupist development app did not return within the timeout",
    });
    process.exit(0);
  }

  const adoptPath = stringFlag("--adopt-dev", argv);
  if (adoptPath) {
    const port = Number(process.env.TAURI_WEBDRIVER_PORT || DEV_WEBDRIVER_PORT);
    const viteUp =
      (await portIsOpen(DEV_VITE_PORT, "127.0.0.1")) ||
      (await portIsOpen(DEV_VITE_PORT, "::1"));
    if (!viteUp || !(await webdriverIsReady(port))) process.exit(1);
    const launcherPid = listeningPid(port);
    if (!launcherPid) process.exit(1);
    writeSession(adoptPath, launcherPid, port, "development");
    process.exit(0);
  }

  const resolvePreferred = integerFlag("--resolve-port", 0, argv);
  if (resolvePreferred) {
    process.stdout.write(String(await resolveWebdriverPort(resolvePreferred)));
    process.exit(0);
  }

  const writePath = stringFlag("--write-session", argv);
  if (writePath) {
    const devUrl = stringFlag("--dev-url", argv);
    if (devUrl != null && !/^http:\/\/(?:127\.0\.0\.1|localhost):\d{2,5}$/.test(devUrl)) {
      throw new Error(`--dev-url must be a loopback http URL\n${usage}`);
    }
    const extras = {};
    if (argv.includes("--spawned-at-ms")) extras.spawnedAtMs = integerFlag("--spawned-at-ms", 0, argv);
    if (devUrl != null) extras.devUrl = devUrl;
    writeSession(
      writePath,
      integerFlag("--pid", 0, argv),
      integerFlag("--port", 0, argv),
      stringFlag("--profile", argv),
      stringFlag("--live-profile", argv),
      extras,
    );
    process.exit(0);
  }

  if (argv.includes("--wait-owned")) {
    const pid = integerFlag("--pid", 0, argv);
    const port = integerFlag("--port", 0, argv);
    const timeoutMs = integerFlag("--timeout-ms", 300_000, argv);
    if (!pid || !port) {
      throw new Error(usage);
    }
    await waitUntil(
      async () => (await webdriverIsReady(port)) && listenerOwnedBy(port, pid),
      {
        pid,
        timeoutMs,
        missingPidMessage:
          "Loupist launcher exited before WebDriver became ready",
        timeoutMessage: `Loupist WebDriver did not become ready within ${timeoutMs} milliseconds`,
      },
    );
    process.exit(0);
  }

  const pid = integerFlag("--pid", 0, argv);
  const sessionPath = stringFlag("--session", argv);
  const timeoutMs = integerFlag("--timeout-ms", 300_000, argv);
  if (!pid || !sessionPath) {
    throw new Error(usage);
  }

  await waitUntil(() => sessionIsLive(sessionPath, pid), {
    pid,
    timeoutMs,
    missingPidMessage: "Loupist launcher exited before the session was ready",
    timeoutMessage: `Loupist session was not ready within ${timeoutMs} milliseconds`,
  });
  process.exit(0);
}
