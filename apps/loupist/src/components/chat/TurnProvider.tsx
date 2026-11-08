import { createContext, useCallback, useContext, useMemo, useRef, useState, type ReactNode } from "react";
import type { ChatEvidenceRef, ChatMessage, MemoryRef } from "../../types";
import { evidenceTarget } from "../../chat/navigation";
import type { EvidenceStepLink } from "../../chat/workspace";
import { OpenProvider, useOpenLayer, type OpenLayer } from "../navigation/Open";
import { useConversationScroll } from "../ai-elements/conversation";
import { QuoteEvidencePopover } from "./QuoteEvidence";

type RevealedResult = { id: string; request: number };

type Turn = {
  evidenceRefs: ChatEvidenceRef[];
  memoryRefs?: MemoryRef[];
  /** The answer's as-of date; every number popover says "Known as of". */
  asOf?: string;
  /** The execution result a citation or "Show step" asked to reveal inline. */
  revealedResult: RevealedResult | null;
};

const TurnContext = createContext<Turn | null>(null);
const EMPTY: ChatEvidenceRef[] = [];

/** The answer around a citation. Null outside one: a link then has no evidence to resolve. */
export const useTurn = () => useContext(TurnContext);

function stepFor(message: ChatMessage, evidence: ChatEvidenceRef, cell: EvidenceStepLink["cell"]): EvidenceStepLink | undefined {
  const step = message.progress?.items.find((entry) => entry.kind === "tool" && entry.resultIds?.includes(evidence.id));
  const link = step?.kind === "tool" ? { label: step.action, itemId: step.id } : undefined;
  return cell ? { label: link?.label ?? "", itemId: link?.itemId, cell } : link;
}

/**
 * One answer's open policy. A citation resolves here, where the message's Memory reads and
 * steps are known: Memory opens in the Evidence reader, execution output reveals its step in
 * the thread, a quote basket opens its popover, and everything else goes up with its step.
 */
export function TurnProvider({ message, loupistId, children }: { message: ChatMessage; loupistId: string; children: ReactNode }) {
  const conversation = useConversationScroll();
  const [revealedResult, setRevealedResult] = useState<RevealedResult | null>(null);
  const [quote, setQuote] = useState<{ evidence: ChatEvidenceRef; anchor: HTMLElement; pointer?: string } | null>(null);
  // Read through a ref so the opener stays stable while the answer streams.
  const latest = useRef(message);
  latest.current = message;

  const open = useOpenLayer(useCallback<OpenLayer>((to, how) => {
    if (to.kind !== "evidence") return undefined;
    const current = latest.current;
    const target = evidenceTarget(to.evidence, current.memory_refs, loupistId);
    switch (target.kind) {
      case "memory": return { kind: "memory", location: target.location, title: target.title };
      case "asset": return { kind: "asset", target: target.target };
      case "step": return () => {
        conversation?.holdScrollPosition();
        setRevealedResult((previous) => ({ id: to.evidence.id, request: (previous?.request ?? 0) + 1 }));
      };
      case "quote": return () => {
        // The clicked control, not document.activeElement: a mouse click in WKWebView
        // can leave focus on the page, and a snapshot of that rectangle opens off screen.
        if (how.anchor) setQuote({ evidence: to.evidence, anchor: how.anchor, pointer: to.cell?.pointer });
      };
      case "browser":
      case "reader": return { ...to, step: to.step ?? stepFor(current, to.evidence, to.cell) };
    }
  }, [conversation, loupistId]));

  const turn = useMemo<Turn>(() => ({
    evidenceRefs: message.evidence_refs ?? EMPTY,
    memoryRefs: message.memory_refs,
    asOf: message.as_of,
    revealedResult,
  }), [message.evidence_refs, message.memory_refs, message.as_of, revealedResult]);

  return <TurnContext.Provider value={turn}>
    <OpenProvider open={open}>
      {children}
      {quote ? <QuoteEvidencePopover evidence={quote.evidence} anchor={quote.anchor} pointer={quote.pointer} onClose={() => setQuote(null)} /> : null}
    </OpenProvider>
  </TurnContext.Provider>;
}
