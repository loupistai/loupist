import "@/styles/print.css";
import { createRoot, type Root } from "react-dom/client";
import { flushSync } from "react-dom";
import type { ReactNode } from "react";

type Sheet = { element: HTMLElement; root?: Root; title: string; end: () => void };
let current: Sheet | null = null;

const nextFrame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

/** Removes the print sheet: the app prints as itself again and the window keeps its title. */
export function endPrint() {
  const sheet = current;
  if (!sheet) return;
  current = null;
  sheet.end();
  sheet.root?.unmount();
  sheet.element.remove();
  delete document.documentElement.dataset.printing;
  document.title = sheet.title;
}

/** Waits until the sheet's fonts and pictures are ready, so the first page is not drawn half loaded. */
async function settled(element: HTMLElement) {
  await nextFrame();
  await nextFrame();
  await Promise.all([...element.querySelectorAll("img")].map((image) => image.decode?.().catch(() => undefined)));
  await document.fonts?.ready;
}

/**
 * Prints one prepared view through the webview's print dialog, which offers
 * "Save as PDF" on macOS (shell-briefings §2.6, X9). The view is a sheet
 * outside the app, styled for paper in the light theme; in print media only
 * the sheet shows, so nothing on screen changes. `title` names the PDF.
 *
 * WebKit draws the pages while its print sheet is open and `window.print()`
 * returns at once there, so the sheet stays until the dialog has closed:
 * `afterprint`, or the next press or key in the app, removes it.
 */
export async function printView(title: string, content: ReactNode | HTMLElement) {
  endPrint();
  const element = document.createElement("div");
  element.className = "print-sheet";
  document.body.append(element);
  let root: Root | undefined;
  if (content instanceof HTMLElement) element.append(content);
  else {
    root = createRoot(element);
    flushSync(() => root!.render(content));
  }
  const end = () => {
    window.removeEventListener("afterprint", endPrint);
    window.removeEventListener("pointerdown", endPrint, true);
    window.removeEventListener("keydown", endPrint, true);
  };
  current = { element, root, title: document.title, end };
  try {
    await settled(element);
    document.documentElement.dataset.printing = "";
    document.title = title;
    window.addEventListener("afterprint", endPrint);
    window.addEventListener("pointerdown", endPrint, true);
    window.addEventListener("keydown", endPrint, true);
    // macOS: Tauri routes this to the native print panel and resolves when it is shown.
    await window.print();
  } catch (cause) {
    endPrint();
    throw cause;
  }
}
