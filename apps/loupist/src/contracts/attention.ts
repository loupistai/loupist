/** What waits for the user across Loupists; Rust builds the list (`attention_list`). */
/** `watchAlerts`: watcher alerts delivered but not yet on screen ("{Loupist} · {n} new alerts"), after unread results. */
/** `question`: a chat whose finished last answer asked the user something; Rust flags the answer as it is saved. */
export type AttentionKind = "approval" | "question" | "lineCrossed" | "briefingConflict" | "unreadResults" | "watchAlerts" | "checkDue";

export type AttentionItem = {
  kind: AttentionKind;
  loupistId: string;
  loupistName: string;
  /** The chat to open for an approval or a question. */
  chatId?: string;
  /** The Journal entry for a check; the asset page for a crossed line. */
  recordId?: string;
  /** A connection and tool for an approval, the chat's title for a question, the claim for a check; empty for briefings. */
  title: string;
  count?: number;
  dueDate?: string;
  atMs: number;
};

export type AttentionNotifyRequest = {
  loupistId: string;
  chatId: string;
  /** Localized title; Rust adds the body from the stored answer. */
  title: string;
};
