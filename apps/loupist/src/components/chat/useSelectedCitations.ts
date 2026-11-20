import { useEffect } from "react";

const CITATIONS = ".message-content .loupist-citation";

/**
 * A citation number is a small box with its own fill. Inside a text selection it is
 * marked `data-in-selection` and takes the selection's colour (evidence.css), so the
 * selected run reads as one piece instead of text with boxes in it.
 */
export function useSelectedCitations(active: boolean) {
  useEffect(() => {
    if (!active) return;
    let marked: Element[] = [];
    const update = () => {
      const selection = document.getSelection();
      const range = selection && !selection.isCollapsed && selection.rangeCount ? selection.getRangeAt(0) : null;
      const common = range?.commonAncestorContainer;
      const scope = common instanceof Element ? common : common?.parentElement;
      const next = range && scope ? [...scope.querySelectorAll(CITATIONS)].filter((chip) => range.intersectsNode(chip)) : [];
      for (const chip of marked) if (!next.includes(chip)) chip.removeAttribute("data-in-selection");
      for (const chip of next) chip.setAttribute("data-in-selection", "");
      marked = next;
    };
    document.addEventListener("selectionchange", update);
    return () => {
      document.removeEventListener("selectionchange", update);
      for (const chip of marked) chip.removeAttribute("data-in-selection");
    };
  }, [active]);
}
