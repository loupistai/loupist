// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { BriefingContext } from "../briefings";
import type { RunStatus, RunSummaryItem, Schedule, SkippedTool, Task, TaskKind } from "../scheduled_tasks/types";

export type BriefingSettings = { enabled: boolean, schedule: Schedule, task: Task | null };

export type BriefingSettingsRequest = {
  loupistId: string,
  enabled: boolean,
  schedule?: Schedule | null,
};

export type BriefingsLoupistRequest = { loupistId: string };

export type InboxItem = {
  briefingContext: BriefingContext | null,
  error: string | null,
  runId: string,
  taskId: string,
  kind: TaskKind,
  title: string,
  status: RunStatus,
  summary: string,
  summaryItems: Array<RunSummaryItem>,
  startedAtMs: number,
  finishedAtMs: number | null,
  readAtMs: number | null,
  threadId: string | null,
  messageId: string | null,
  skippedTools: Array<SkippedTool>,
};

export type InboxPage = { items: Array<InboxItem>, nextCursor: string | null };

export type InboxRequest = { loupistId: string, cursor?: string | null, limit: number };

export type ReadRequest = { loupistId: string, runIds: Array<string> };

export type UnreadCount = { loupistId: string, count: number };
