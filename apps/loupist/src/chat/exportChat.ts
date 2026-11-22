import i18n from "@/i18n";
import { chatTitle } from "../app/chatTitle";
import { formatDate, formatDateTime } from "@/i18n/format";
import type { ChatThread } from "../types";
import { checkFor, exportFootnote } from "./numberChecks";
import { evidenceLinkStarts } from "./evidenceLinkOrder";

const t = i18n.getFixedT(null, "chat");

/** A file name from the chat title: letters, digits and spaces only, at most 80 characters. */
export const exportFileName = (title: string) => (title.replace(/[^\p{L}\p{N} _-]+/gu, " ").replace(/\s+/gu, " ").trim().slice(0, 80) || "chat");

/**
 * A Markdown transcript of one Chat (shell spec X9): a title line (chat, Loupist,
 * date), the AI-produced disclosure with the models used, each message with its
 * role and time, citations as footnotes with the source title and URL, and the
 * AI notice as the last line. The print view draws the notice itself, after
 * the footnotes the renderer gathers at the end (`notice: false`).
 */
export function chatMarkdown(thread: ChatThread, loupistName: string, nowMs = Date.now(), { notice = true } = {}) {
  const lines = [`# ${chatTitle(thread)}`, "", `${loupistName} · ${formatDate(nowMs)}`, ""];
  // AI-produced disclosure (W17.6): the third line names the models that wrote the answers.
  const models = [...new Set(thread.messages.flatMap((message) => message.role === "assistant" && message.execution_model?.name ? [message.execution_model.name] : []))];
  if (models.length) lines.push(t("chatExport.madeWithAi", { models: models.join(", ") }), "");
  const footnotes: string[] = [];
  for (const message of thread.messages) {
    const time = Date.parse(message.created_at);
    // A scheduled run's request is its task's, not the user's; the briefing's is the host's and is left out.
    const turn = message.role === "user" ? message.scheduled : undefined;
    const who = turn ? t("scheduledTurn.ran", { title: chatTitle({ title: turn.task_title, messages: [message] }) })
      : message.role === "bot_handoff" ? t("handoff.fromBot") : message.role === "user" ? t("chatExport.you") : loupistName;
    lines.push(`## ${who}${Number.isFinite(time) ? ` · ${formatDateTime(time)}` : ""}`, "");
    if (turn?.task_kind === "briefing") continue;
    let content = message.content.trim();
    // Number links keep the number and gain a footnote with its formula or source: each occurrence
    // its own check, found by its place among the answer's links as Rust counted them.
    const starts = evidenceLinkStarts(message.content);
    const lead = message.content.length - message.content.trimStart().length;
    content = content.replace(/\[([^\]\n]+)\]\(#evidence-([A-Za-z0-9_]+)\)/gu, (whole, text: string, id: string, at: number) => {
      const index = starts.indexOf(at + lead);
      const check = checkFor(message.number_checks, index < 0 ? undefined : index, id, text);
      if (!check) return whole;
      const number = footnotes.length + 1;
      const evidence = message.evidence_refs?.find((item) => item.id === id);
      footnotes.push(`[^${number}]: ${exportFootnote(check, evidence, message.evidence_refs ?? [])}`);
      return `${text}[^${number}]`;
    });
    // Citation links point at in-app readers; in a file they become numbered footnotes.
    for (const evidence of message.evidence_refs ?? []) {
      const number = footnotes.length + 1;
      const pattern = new RegExp(`\\[(\\d+)\\]\\(#evidence-${evidence.id.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&")}\\)`, "gu");
      if (!pattern.test(content)) continue;
      content = content.replace(pattern, `[^${number}]`);
      footnotes.push(`[^${number}]: ${evidence.title}${evidence.url ? ` — ${evidence.url}` : ""}`);
    }
    lines.push(content, "");
    if (message.role === "bot_handoff" && message.handoff_note?.userMessageInBot) {
      lines.push(t("handoff.youSaid"), "", ...message.handoff_note.userMessageInBot.split(/\r?\n/u).map(line => `> ${line}`), "");
    }
  }
  if (footnotes.length) lines.push("---", "", ...footnotes, "");
  if (notice) lines.push(`_${t("chatExport.notice")}_`, "");
  return lines.join("\n");
}

const significantDigits = (text: string) => text.replace(/^-/u, "").replace(/e.*$/iu, "").replace(".", "").replace(/^0+/u, "").length;

/**
 * A dataset cell as text. A fraction that a provider stored as a 32-bit float
 * and that reached us widened (225.3000030517578) prints as the shortest
 * decimal that 32-bit value stands for (225.3). Every other value, including
 * every integer and any number of 9 or fewer significant digits, prints exactly.
 */
export function datasetCellText(value: unknown): string {
  if (value == null) return "";
  const text = String(value);
  if (typeof value !== "number" || Number.isInteger(value) || !Number.isFinite(value) || Math.fround(value) !== value || significantDigits(text) <= 9) return text;
  for (let digits = 1; digits <= 9; digits += 1) {
    const shorter = Number(value.toPrecision(digits));
    if (Math.fround(shorter) === value) return String(shorter);
  }
  return text;
}

/** Comma-separated rows with a header line; cells with commas, quotes or line breaks are quoted. */
export function datasetCsv(columns: readonly { label: string }[], rows: readonly unknown[][]) {
  const cell = (value: unknown) => {
    const text = datasetCellText(value);
    return /[",\n\r]/u.test(text) ? `"${text.replaceAll('"', '""')}"` : text;
  };
  return [columns.map((column) => column.label), ...rows].map((row) => row.map(cell).join(",")).join("\r\n");
}

export const utf8Bytes = (text: string) => Array.from(new TextEncoder().encode(text));
