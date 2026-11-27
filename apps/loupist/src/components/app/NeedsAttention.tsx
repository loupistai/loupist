import i18n, { useTranslation } from "@/i18n";
import { formatDate, formatNumber, subjectName } from "@/i18n/format";
import { useEffect, useState } from "react";
import { SidebarGroup, SidebarGroupContent, SidebarGroupLabel } from "@/components/ui/sidebar";
import type { ChatThread } from "../../types";
import type { AttentionRow } from "../../app/useAttention";
import { scheduledTaskTitle } from "../briefings/scheduledPresentation";
import { chatTitle } from "../../app/chatTitle";
import type { AttentionItem } from "../../contracts/attention";

const t = i18n.getFixedT(null, "app");
const MAX_ROWS = 5;

export const attentionItemKey = (item: Pick<AttentionItem, "kind" | "loupistId" | "recordId" | "chatId" | "title">) =>
  `${item.kind}:${item.loupistId}:${item.recordId ?? item.chatId ?? item.title}`;

const settledAttention = new Set<string>();
const settledListeners = new Set<() => void>();

/** Turn on and Not now drop the proposal from the sidebar and the Bot tray together. */
export function settleAttention(item: Pick<AttentionItem, "kind" | "loupistId" | "recordId" | "chatId" | "title">) {
  const key = attentionItemKey(item);
  if (settledAttention.has(key)) return;
  settledAttention.add(key);
  for (const listener of settledListeners) listener();
}

export function useSettledAttention() {
  const [, bump] = useState(0);
  useEffect(() => {
    const listener = () => bump((value) => value + 1);
    settledListeners.add(listener);
    return () => { settledListeners.delete(listener); };
  }, []);
  return settledAttention;
}
type Threads = Readonly<Record<string, readonly ChatThread[] | undefined>>;
const DAY_MS = 24 * 60 * 60 * 1000;

/** Midnight of a YYYY-MM-DD date in local time. */
const localDay = (date: string) => {
  const [year, month, day] = date.slice(0, 10).split("-").map(Number);
  return new Date(year!, (month ?? 1) - 1, day ?? 1).getTime();
};

/** "Friday" within a week ahead, else "Oct 29". */
function dueDay(date: string, nowMs: number) {
  const due = localDay(date);
  const today = new Date(nowMs); today.setHours(0, 0, 0, 0);
  const days = Math.round((due - today.getTime()) / DAY_MS);
  return days >= 0 && days < 7
    ? formatDate(due, { weekday: "long", year: undefined, month: undefined, day: undefined })
    : formatDate(due, { year: undefined, month: "short", day: "numeric" });
}

/** The connection an approval waits on: Rust titles it "Connection · tool_name", and the tool name is internal. */
function approvalConnection(title: string) {
  const at = title.lastIndexOf(" · ");
  return (at > 0 ? title.slice(0, at) : title).trim();
}

/** Whether the sentence needs no separate Loupist label. */
export const attentionNamesLoupist = (item: AttentionItem) => item.kind !== "checkDue" && item.kind !== "lineCrossed";

/** One sentence that names what waits: the Loupist and the connection, scheduled result or check. */
export function attentionWhat(item: AttentionRow, threads?: Threads, nowMs = Date.now()) {
  const loupist = item.loupistName;
  switch (item.kind) {
    case "approval": {
      const connection = approvalConnection(item.title);
      const loupistSubject = subjectName(loupist, i18n.language);
      return connection ? t("attention.approval", { loupistSubject, connection }) : t("attention.approvalAny", { loupistSubject });
    }
    // The chat's title tells several waiting questions apart.
    case "question": {
      const loupistSubject = subjectName(loupist, i18n.language);
      const thread = threads?.[item.loupistId]?.find(entry => entry.id === item.chatId);
      const chat = item.taskKind ? scheduledTaskTitle(item.taskKind, item.title)
        : thread && item.title.trim() ? chatTitle({ ...thread, title: item.title }) : item.title.trim();
      return chat ? t("attention.question", { loupistSubject, chat }) : t("attention.questionAny", { loupistSubject });
    }
    case "savedKeyPermission": return t("attention.savedKeyPermission", { count: item.count ?? 1 });
    case "proposal": return t("attention.proposal", { title: item.title });
    case "taskPaused": return t("attention.taskPaused", { title: item.title });
    case "lineCrossed": return t("attention.lineCrossed", { name: item.title });
    case "offer": return t("attention.offer", { title: item.title });
    case "ask": {
      const loupistSubject = subjectName(loupist, i18n.language);
      return t("attention.ask", { loupistSubject });
    }
    case "watchQuiet": return t("attention.watchQuiet", { title: item.title });
    case "watchLoud": return t("attention.watchLoud", { title: item.title });
    case "briefingConflict": return t("attention.conflict", { loupist });
    case "unreadResults": return t("attention.unreadResults", { loupist, count: item.count ?? 1, formattedCount: formatNumber(item.count ?? 1) });
    case "triggerFailing": return t("attention.triggerFailing", { title: item.title });
    case "triggerUpdate": return t("attention.triggerUpdate", { title: item.title });
    case "checkDue": {
      const due = item.dueDate ?? "";
      const late = due ? localDay(due) < new Date(nowMs).setHours(0, 0, 0, 0) : false;
      return t(late ? "attention.checkLate" : "attention.checkDue", { day: due ? dueDay(due, nowMs) : "", title: item.title });
    }
  }
}

/**
 * The sidebar's "Needs attention" group: approvals still pending, answers that
 * asked the user a question, unread scheduled results and Journal checks coming
 * due, across every Loupist. It is not rendered when nothing waits.
 */
export function NeedsAttention({ items, threads, onOpen, onMore }: {
  items: readonly AttentionItem[];
  threads?: Threads;
  onOpen: (item: AttentionItem) => void;
  /** "{n} more" opens the command palette on what needs attention. */
  onMore?: () => void;
}) {
  useTranslation("app");
  const settled = useSettledAttention();
  const pending = items.filter((item) => !settled.has(attentionItemKey(item)));
  if (!pending.length) return null;
  const shown = pending.slice(0, MAX_ROWS);
  // Most sentences name their Loupist; a check names its claim, and its Loupist only when there are several.
  const manyLoupists = new Set(pending.map((item) => item.loupistId).filter(Boolean)).size > 1;
  const more = pending.length - shown.length;
  return <SidebarGroup className="needs-attention">
    <SidebarGroupLabel>{t("shell.needsAttention")}</SidebarGroupLabel>
    <SidebarGroupContent>
      <ul>
        {shown.map((item) => {
          const what = attentionWhat(item, threads);
          const prefix = !attentionNamesLoupist(item) && manyLoupists;
          return <li key={`${item.kind}:${item.loupistId}:${item.chatId ?? item.recordId ?? ""}`}>
            <button type="button" className="needs-attention-row" data-kind={item.kind} title={attentionNamesLoupist(item) ? what : `${item.loupistName} · ${what}`} onClick={() => onOpen(item)}>
              <span className="needs-attention-dot" aria-hidden="true" />
              <span className="needs-attention-text">{prefix ? <><strong>{item.loupistName}</strong> · </> : null}{what}</span>
            </button>
          </li>;
        })}
      </ul>
      {more > 0 ? (onMore
        ? <button type="button" className="needs-attention-more" onClick={onMore}>{t("attention.more", { count: more, formattedCount: formatNumber(more) })}</button>
        : <p className="needs-attention-more">{t("attention.more", { count: more, formattedCount: formatNumber(more) })}</p>) : null}
    </SidebarGroupContent>
  </SidebarGroup>;
}
