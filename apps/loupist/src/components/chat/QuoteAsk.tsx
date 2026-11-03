import i18n, { useTranslation } from "@/i18n";
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import type { KnowledgeSourceSelectionRequest } from "../../types";

const t = i18n.getFixedT(null, "chat");
const MIN_WORDS = 3;
/** Scripts without spaces between words still count as a phrase at this length. */
const MIN_CHARS = 12;

type Anchor = { left: number; top: number; messageId: string; text: string; retained?: KnowledgeSourceSelectionRequest; portal: HTMLElement };

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
  const rects = typeof range.getClientRects === "function" ? range.getClientRects() : [];
  const end = rects.length ? rects[rects.length - 1]! : typeof range.getBoundingClientRect === "function" ? range.getBoundingClientRect() : content.getBoundingClientRect();
  const reference = content.getAttribute("data-quote-retained-source");
  let retained: KnowledgeSourceSelectionRequest | undefined;
  if (reference) {
    const prefix = range.cloneRange();
    prefix.selectNodeContents(content);
    prefix.setEnd(range.startContainer, range.startOffset);
    const startUtf16 = prefix.toString().length;
    retained = { source: JSON.parse(reference), selection: { startUtf16, endUtf16: startUtf16 + range.toString().length, text: range.toString() } };
  }
  return { left: end.right, top: end.top, messageId: found.id, text, retained, portal: content.closest<HTMLElement>(".workspace-panel") ?? document.body };
}

/**
 * Quote to ask (chat spec X3, panel spec P4): selecting a phrase inside an
 * answer, or inside a panel reader, shows one button just above the
 * selection's end ("Ask about this" / "Add to question"). It adds the phrase
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
  const left = Math.max(8, Math.min(anchor.left - 60, window.innerWidth - 160));
  const top = Math.max(8, anchor.top - 40);
  return createPortal(
    <button type="button" className="quote-ask" style={{ left, top }}
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
