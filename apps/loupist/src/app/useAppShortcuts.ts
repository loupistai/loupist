import { useEffect } from "react";
import { GLOBAL_KEYS, modalDialogOpen } from "./shortcuts";

/** Something that takes text (or a menu or dialog) has focus, so a bare "/" belongs to it. */
const typingTarget = (target: EventTarget | null) => target instanceof HTMLElement
  && (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)
    || target.closest('[role="dialog"], [role="menu"], [role="listbox"]') !== null);

export function useAppShortcuts({ onSettings, onNewChat, onPalette, canCreateChat }: {
  onSettings: () => void;
  onNewChat: () => void;
  onPalette?: () => void;
  canCreateChat: boolean;
}) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat || event.isComposing || event.keyCode === 229 || event.altKey) return;
      if (event.key === GLOBAL_KEYS.focusComposer && !event.metaKey && !event.ctrlKey && !event.shiftKey && !typingTarget(event.target)) {
        const composer = document.querySelector<HTMLTextAreaElement>("[data-chat-composer]");
        if (composer) { event.preventDefault(); composer.focus(); }
        return;
      }
      if (event.shiftKey || !(event.metaKey || event.ctrlKey) || modalDialogOpen()) return;
      const key = event.key.toLowerCase();
      if (key === GLOBAL_KEYS.settings) {
        event.preventDefault();
        onSettings();
      } else if (key === GLOBAL_KEYS.newChat && canCreateChat) {
        event.preventDefault();
        onNewChat();
      } else if (key === GLOBAL_KEYS.palette && onPalette) {
        event.preventDefault();
        onPalette();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [canCreateChat, onNewChat, onPalette, onSettings]);
}
