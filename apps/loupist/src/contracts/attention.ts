import type * as Wire from "./generated";
/** What waits for the user across Loupists; Rust builds the list (`attention_list`). */
/** `watchAlerts`: watcher alerts delivered but not yet on screen ("{Loupist} · {n} new alerts"), after unread results. */
/** `question`: a chat whose finished last answer asked the user something; Rust flags the answer as it is saved. */
export type AttentionKind = Wire.AttentionKind;

export type AttentionItem = Wire.AttentionItem;

export type AttentionNotifyRequest = {
  loupistId: string;
  chatId: string;
  /** Localized title; Rust adds the body from the stored answer. */
  title: string;
};
