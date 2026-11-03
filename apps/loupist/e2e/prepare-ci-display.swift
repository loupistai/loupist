import AppKit
import CoreGraphics
import Foundation

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data("\(message)\n".utf8))
    exit(1)
}

// This changes only an ephemeral GitHub runner's login session, never a local desktop.
guard ProcessInfo.processInfo.environment["GITHUB_ACTIONS"] == "true",
      ProcessInfo.processInfo.environment["RUNNER_OS"] == "macOS" else {
    fail("Display preparation is restricted to GitHub Actions macOS runners.")
}

let display = CGMainDisplayID()
let options = [kCGDisplayShowDuplicateLowResolutionModes: true] as CFDictionary
guard let modes = CGDisplayCopyAllDisplayModes(display, options) as? [CGDisplayMode] else {
    fail("Could not enumerate CI display modes.")
}

let dimensions = Set(modes.map { "\($0.width)x\($0.height)" }).sorted()
print("Available CI display modes: \(dimensions.joined(separator: ", "))")

// Keep the native suite's required 1680x820 window inside the visible desktop.
// Prefer 2560x1440 so the optional wide layout boundary can run as well.
let candidates = modes.filter {
    $0.isUsableForDesktopGUI() && $0.width >= 1920 && $0.height >= 1080
}.sorted {
    let leftPreferred = $0.width == 2560 && $0.height == 1440
    let rightPreferred = $1.width == 2560 && $1.height == 1440
    if leftPreferred != rightPreferred { return leftPreferred }
    return $0.width * $0.height < $1.width * $1.height
}

guard !candidates.isEmpty else {
    fail("CI display has no supported mode large enough for native interaction tests.")
}

var selected = false
for mode in candidates {
    var configuration: CGDisplayConfigRef?
    guard CGBeginDisplayConfiguration(&configuration) == .success else {
        fail("Could not begin CI display configuration.")
    }
    let configured = CGConfigureDisplayWithDisplayMode(configuration, display, mode, nil)
    guard configured == .success else {
        CGCancelDisplayConfiguration(configuration)
        continue
    }
    // forSession survives this helper process, without changing permanent settings.
    let result = CGCompleteDisplayConfiguration(configuration, .forSession)
    if result == .success {
        print("Selected CI display mode: \(mode.width)x\(mode.height)")
        selected = true
        break
    }
    print("Could not apply \(mode.width)x\(mode.height): \(result.rawValue)")
}
guard selected else { fail("Could not apply a supported CI display mode.") }

let deadline = Date().addingTimeInterval(5)
repeat {
    RunLoop.current.run(until: Date().addingTimeInterval(0.25))
    if let frame = NSScreen.main?.visibleFrame,
       frame.width >= 1680, frame.height >= 820 {
        print("CI visible desktop: \(Int(frame.width))x\(Int(frame.height))")
        exit(0)
    }
} while Date() < deadline
fail("CI visible desktop is still too small: \(String(describing: NSScreen.main?.visibleFrame))")
