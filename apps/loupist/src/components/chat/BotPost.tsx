import type { ReactNode } from "react";
import { formatTime } from "@/i18n/format";
import { LoupistAvatar } from "../app/LoupistAvatar";
import { useChatSurface } from "./turnMotion";
import "@/styles/bot-thread.css";

/**
 * What the Loupist posts into Bot without a model call (a scheduled result, a crossed line,
 * a hand-off, a link, its news) sits where its answers do: its face and name beside the post.
 * The face is identity here, not a control: a host post has no activity to open. Outside Bot
 * (a test, a reader) the post keeps its name line alone.
 */
export function BotPostBody({ loupistName, at, children }: { loupistName: string; at?: number; children: ReactNode }) {
  const surface = useChatSurface();
  const bot = surface.kind === "bot";
  const meta = <div className={bot ? "message-meta bot-who" : "message-meta"}>
    <span>{loupistName}</span>
    {at !== undefined && Number.isFinite(at) ? <time dateTime={new Date(at).toISOString()}>{formatTime(at)}</time> : null}
  </div>;
  if (!bot) return <>{meta}{children}</>;
  return <>
    <span className="bot-face bot-face-still" aria-hidden="true"><LoupistAvatar loupist={surface.loupist} size={32} /></span>
    <div className="bot-col">{meta}{children}</div>
  </>;
}

export type MetaItem = { key: string; node: ReactNode } | null | false | undefined;

/**
 * "a · b · c" that wraps only between items: every item carries its "·" in front, and the
 * row is clipped one separator to the left (bot-cards.css), so the "·" of whichever item
 * starts a line falls outside it. A line neither ends nor starts on a separator. Keys keep a
 * control mounted while items around it change.
 */
export function MetaLine({ className, items }: { className?: string; items: MetaItem[] }) {
  const shown = items.filter((item): item is { key: string; node: ReactNode } => Boolean(item));
  return <p className={className ? `bot-meta ${className}` : "bot-meta"}>
    <span className="bot-meta-items">
      {shown.map((item) => <span key={item.key} className="bot-meta-item"><span className="bot-meta-sep" aria-hidden="true">·</span>{item.node}</span>)}
    </span>
  </p>;
}

/** Joins words with " · " so the separator stays with the word after it in running text. */
export const joinMeta = (parts: readonly (string | null | undefined | false)[]) =>
  parts.filter(Boolean).join(" · ");
