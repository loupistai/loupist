import { memo, useLayoutEffect, useRef } from "react";
import { Message, MessageContent } from "@/components/ai-elements/message";
import { useConversationScroll } from "../ai-elements/conversation";
import { stableMarkdown } from "../../chat/streamingMarkdown";
import type { ProseTone } from "../../chat/turnView";
import { usePacedText } from "../../chat/usePacedText";
import type { OpenLinkOptions } from "../../chat/links";
import type { ChatEvidenceRef, ChatMessage, MemoryRef } from "../../types";
import type { EvidenceCell } from "./NumberChip";
import { SafeMessageResponse } from "./SafeMessageResponse";

/** Where a prose link, citation or number goes. The card owns the policy. */
export type ProseLinks = {
  onOpenLink: (url: string, options?: OpenLinkOptions) => Promise<void>;
  onOpenFile?: (path: string) => void;
  onLensLink: (recordId: string) => void;
  onOpenEvidence: (evidence: ChatEvidenceRef, anchor?: HTMLElement) => void;
  onOpenEvidenceCell: (evidence: ChatEvidenceRef, cell?: EvidenceCell) => void;
  onShowStep: (evidence: ChatEvidenceRef) => void;
};

type Props = {
  text: string;
  streaming: boolean;
  tone: ProseTone;
  checks?: ChatMessage["number_checks"];
  evidenceRefs?: ChatEvidenceRef[];
  memoryRefs?: MemoryRef[];
  asOf?: string;
  workspacePath?: string;
  links: ProseLinks;
};

const sameLinks = (previous: Props, next: Props) =>
  previous.text === next.text && previous.streaming === next.streaming && previous.tone === next.tone && previous.checks === next.checks
  && previous.evidenceRefs === next.evidenceRefs && previous.memoryRefs === next.memoryRefs && previous.asOf === next.asOf
  && previous.workspacePath === next.workspacePath;

/**
 * One run of answer or narration prose. Text is paced first, then held back
 * where a half-written construct would render wrong, so a released table is
 * never revealed word by word. Finished segments stop animating and keep
 * their rendered blocks.
 */
export const ProseSegment = memo(function ProseSegment({ text, streaming, tone, checks, evidenceRefs, memoryRefs, asOf, workspacePath, links }: Props) {
  // The card hands over fresh closures each render; retained Markdown links
  // call through this ref so finished blocks never re-render for them.
  const latest = useRef(links);
  latest.current = links;
  const stable = useRef<ProseLinks>({
    onOpenLink: (url, options) => options ? latest.current.onOpenLink(url, options) : latest.current.onOpenLink(url),
    onOpenFile: (path) => latest.current.onOpenFile?.(path),
    onLensLink: (recordId) => latest.current.onLensLink(recordId),
    onOpenEvidence: (evidence, anchor) => latest.current.onOpenEvidence(evidence, anchor),
    onOpenEvidenceCell: (evidence, cell) => latest.current.onOpenEvidenceCell(evidence, cell),
    onShowStep: (evidence) => latest.current.onShowStep(evidence),
  }).current;
  const paced = usePacedText(text, streaming);
  const { text: visible, caret } = stableMarkdown(paced, streaming);
  const conversation = useConversationScroll();
  useLayoutEffect(() => { if (streaming) conversation?.followGrowth(); }, [conversation, streaming, visible]);
  return (
    <Message from="assistant" className="max-w-full">
      <MessageContent className={`message-content overflow-visible${tone === "narration" ? " message-narration" : ""}`}>
        <SafeMessageResponse text={visible} isAnimating={streaming} caret={caret} evidenceRefs={evidenceRefs} memoryRefs={memoryRefs}
          numberChecks={checks} asOf={asOf} onOpenEvidence={stable.onOpenEvidence} onOpenEvidenceCell={stable.onOpenEvidenceCell}
          onShowStep={stable.onShowStep} onOpenLink={stable.onOpenLink} onOpenFile={links.onOpenFile ? stable.onOpenFile : undefined}
          onLensLink={stable.onLensLink} workspacePath={workspacePath} />
      </MessageContent>
    </Message>
  );
}, sameLinks);
