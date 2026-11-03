# Loupist E2E patch

Source: the published `tauri-plugin-wdio-webdriver` 1.3.0 crate, from
https://crates.io/crates/tauri-plugin-wdio-webdriver/1.3.0 .
Published archive SHA-256:
`7ad665bda48567107b6a3d070fe6d58a98e21f3ebf52abe41308fa4d5919f61b`.
Upstream MIT LICENSE is retained verbatim. No upstream dependency was added.
The app Cargo.lock pins the complete graph; this is an app-local workspace
member so its tests use that same graph, with the app as the default member.

Current change: WebDriver browsing contexts resolve through Tauri `Webview`
handles, whose owner `Window` is used only for window geometry/management.
Tauri `webview_windows()` intentionally omits a window after an independently
labelled child is added. Resolving a live main context through that collection
therefore produced `no such window`. Main is ordered first for an unspecified
capability; repository sessions additionally request `windowLabel: main`.
Closing a child closes only that webview. Existing native evaluation, framing,
script locks, event handling, loopback binding, and IPC authority are unchanged.

Switching context also shows, restores and focuses its native owner window and
focuses the selected webview. Changing only the protocol handle left macOS
webviews occluded, allowing DOM execution while rendering remained suspended.
The native suites verify document focus and visibility before exercising UI.
Clicking a native select option applies selection and emits input/change events;
`HTMLElement.click()` alone has no selection default action. Disabled options,
groups and selects retain their value. Configuration persistence exercises this
through real scope and retry controls in the webview.

Desktop window geometry also uses WebDriver logical/CSS pixels consistently.
The upstream executor returned raw Tauri physical sizes and submitted
`PhysicalSize`/`PhysicalPosition`, so a 700×560 request on a 2× Retina display
made a 350×280 logical window while reporting 700×560. Get now converts position
and outer size with the current display scale; Set uses `LogicalPosition` and
`LogicalSize`, subtracting converted window decorations without premature
rounding. Integer response coordinates use dpi's rounded conversions. This
shared path covers Get/Set Window Rect, WebdriverIO size operations, maximize
and fullscreen responses. Errors reading or applying geometry are reported.

The PNG snapshot path is unchanged. A Retina image may have twice the logical
width and height; its pixel dimensions do not establish the CSS viewport size.
Native narrow-window qualification must check the logical window and CSS
viewport together. The previous 700×560-physical run was not a 700×560-logical
layout qualification.

Verification:
`cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml -p tauri-plugin-wdio-webdriver --locked`
checks actual Tauri MockRuntime main/child lookup and independent child closing,
along with upstream behavioral tests. Executor geometry tests cover 1×, Retina
2× and fractional display scales, decoration subtraction and rounding. Real
external navigation, back and reload
must additionally be verified in the isolated native app.

This dependency is optional for `webdriver`/`e2e`; release builds reject those
features. The production browser authority remains in `src/browser.rs` and the
main-only capability. Remote child views do not gain host command permission.
