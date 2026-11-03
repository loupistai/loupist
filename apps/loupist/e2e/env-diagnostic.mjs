import { existsSync, lstatSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import path, { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { livePreset } from "./live-preset.mjs";
import { liveProfileName, portIsOpen } from "./wait-session-lib.mjs";

const e2eRoot = dirname(fileURLToPath(import.meta.url));

export function developmentPiAgentDir({
  platform = process.platform,
  home = homedir(),
} = {}) {
  const paths = platform === "win32" ? path.win32 : path.posix;
  return paths.join(home, ".loupist", "development", "pi");
}

export function inspectDirectory(path, stat = lstatSync) {
  try {
    const info = stat(path);
    if (info.isSymbolicLink()) return "symlink";
    if (!info.isDirectory()) return "not-directory";
    return "present";
  } catch (error) {
    return error?.code === "ENOENT" ? "missing" : "unavailable";
  }
}

export function liveStateDir(artifactRoot, profile) {
  return join(artifactRoot, "live-profiles", liveProfileName(profile));
}

function sessionSummary(sessionPath) {
  if (!existsSync(sessionPath)) return { present: false };
  try {
    const session = JSON.parse(readFileSync(sessionPath, "utf8"));
    return {
      present: true,
      profile: session.profile ?? null,
      liveProfile: session.liveProfile ?? null,
      manualProfile: session.manualProfile ?? null,
      port: session.webdriverConfig?.port ?? null,
    };
  } catch {
    return { present: true, readable: false };
  }
}

export async function environmentReport({
  piDir = developmentPiAgentDir(),
  artifactRoot = join(e2eRoot, "artifacts"),
  profile = null,
} = {}) {
  const vite =
    (await portIsOpen(1420, "127.0.0.1")) || (await portIsOpen(1420, "::1"));
  const webdriver =
    (await portIsOpen(14440, "127.0.0.1")) ||
    (await portIsOpen(14440, "::1"));
  return {
    preset: livePreset(),
    piAgentDataDir: { path: piDir, status: inspectDirectory(piDir) },
    ...(profile
      ? { liveStateDir: liveStateDir(artifactRoot, profile) }
      : {}),
    session: sessionSummary(join(artifactRoot, "current-session.json")),
    ports: { vite1420: vite ? "open" : "closed", webdriver14440: webdriver ? "open" : "closed" },
    credentials: "not read",
  };
}

function option(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : null;
}

async function main() {
  if (process.argv.includes("--print-pi-dir")) {
    const path = developmentPiAgentDir();
    const status = inspectDirectory(path);
    if (status !== "present") {
      console.error(`development Pi directory is ${status}: ${path}`);
      console.error("Credentials were not read. No sign-in setup was attempted.");
      process.exit(1);
    }
    process.stdout.write(`${path}\n`);
    return;
  }
  if (process.argv.includes("--print-live-state")) {
    const profile = option("--profile");
    if (!profile) throw new Error("--profile is required");
    process.stdout.write(`${liveStateDir(join(e2eRoot, "artifacts"), profile)}\n`);
    return;
  }
  console.log(JSON.stringify(await environmentReport({
    profile: option("--profile"),
  }), null, 2));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main();
}
