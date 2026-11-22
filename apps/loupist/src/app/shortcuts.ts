import { APP_DESTINATIONS } from "./navigation";
import { menuShortcut, menuShortcutKey } from "./nativeMenu";
import { isMacPlatform } from "../lib/platform";
export { isMacPlatform } from "../lib/platform";

/** One destination's key from the ⌘1–⌘n binding, for rows that name a single destination. */
export const tabShortcut = (index: number, mac = isMacPlatform()) => menuShortcut(APP_DESTINATIONS[index - 1]?.id ?? "", mac);
const destinationKeys = (mac: boolean) => APP_DESTINATIONS.map((_, index) => tabShortcut(index + 1, mac)).join(" · ");

/**
 * The app's keyboard shortcuts, in one place: Settings › Keyboard lists this
 * table, and the global handlers read their keys from it, so the page cannot
 * drift from what the keys actually do.
 */
export type ShortcutId =
  | "newChat" | "palette" | "tabs" | "panel" | "settings"
  | "sidebar" | "printChat" | "closeWindow" | "minimize" | "hideApp" | "quit" | "zoomIn" | "zoomOut" | "actualSize" | "fullscreen"
  | "send" | "newLine" | "guide" | "stop" | "close" | "focusComposer";

export type Shortcut = { id: ShortcutId; mac: string; other: string };

const fromMenu = (id: ShortcutId, action: string): Shortcut => ({ id, mac: menuShortcut(action, true), other: menuShortcut(action, false) });

export const SHORTCUTS: readonly Shortcut[] = [
  fromMenu("newChat", "new-chat"),
  { id: "palette", mac: "⌘K", other: "Ctrl+K" },
  { id: "tabs", mac: destinationKeys(true), other: destinationKeys(false) },
  { id: "panel", mac: "⌘\\", other: "Ctrl+\\" },
  fromMenu("settings", "settings"),
  fromMenu("sidebar", "toggle-sidebar"),
  fromMenu("printChat", "print-chat"),
  fromMenu("closeWindow", "close-window"),
  fromMenu("minimize", "minimize"),
  fromMenu("hideApp", "hide"),
  fromMenu("quit", "quit"),
  fromMenu("zoomIn", "zoom-in"),
  fromMenu("zoomOut", "zoom-out"),
  fromMenu("actualSize", "actual-size"),
  fromMenu("fullscreen", "fullscreen"),
  { id: "send", mac: "Enter", other: "Enter" },
  { id: "newLine", mac: "Shift+Enter", other: "Shift+Enter" },
  { id: "guide", mac: "⌘Enter", other: "Ctrl+Enter" },
  { id: "stop", mac: "Esc", other: "Esc" },
  { id: "close", mac: "Esc", other: "Esc" },
  { id: "focusComposer", mac: "/", other: "/" },
];

/** The single-key part of the global shortcuts the app handles itself. */
export const GLOBAL_KEYS = { newChat: menuShortcutKey("new-chat"), palette: "k", settings: menuShortcutKey("settings"), sidebar: menuShortcutKey("toggle-sidebar"), panel: "\\", focusComposer: "/" } as const;

/** A real modal is open. The narrow navigation sheet is a dialog too, and it must not swallow app shortcuts. */
export const modalDialogOpen = () => Boolean(document.querySelector(
  ':is([role="dialog"], [role="alertdialog"]):is([data-state="open"], [aria-modal="true"]):not([aria-modal="false"], [data-sidebar="sidebar"], [hidden], [hidden] *)',
));
export const shortcutKeys = (shortcut: Shortcut, mac = isMacPlatform()) => (mac ? shortcut.mac : shortcut.other);
export const shortcutFor = (id: ShortcutId) => shortcutKeys(SHORTCUTS.find((item) => item.id === id)!);
