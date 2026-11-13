import { memo, useEffect, useRef, type RefObject } from "react";
import { Message, MessageContent } from "@/components/ai-elements/message";
import { stableMarkdown } from "../../chat/streamingMarkdown";
import type { ProseTone } from "../../chat/turnView";
import { usePacedText } from "../../chat/usePacedText";
import type { ChatMessage } from "../../types";
import { SafeMessageResponse } from "./SafeMessageResponse";
import { useArrived } from "./turnMotion";

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
  /** The host is still sending this text. */
  streaming: boolean;
  tone: ProseTone;
  /** A short note is one step back in colour; a long note is content and keeps the answer's. */
  short?: boolean;
  /** Stop or failure confirmed: what was received shows at once, with no fade. */
  halted?: boolean;
  /** A stop was requested: no more words are revealed until the host confirms. */
  paused?: boolean;
  /** Called once every word received has been revealed after the host finished (the commit gate). */
  onDrained?: () => void;
  checks?: ChatMessage["number_checks"];
  workspacePath?: string;
};

/**
 * One run of answer or note prose. The reveal governor paces the text, then
 * hold-back keeps a half-written construct off the page: a heading until its
 * line ends, a table whole, behind a ruled placeholder of fixed height. Each
 * word fades in as it is released. A segment that mounts with text already
 * received (a resumed run, another chat) shows it at once, with no fade.
 */
export const ProseSegment = memo(function ProseSegment({ text, streaming, tone, short = false, halted = false, paused = false, onDrained, checks, workspacePath }: Props) {
  const arrived = useArrived();
  const paced = usePacedText(text, { live: streaming, halted, paused, arrived, onDrained });
  const { text: visible, pending } = stableMarkdown(paced.text, paced.animating);
  const content = useRef<HTMLDivElement>(null);
  // A finished answer measures its tables once; a streaming one waits for the table to settle.
  useTableOverflow(content, paced.animating ? "" : visible);
  return (
    <Message from="assistant" className="max-w-full" data-turn-item="prose">
      <MessageContent ref={content} className={`message-content overflow-visible${tone === "note" && short ? " message-narration" : ""}`}
        data-tone={tone} data-streaming={(arrived && paced.animating) || undefined}>
        <SafeMessageResponse text={visible} isAnimating={paced.animating} fade={arrived} numberChecks={checks} workspacePath={workspacePath} />
        {pending === "table" ? <div className="message-table-pending" aria-hidden="true"><i /><i /><i /></div> : null}
      </MessageContent>
    </Message>
  );
});
