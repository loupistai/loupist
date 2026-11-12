import { memo, useEffect, useLayoutEffect, useRef, type RefObject } from "react";
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

/**
 * Marks a Markdown table wrapper that scrolls sideways (`data-overflows`), so its right
 * edge fades. Streamdown scrolls the table inside its own frame (the table's parent), and
 * may mount the table after this segment commits, so tables are collected as they appear.
 */
function useTableOverflow(root: RefObject<HTMLElement | null>, revision: string) {
  useEffect(() => {
    const element = root.current;
    if (!element || typeof ResizeObserver !== "function") return;
    const observed = new Set<Element>();
    const wrappers = () => [...element.querySelectorAll<HTMLElement>('[data-streamdown="table-wrapper"]')];
    const mark = () => {
      for (const wrapper of wrappers()) {
        const frame = wrapper.querySelector("table")?.parentElement ?? wrapper;
        wrapper.toggleAttribute("data-overflows", frame.scrollWidth > frame.clientWidth + 1);
      }
    };
    const resize = new ResizeObserver(mark);
    const collect = () => {
      for (const wrapper of wrappers()) {
        for (const target of [wrapper, wrapper.querySelector("table")]) {
          if (target && !observed.has(target)) { observed.add(target); resize.observe(target); }
        }
      }
      mark();
    };
    const mutations = typeof MutationObserver === "function" ? new MutationObserver(collect) : null;
    mutations?.observe(element, { childList: true, subtree: true });
    collect();
    return () => { resize.disconnect(); mutations?.disconnect(); };
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
        <SafeMessageResponse text={bindCitations(visible)} isAnimating={streaming} caret={caret} numberChecks={checks} workspacePath={workspacePath} />
      </MessageContent>
    </Message>
  );
});
