// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

/** One question the reply asked, shown as a card at its step. */
export type ChatQuestion = {
  requestId: string,
  /** The progress step that waits; absent when the call carried no tool call id. */
  itemId?: string,
  question: string,
  options: Array<ChatQuestionOption>,
  /** Index into `options` the agent suggests and assumes when nobody answers. */
  recommended?: number,
  state: QuestionState,
  answer?: QuestionAnswer,
  requestedAtMs: number,
};

export type ChatQuestionOption = { label: string, description?: string };

/** An option's index, or the user's own words. */
export type QuestionAnswer = { "choice": number } | { "text": string };

export type QuestionState = "waiting" | "answered" | "expired";
