import i18n, { useTranslation } from "@/i18n";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { KnowledgeSourceSelectionRequest } from "../../types";

const t = i18n.getFixedT(null, "chat");
const MIN_WORDS = 3;
/** Scripts without spaces between words still count as a phrase at this length. */
const MIN_CHARS = 12;
/** Space between the selected text and the button. */
const GAP_PX = 6;
/** The button keeps this far inside the visible thread or reader. */
const EDGE_PX = 8;

/** A box in viewport coordinates. */
type Box = { top: number; bottom: number; left: number; right: number };
/** The selected text's first and last lines. */
type SelectedLines = { first: Box; last: Box };

type Anchor = { lines: SelectedLines; bounds: Box; messageId: string; text: string; retained?: KnowledgeSourceSelectionRequest; portal: HTMLElement };

type Scope = "answer" | "reader";

/** Where a quotable selection may start: an answer's body, or a panel reader marked `data-quote-source`. */
function quotableContainer(element: Element | null | undefined, scope: Scope) {
  if (scope === "answer") {
    const content = element?.closest(".message.assistant .message-content");
    const id = content?.closest("[data-message-id]")?.getAttribute("data-message-id");
    return content && id ? { content, id } : null;
  }
  const content = element?.closest("[data-quote-source]");
  const id = content?.getAttribute("data-quote-source");
  return content && id ? { content, id } : null;
}

const toBox = ({ top, bottom, left, right }: Box): Box => ({ top, bottom, left, right });

/** The rectangles of the selected text itself, not the boxes of whole elements a range also reports. */
function textRects(range: Range): DOMRect[] {
  const root = range.commonAncestorContainer;
  const texts: Node[] = [];
  if (root.nodeType === Node.TEXT_NODE) texts.push(root);
  else {
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) if (range.intersectsNode(node)) texts.push(node);
  }
  return texts.flatMap((text) => {
    const part = document.createRange();
    part.selectNodeContents(text);
    if (text === range.startContainer) part.setStart(text, range.startOffset);
    if (text === range.endContainer) part.setEnd(text, range.endOffset);
    return [...part.getClientRects()].filter((rect) => rect.width > 0 && rect.height > 0);
  });
}

/** The first and last lines of the selection; the rectangles on one line overlap by at least half their height. */
export function selectedLines(rects: readonly Box[]): SelectedLines | null {
  if (!rects.length) return null;
  const top = Math.min(...rects.map((rect) => rect.top));
  const bottom = Math.max(...rects.map((rect) => rect.bottom));
  const line = (rows: readonly Box[]): Box => ({
    top: Math.min(...rows.map((rect) => rect.top)), bottom: Math.max(...rows.map((rect) => rect.bottom)),
    left: Math.min(...rows.map((rect) => rect.left)), right: Math.max(...rows.map((rect) => rect.right)),
  });
  return {
    first: line(rects.filter((rect) => rect.top < top + (rect.bottom - rect.top) / 2)),
    last: line(rects.filter((rect) => rect.bottom > bottom - (rect.bottom - rect.top) / 2)),
  };
}

/**
 * Where the button goes: just under the selection's last line, centred on where the
 * selection ends; over its first line when there is no room below; inside the bounds
 * either way. It never sits on the selected lines unless they fill the bounds.
 */
export function quoteAskPosition({ first, last }: SelectedLines, size: { width: number; height: number }, bounds: Box) {
  const room = { top: bounds.top + EDGE_PX, bottom: bounds.bottom - EDGE_PX, left: bounds.left + EDGE_PX, right: bounds.right - EDGE_PX };
  const below = last.bottom + GAP_PX;
  const above = first.top - GAP_PX - size.height;
  const under = below + size.height <= room.bottom || above < room.top;
  const top = under ? Math.max(room.top, Math.min(below, room.bottom - size.height)) : above;
  const x = under ? last.right - size.width / 2 : first.left;
  return { top, left: Math.max(room.left, Math.min(x, room.right - size.width)) };
}

/** The visible part of the scrolling thread or reader that holds the selection, inside the window. */
function visibleBounds(content: Element): Box {
  const viewport = { top: 0, left: 0, right: window.innerWidth, bottom: window.innerHeight };
  for (let node = content.parentElement; node; node = node.parentElement) {
    const { overflowY } = getComputedStyle(node);
    if (overflowY !== "auto" && overflowY !== "scroll") continue;
    const box = node.getBoundingClientRect();
    return { top: Math.max(viewport.top, box.top), bottom: Math.min(viewport.bottom, box.bottom), left: Math.max(viewport.left, box.left), right: Math.min(viewport.right, box.right) };
  }
  return viewport;
}

