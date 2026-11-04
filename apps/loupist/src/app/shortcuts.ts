/**
 * The app's keyboard shortcuts, in one place: Settings › Keyboard lists this
 * table, and the global handlers read their keys from it, so the page cannot
 * drift from what the keys actually do.
 */
export type ShortcutId =
  | "newChat" | "palette" | "tabs" | "panel" | "settings"
  | "send" | "newLine" | "guide" | "stop" | "close" | "focusComposer";

export type Shortcut = { id: ShortcutId; mac: string; other: string };

export const SHORTCUTS: readonly Shortcut[] = [
  { id: "newChat", mac: "⌘N", other: "Ctrl+N" },
  { id: "palette", mac: "⌘K", other: "Ctrl+K" },
  { id: "tabs", mac: "⌘1 · ⌘2 · ⌘3 · ⌘4", other: "Ctrl+1 · Ctrl+2 · Ctrl+3 · Ctrl+4" },
  { id: "panel", mac: "⌘\\", other: "Ctrl+\\" },
  { id: "settings", mac: "⌘,", other: "Ctrl+," },
  { id: "send", mac: "Enter", other: "Enter" },
  { id: "newLine", mac: "Shift+Enter", other: "Shift+Enter" },
  { id: "guide", mac: "⌘Enter", other: "Ctrl+Enter" },
  { id: "stop", mac: "Esc", other: "Esc" },
  { id: "close", mac: "Esc", other: "Esc" },
  { id: "focusComposer", mac: "/", other: "/" },
];

/** The single-key part of the global shortcuts the app handles itself. */
export const GLOBAL_KEYS = { newChat: "n", palette: "k", settings: ",", panel: "\\", focusComposer: "/" } as const;

export const isMacPlatform = () => /Mac|iPhone|iPad/.test(navigator.platform);

/** A real modal is open. The narrow navigation sheet is a dialog too, and it must not swallow app shortcuts. */
export const modalDialogOpen = () => [...document.querySelectorAll('[role="dialog"][data-state="open"]')]
  .some((dialog) => dialog.getAttribute("data-sidebar") !== "sidebar");
export const shortcutKeys = (shortcut: Shortcut, mac = isMacPlatform()) => (mac ? shortcut.mac : shortcut.other);
export const shortcutFor = (id: ShortcutId) => shortcutKeys(SHORTCUTS.find((item) => item.id === id)!);
/** One tab's key from the ⌘1–⌘4 binding, for rows that name a single tab. */
export const tabShortcut = (index: 1 | 2 | 3 | 4, mac = isMacPlatform()) => (mac ? `⌘${index}` : `Ctrl+${index}`);
