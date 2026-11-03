import { useSyncExternalStore } from "react";

/**
 * How the user last reached a control: a press or a key. WebKit counts focus that a
 * script moves as keyboard focus when a text field (the composer) had it, so the
 * focus ring would follow a mouse click. Controls that move focus or style their
 * ring ask this first; keys still get the ring and the moved focus.
 */
export type InputModality = "pointer" | "keyboard";

let modality: InputModality = "pointer";
const listeners = new Set<() => void>();
let installed = false;

const set = (next: InputModality) => {
  if (next === modality) return;
  modality = next;
  for (const listener of listeners) listener();
};

function install() {
  if (installed || typeof document === "undefined") return;
  installed = true;
  document.addEventListener("pointerdown", () => set("pointer"), true);
  // Shortcuts with a modifier are not keyboard navigation.
  document.addEventListener("keydown", (event) => { if (!event.metaKey && !event.ctrlKey && !event.altKey) set("keyboard"); }, true);
}
// Listen from the start: the key that opens something must already count when it asks.
install();

const subscribe = (listener: () => void) => {
  install();
  listeners.add(listener);
  return () => { listeners.delete(listener); };
};

export function inputModality(): InputModality {
  install();
  return modality;
}

export function useInputModality(): InputModality {
  return useSyncExternalStore(subscribe, inputModality, () => "pointer");
}

/** Moves focus for keyboard users; after a press, focus stays where the press left it. */
export function focusForKeyboard(element: HTMLElement | null | undefined) {
  if (inputModality() === "keyboard") element?.focus({ preventScroll: true });
}
