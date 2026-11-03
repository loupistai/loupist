import { execFile } from "node:child_process";
import { mkdir, mkdtemp, rm, stat } from "node:fs/promises";
import { join } from "node:path";
import { promisify } from "node:util";
import { nativeAppTarget, processIdentity } from "./wait-session-lib.mjs";
import { minimumWindowSize } from "./window-geometry.mjs";

const run = promisify(execFile);
// CoreGraphics also lists menu strips as layer-zero windows on other Spaces.
// Use the app's minimum window size and require one eligible window.
const findWindow = `
ObjC.import("AppKit");
ObjC.import("CoreGraphics");
ObjC.bindFunction("CGPreflightScreenCaptureAccess", ["bool", []]);
function run([targetPid, bundlePath, minWidth, minHeight]) {
  const pid = Number(targetPid);
  const app = $.NSRunningApplication.runningApplicationWithProcessIdentifier(pid);
  if (ObjC.unwrap(app.bundleURL.path) !== bundlePath ||
      !String(ObjC.unwrap(app.bundleIdentifier)).startsWith("com.monarchjuno.loupist.")) {
    throw Error("The recorded development app changed.");
  }
  if (!$.CGPreflightScreenCaptureAccess()) throw Error("Screen Recording permission is required.");
  const info = $.CGWindowListCopyWindowInfo(0, 0);
  const windows = ObjC.deepUnwrap(ObjC.castRefToObject(info)).filter(w =>
    w.kCGWindowOwnerPID === pid && w.kCGWindowLayer === 0 &&
    w.kCGWindowBounds.Width >= Number(minWidth) && w.kCGWindowBounds.Height >= Number(minHeight)
  );
  if (windows.length !== 1) throw Error("Expected one app-sized window, found " + windows.length);
  const w = windows[0];
  return JSON.stringify({
    id: w.kCGWindowNumber, width: w.kCGWindowBounds.Width, height: w.kCGWindowBounds.Height,
    onScreen: !!w.kCGWindowIsOnscreen, frontmost: !!app.active
  });
}`;

export async function captureOwnedWindow(sessionPath, artifactRoot) {
  if (process.platform !== "darwin") throw new Error("native-snapshot requires macOS.");
  const { nativePid, nativeStartedAt, appBundlePath } = await nativeAppTarget(sessionPath);
  const query = async () => {
    const { stdout } = await run("/usr/bin/osascript", [
      "-l", "JavaScript", "-e", findWindow, String(nativePid), appBundlePath,
      String(minimumWindowSize.width), String(minimumWindowSize.height),
    ], { timeout: 8_000 });
    const window = JSON.parse(stdout);
    if (!Number.isSafeInteger(window.id) || window.id <= 0 || processIdentity(nativePid) !== nativeStartedAt) {
      throw new Error("The recorded development window is no longer available.");
    }
    return window;
  };
  const before = await query();
  await mkdir(artifactRoot, { recursive: true, mode: 0o700 });
  const directory = await mkdtemp(join(artifactRoot, "native-window-"));
  const path = join(directory, "window.png");
  try {
    // One window ID. Never activate an app or capture the desktop as a fallback.
    await run("/usr/sbin/screencapture", ["-x", "-o", "-l", String(before.id), path], { timeout: 8_000 });
    const after = await query();
    if (before.id !== after.id || !(await stat(path)).size) throw new Error("Window changed during capture.");
    return { path, windowPoints: { width: before.width, height: before.height }, onScreen: before.onScreen, frontmost: before.frontmost };
  } catch (error) {
    await rm(directory, { recursive: true, force: true });
    throw error;
  }
}
