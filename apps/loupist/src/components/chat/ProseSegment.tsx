import { memo, useEffect, useLayoutEffect, useRef, type RefObject } from "react";
import { Message, MessageContent } from "@/components/ai-elements/message";
import { useConversationScroll } from "../ai-elements/conversation";
import { stableMarkdown } from "../../chat/streamingMarkdown";
import type { ProseTone } from "../../chat/turnView";
import { usePacedText } from "../../chat/usePacedText";
import type { ChatMessage } from "../../types";
import { SafeMessageResponse } from "./SafeMessageResponse";

/**
 * Marks a Markdown table wrapper that scrolls sideways (`data-overflows`), so its right
 * edge fades. One observer per segment, only while a table is rendered.
 */
function useTableOverflow(root: RefObject<HTMLElement | null>, revision: string) {
  useEffect(() => {
    const wrappers = [...(root.current?.querySelectorAll<HTMLElement>('[data-streamdown="table-wrapper"]') ?? [])];
    if (!wrappers.length || typeof ResizeObserver !== "function") return;
    const mark = () => { for (const wrapper of wrappers) wrapper.toggleAttribute("data-overflows", wrapper.scrollWidth > wrapper.clientWidth + 1); };
    const observer = new ResizeObserver(mark);
    for (const wrapper of wrappers) {
      observer.observe(wrapper);
      const table = wrapper.querySelector("table");
      if (table) observer.observe(table);
    }
    mark();
    return () => observer.disconnect();
  }, [root, revision]);
}

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
  const content = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { if (streaming) conversation?.followGrowth(); }, [conversation, streaming, visible]);
  // A finished answer measures its tables once; a streaming one waits for the table to settle.
  useTableOverflow(content, streaming ? "" : visible);
  return (
    <Message from="assistant" className="max-w-full">
      <MessageContent ref={content} className={`message-content overflow-visible${tone === "narration" ? " message-narration" : ""}`}>
        <SafeMessageResponse text={visible} isAnimating={streaming} caret={caret} numberChecks={checks} workspacePath={workspacePath} />
      </MessageContent>
    </Message>
  );
});
