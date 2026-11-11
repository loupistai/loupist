// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { ChatApproval } from "../chat/approvals";
import type { ChatQuestion } from "../chat/questions";

export type ChatPlan = { items: Array<ChatPlanItem> };

export type ChatPlanItem = {
  id: string,
  label: string,
  status: ChatPlanStatus,
  /** When the item first became active; steps that start after it nest under it. */
  activatedAtMs?: number,
};

export type ChatPlanStatus = "pending" | "active" | "done" | "not_done";

export type ChatProgress = {
  startedAtMs: number,
  finishedAtMs: number | null,
  items: Array<ChatProgressItem>,
  /** The agent's short checklist for multi-step work (`plan_update`); absent for ordinary runs. */
  plan?: ChatPlan,
  /** Approvals this reply asked for (composer spec D3), in request order. */
  approvals?: Array<ChatApproval>,
  /** Questions this reply asked the user (`ask_user`), in request order. */
  questions?: Array<ChatQuestion>,
};

export type ChatProgressCategory =
  | "web"
  | "memory"
  | "capability"
  | "finance"
  | "files"
  | "artifact"
  | "compute"
  | "system";

export type ChatProgressItem =
  | { "kind": "commentary", id: string, text: string }
  | {
    "kind": "tool",
    id: string,
    /**
     * The host's name for the tool this step called (`chart_publish`,
     * `mcp__openbb__equity_price_quote`). The renderer recognizes and
     * localizes steps by it; absent only on steps recorded before it existed.
     */
    toolName?: string,
    category: ChatProgressCategory,
    operation: ChatProgressOperation,
    action: string,
    running?: string,
    target: string | null,
    href: string | null,
    source?: ChatProgressSource,
    summary?: string,
    resultIds?: Array<string>,
    /** World names this step's acknowledged reads delivered. Host-attributed, never model input. */
    entityIds?: Array<string>,
    outputRevision?: number,
    detailId?: string,
    filePath?: string,
    status: ChatProgressStatus,
    /** Set on a failed step the host refused under a security rule. */
    refusal?: ChatStepRefusal,
    startedAtMs: number,
    finishedAtMs: number | null,
  }
  | {
    "kind": "system",
    id: string,
    category: ChatProgressCategory,
    operation: ChatProgressOperation,
    action: string,
    target: string | null,
    href: string | null,
    status: ChatProgressStatus,
    startedAtMs: number,
    finishedAtMs: number | null,
  };

export type ChatProgressOperation =
  | "search"
  | "read"
  | "write"
  | "edit"
  | "list"
  | "calculate"
  | "publish"
  | "execute"
  | "submit"
  | "retry"
  | "compact"
  | "generic";

export type ChatProgressPatch = {
  sequence: number,
  upsertItems?: Array<ChatProgressItem>,
  /**
   * Text appended to commentary this stream already published, in order. A
   * commentary id is in `upsert_items` or here, never both: its first
   * publication carries the whole item.
   */
  appendText?: Array<CommentaryAppend>,
  removeItemIds?: Array<string>,
  finishedAtMs?: number,
  /** The whole plan whenever it changed since the last patch. */
  plan?: ChatPlan,
  /** Every approval whenever one changed since the last patch. */
  approvals?: Array<ChatApproval>,
  /** Every question whenever one changed since the last patch. */
  questions?: Array<ChatQuestion>,
};

export type ChatProgressSource = { kind: ChatProgressSourceKind, name: string };

export type ChatProgressSourceKind =
  | "web"
  | "memory"
  | "files"
  | "finance"
  | "connector"
  | "artifact"
  | "compute"
  | "capability";

export type ChatProgressStatus = "running" | "succeeded" | "failed" | "stopped" | "skipped";

/**
 * Why the host refused a failed step, when a security rule or the user's own
 * card answer did. The renderer explains it in the step and offers no override.
 */
export type ChatStepRefusal = "untrusted_text_paused" | "protected_path" | "outside_workspace";

export type CommentaryAppend = { id: string, text: string };
