import { memo, useLayoutEffect } from "react";
import { Message, MessageContent } from "@/components/ai-elements/message";
import { useConversationScroll } from "../ai-elements/conversation";
import { stableMarkdown } from "../../chat/streamingMarkdown";
import type { ProseTone } from "../../chat/turnView";
import { usePacedText } from "../../chat/usePacedText";
import type { ChatMessage } from "../../types";
import { SafeMessageResponse } from "./SafeMessageResponse";

/** A numbered citation never starts a line on its own: the space before it does not break. */
const CITATION_GAP = /[ \t]+(\[\d{1,3}\]\(#evidence-)/gu;
const bindCitations = (text: string) => text.replace(CITATION_GAP, "\u00a0$1");

type Props = {
  text: string;
  streaming: boolean;
  tone: ProseTone;
  checks?: ChatMessage["number_checks"];
  workspacePath?: string;
};

/**
 * One run of answer or narration prose. Text is paced first, then held back
 * where a half-written construct would render wrong, so a released table is
 * never revealed word by word. Finished segments stop animating and keep
 * their rendered blocks.
 */
export const ProseSegment = memo(function ProseSegment({ text, streaming, tone, checks, workspacePath }: Props) {
  const paced = usePacedText(text, streaming);
  const { text: visible, caret } = stableMarkdown(paced, streaming);
  const conversation = useConversationScroll();
  useLayoutEffect(() => { if (streaming) conversation?.followGrowth(); }, [conversation, streaming, visible]);
  return (
    <Message from="assistant" className="max-w-full">
      <MessageContent className={`message-content overflow-visible${tone === "narration" ? " message-narration" : ""}`}>
        <SafeMessageResponse text={bindCitations(visible)} isAnimating={streaming} caret={caret} numberChecks={checks} workspacePath={workspacePath} />
      </MessageContent>
    </Message>
  );
});
