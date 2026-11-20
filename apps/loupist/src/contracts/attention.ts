import type * as Wire from "./generated";
/** What waits across Loupists. A question comes from a live ask-the-user request in an existing chat. */
export type AttentionKind = Wire.AttentionKind;

export type AttentionItem = Wire.AttentionItem;

export type AttentionNotifyRequest = {
  loupistId: string;
  chatId: string;
  /** Localized title; Rust adds the body from the stored answer. */
  title: string;
};