function selectionIn(scope: Scope): Anchor | null {
  const selection = document.getSelection();
  if (!selection || selection.isCollapsed || !selection.rangeCount) return null;
  const range = selection.getRangeAt(0);
  const node = range.commonAncestorContainer;
  const found = quotableContainer(node instanceof Element ? node : node.parentElement, scope);
  if (!found) return null;
  const { content } = found;
  const text = selection.toString().replace(/\s+/gu, " ").trim();
  if (text.split(" ").length < MIN_WORDS && [...text].length < MIN_CHARS) return null;
  // Layout-free environments lack range geometry; fall back to the answer's box.
  const lines = (typeof range.getClientRects === "function" ? selectedLines(textRects(range).map(toBox)) : null)
    ?? { first: toBox(content.getBoundingClientRect()), last: toBox(content.getBoundingClientRect()) };
  const reference = content.getAttribute("data-quote-retained-source");
  let retained: KnowledgeSourceSelectionRequest | undefined;
  if (reference) {
    const prefix = range.cloneRange();
    prefix.selectNodeContents(content);
    prefix.setEnd(range.startContainer, range.startOffset);
    const startUtf16 = prefix.toString().length;
    retained = { source: JSON.parse(reference), selection: { startUtf16, endUtf16: startUtf16 + range.toString().length, text: range.toString() } };
  }
  return { lines, bounds: visibleBounds(content), messageId: found.id, text, retained, portal: content.closest<HTMLElement>(".workspace-panel") ?? document.body };
}

/**
 * Quote to ask (chat spec X3, panel spec P4): selecting a phrase inside an
 * answer, or inside a panel reader, shows one button just under the end of the
 * selection, or over its start near the bottom of the view ("Ask about this" /
 * "Add to question"); it never covers the selected text. It adds the phrase
 * as a quote chip, or resolves a retained Source selection through the host,
 * and focuses the composer; it never sends by itself.
 */
export function QuoteAsk({ onQuote, scope = "answer" }: {
  /** Receives the answer's message id, or the reader's `data-quote-source`, and the selected text. */
  onQuote: (sourceId: string, text: string, retained?: KnowledgeSourceSelectionRequest) => void | Promise<void>;
  /** "reader": selections in panel readers (Evidence, files) become "Add to question". */
  scope?: Scope;
}) {
  useTranslation("chat");
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const [anchor, setAnchor] = useState<Anchor | null>(null);
  const button = useRef<HTMLButtonElement>(null);
  // The button's own size places it; until it has been measured, a typical one does.
  const [size, setSize] = useState({ width: 120, height: 28 });
  // Measured when the button appears or its label changes (a retry), before it is painted.
  useLayoutEffect(() => {
    const node = button.current;
    if (!node) return;
    const width = node.offsetWidth, height = node.offsetHeight;
    if (width && height && (width !== size.width || height !== size.height)) setSize({ width, height });
  }, [anchor, failed, size.width, size.height]);
  useEffect(() => {
    // Wait for the pointer or keyboard to finish selecting, then read the result once.
    const settle = () => window.setTimeout(() => { setAnchor(selectionIn(scope)); setFailed(false); }, 0);
    const clearIfCollapsed = () => { if (document.getSelection()?.isCollapsed) setAnchor(null); };
    // Scrolling or resizing moves the selection; the button follows it rather than vanishing.
    const follow = () => setAnchor(selectionIn(scope));
    document.addEventListener("mouseup", settle);
    document.addEventListener("keyup", settle);
    document.addEventListener("selectionchange", clearIfCollapsed);
    window.addEventListener("scroll", follow, true);
    window.addEventListener("resize", follow);
    return () => {
      document.removeEventListener("mouseup", settle);
      document.removeEventListener("keyup", settle);
      document.removeEventListener("selectionchange", clearIfCollapsed);
      window.removeEventListener("scroll", follow, true);
      window.removeEventListener("resize", follow);
    };
  }, [scope]);
  if (!anchor) return null;
  const { left, top } = quoteAskPosition(anchor.lines, size, anchor.bounds);
  return createPortal(
    <button ref={button} type="button" className="quote-ask" style={{ left, top }}
      // Keep the selection while the button is pressed.
      onMouseDown={(event) => event.preventDefault()}
      disabled={busy}
      title={failed ? t("chatConversation.couldNotAddSelection") : undefined}
      onClick={async () => {
        setBusy(true); setFailed(false);
        try {
          await (anchor.retained ? onQuote(anchor.messageId, anchor.text, anchor.retained) : onQuote(anchor.messageId, anchor.text));
          setAnchor(null); document.getSelection()?.removeAllRanges();
        } catch { setFailed(true); } finally { setBusy(false); }
      }}>
      {failed ? t("chatConversation.retrySelection") : scope === "answer" ? t("chatConversation.askAboutThis") : t("chatConversation.addToQuestion")}
    </button>,
    anchor.portal,
  );
}
