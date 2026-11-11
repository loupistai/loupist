// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type BrowserBounds = { x: number, y: number, width: number, height: number };

export type BrowserHistoryDirection = "back" | "forward";

export type BrowserTabBoundsRequest = { tab_id: string, bounds: BrowserBounds, visible: boolean };

export type BrowserTabEvent =
  | { "type": "load_started", tab_id: string, url: string }
  | { "type": "load_finished", tab_id: string, url: string }
  | { "type": "title_changed", tab_id: string, title: string }
  | { "type": "open_requested", tab_id: string, url: string }
  | { "type": "navigation_blocked", tab_id: string, reason: BrowserUrlError }
  | { "type": "download_blocked", tab_id: string, url: string };

export type BrowserTabHistoryRequest = { tab_id: string, direction: BrowserHistoryDirection };

export type BrowserTabNavigateRequest = { tab_id: string, url: string };

export type BrowserTabOpenRequest = { url: string, bounds: BrowserBounds, visible: boolean };

export type BrowserTabRequest = { tab_id: string };

export type BrowserTabState = { tab_id: string, url: string, title: string, loading: boolean };

export type BrowserUrlError = "too_long" | "invalid" | "unsupported_protocol" | "credentials";
