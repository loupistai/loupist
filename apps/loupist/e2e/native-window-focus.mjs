import { execFile } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { nativeAppBundlePath, nativeAppPid, sessionEndpointIsOwned, sessionOwnerIsLive } from "./wait-session-lib.mjs";

const activateSource = `import AppKit
import CoreGraphics

guard CommandLine.arguments.count == 3, let pid = Int32(CommandLine.arguments[1]) else {
  fputs("missing-pid\\n", stderr)
  exit(2)
}
guard let app = NSRunningApplication(processIdentifier: pid),
      app.bundleURL?.path == CommandLine.arguments[2],
      app.bundleIdentifier?.hasPrefix("com.monarchjuno.loupist.") == true else {
  fputs("no-running-application\\n", stderr)
  exit(3)
}
app.unhide()
let activated = app.activate(options: [.activateAllWindows])
var onScreen = 0
var clickPoint: CGPoint?
if let info = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] {
  for window in info {
    guard (window[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == pid else { continue }
    onScreen += 1
    let bounds = window[kCGWindowBounds as String] as? [String: NSNumber]
    let width = bounds?["Width"]?.doubleValue ?? 0
    let height = bounds?["Height"]?.doubleValue ?? 0
    let x = bounds?["X"]?.doubleValue ?? 0
    let y = bounds?["Y"]?.doubleValue ?? 0
    print("win x=\\(x) y=\\(y) w=\\(width) h=\\(height) layer=\\(window[kCGWindowLayer as String] ?? "") alpha=\\(window[kCGWindowAlpha as String] ?? "")")
    if width > 400, height > 300, clickPoint == nil {
      let step = 80.0
      var yScan = y + 80
      while yScan < y + height - 40 && clickPoint == nil {
        var xScan = x + 80
        while xScan < x + width - 40 && clickPoint == nil {
          let point = CGPoint(x: xScan, y: yScan)
          if let top = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] {
            for candidate in top {
              guard let bounds = candidate[kCGWindowBounds as String] as? [String: NSNumber] else { continue }
              let cx = bounds["X"]?.doubleValue ?? 0
              let cy = bounds["Y"]?.doubleValue ?? 0
              let cw = bounds["Width"]?.doubleValue ?? 0
              let ch = bounds["Height"]?.doubleValue ?? 0
              if point.x >= cx && point.x <= cx + cw && point.y >= cy && point.y <= cy + ch {
                if (candidate[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == pid { clickPoint = point }
                break
              }
            }
          }
          xScan += step
        }
        yScan += step
      }
    }
  }
}
if let clickPoint, let down = CGEvent(mouseEventSource: nil, mouseType: .leftMouseDown, mouseCursorPosition: clickPoint, mouseButton: .left),
   let up = CGEvent(mouseEventSource: nil, mouseType: .leftMouseUp, mouseCursorPosition: clickPoint, mouseButton: .left) {
  down.postToPid(pid)
  up.postToPid(pid)
  print("clicked=\\(clickPoint.x),\\(clickPoint.y)")
}
print("activated=\\(activated) policy=\\(app.activationPolicy.rawValue) hidden=\\(app.isHidden) name=\\(app.localizedName ?? "") bundle=\\(app.bundleIdentifier ?? "") onScreen=\\(onScreen)")
`;

function run(command, args, timeout = 8_000, limit = 4_000) {
  return new Promise((resolve) => {
    execFile(command, args, { timeout }, (error, stdout, stderr) => {
      resolve({
        code: error?.code ?? 0,
        stdout: String(stdout ?? "").trim().slice(0, limit),
        stderr: String(stderr ?? "").trim().slice(0, limit),
      });
    });
  });
}

async function activateE2EApp(binary, session) {
  if (!sessionEndpointIsOwned(session)) throw new Error("The native WebDriver owner changed.");
  const pid = nativeAppPid(session.launcherPid);
  const bundle = pid && nativeAppBundlePath(pid);
  if (!bundle) throw new Error("The recorded launcher's development app is unavailable.");
  return run(binary, [String(pid), bundle]);
}

/** Focus through the native driver's selected context, after verifying ownership. */
export async function focusNativeWindow(browser, session, { timeout = 25_000 } = {}) {
  if (!sessionOwnerIsLive(session)) throw new Error("Cannot focus a native window without its live session owner.");
  if (process.platform === "darwin" && !sessionEndpointIsOwned(session)) {
    throw new Error("Cannot focus a native window without its owned WebDriver endpoint.");
  }
  await browser.switchToWindow("main");
  if (process.env.LOUPIST_E2E_ALLOW_UNFOCUSED === "1") {
    await browser.waitUntil(
      () => browser.execute(() => document.readyState === "complete" && innerWidth > 0 && innerHeight > 0),
      { timeout, interval: 100, timeoutMsg: "The native test webview did not become ready." },
    );
    // A locked macOS session suspends CSS animations, leaving opening menus at
    // opacity zero. This opt-in harness mode checks static DOM behavior only.
    await browser.execute(() => {
      const style = document.createElement("style");
      style.textContent = "*, *::before, *::after { animation: none !important; transition: none !important; scroll-behavior: auto !important; }";
      document.head.append(style);
    });
    console.log("Webview-only run (motion disabled): OS focus, native dialogs and on-screen rendering are unverified.");
    return;
  }
  const scratch = process.platform === "darwin" ? await mkdtemp(join(tmpdir(), "loupist-e2e-focus-")) : null;
  try {
    let binary;
    if (scratch) {
      const scriptPath = join(scratch, "activate.swift");
      binary = join(scratch, "activate");
      await writeFile(scriptPath, activateSource);
      const compiled = await run("swiftc", ["-O", "-o", binary, scriptPath], 60_000);
      if (compiled.code !== 0) throw new Error(`Could not compile the native focus helper: ${compiled.stderr || compiled.stdout}`);
    }
    const started = Date.now();
    let last = null;
    let activation = null;
    let lastActivate = 0;
    while (Date.now() - started < timeout) {
      await browser.switchToWindow("main");
      if (Date.now() - lastActivate > 2_000) {
        lastActivate = Date.now();
        if (binary) activation = await activateE2EApp(binary, session);
        await browser.setWindowRect(40, 40, 1280, 820).catch(() => {});
      }
      last = await browser.execute(() => {
        window.focus();
        return {
          hasFocus: document.hasFocus(),
          visibility: document.visibilityState,
          size: { width: innerWidth, height: innerHeight },
        };
      }).catch((error) => ({ error: String(error) }));
      if (last?.hasFocus && last.visibility === "visible") return;
      await browser.pause(200);
    }
    let rect = null;
    try {
      rect = await browser.getWindowRect();
    } catch (error) {
      rect = String(error);
    }
    throw new Error(`The native test window did not become focused and visible. Last: ${JSON.stringify({ ...last, rect, activation })}`);
  } finally {
    if (scratch) await rm(scratch, { recursive: true, force: true });
  }
}
