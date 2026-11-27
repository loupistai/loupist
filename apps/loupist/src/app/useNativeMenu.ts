import { useEffect, useRef } from "react";
import type { AppMenuContext, MenuLanguage } from "../contracts/generated";
import { primaryLanguage } from "../i18n/localized";
import type { LoupistBridge } from "../types";
import { modalDialogOpen } from "./shortcuts";

type State = Omit<AppMenuContext, "modal">;

/** The languages the host words its menu in. */
const MENU_LANGUAGES: readonly MenuLanguage[] = ["en", "ko"];

/** The menu's language for the interface language: the same one when the host has it, else English. */
export const menuLanguage = (language: string): MenuLanguage => MENU_LANGUAGES.find(menu => menu === primaryLanguage(language)) ?? "en";

export function useNativeMenu(bridge: LoupistBridge, context: State, onAction: (action: string) => void) {
  const handler = useRef(onAction);
  handler.current = onAction;
  useEffect(() => bridge.onMenuAction(action => {
    if (!modalDialogOpen()) handler.current(action);
  }), [bridge]);

  const { language, shell, loupist, print, sidebar } = context;
  useEffect(() => {
    let previous: boolean | undefined;
    const sync = (force = false) => {
      const modal = modalDialogOpen();
      if (!force && modal === previous) return;
      previous = modal;
      void bridge.appMenuUpdate({ language, shell, loupist, print, sidebar, modal }).catch(() => {
        if (previous === modal) previous = undefined;
      });
    };
    const observer = new MutationObserver(() => sync());
    observer.observe(document.body, {
      attributes: true,
      attributeFilter: ["role", "aria-modal", "data-state", "data-sidebar", "hidden"],
      childList: true,
      subtree: true,
    });
    const resync = () => sync(true);
    const pickerSettled = (event: Event) => {
      if (event.target instanceof HTMLInputElement && event.target.type === "file") resync();
    };
    window.addEventListener("focus", resync);
    window.addEventListener("pageshow", resync);
    document.addEventListener("change", pickerSettled, true);
    document.addEventListener("cancel", pickerSettled, true);
    sync();
    return () => {
      observer.disconnect();
      window.removeEventListener("focus", resync);
      window.removeEventListener("pageshow", resync);
      document.removeEventListener("change", pickerSettled, true);
      document.removeEventListener("cancel", pickerSettled, true);
    };
  }, [bridge, language, shell, loupist, print, sidebar]);
}
