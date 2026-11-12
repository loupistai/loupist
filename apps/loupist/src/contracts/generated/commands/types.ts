// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { AgentHarnessSnapshot } from "../agent_harness";
import type { LoupistAvailability } from "../app";
import type { ChatArtifact, ChatArtifactRef, ChatArtifactRevision } from "../chat/artifacts";
import type { ChartDataset } from "../chat/chart";
import type { ChatProgress, ChatProgressPatch } from "../chat/progress";
import type { AsOfReport, NumberCheck, SensitivityCard } from "../domain/answer_checks";
import type { ChatEvidenceRef, ChatFailureKind, ChatFeedback, ChatFeedbackRating, ChatMessageStatus, ChatPerformance, ChatToolRefusal, HomeAlert, HomeDelivery, HomeEarnings, HomeHandoff, HomeLink, HomeVocabulary, HomeWatch, MemoryUpdateResult, UnsavedThesis } from "../domain/chat";
import type { LoupistTemplate, LoupistTone } from "../domain/loupist";
import type { LensReceipt } from "../domain/memory";
import type { ExecutionModelLock } from "../providers/catalog";
import type { ScheduleReceipt } from "../scheduled_tasks/types";

export type ChatArtifactListRequest = { loupist_id: string, thread_id: string };

export type ChatArtifactReadRequest = {
  loupist_id: string,
  thread_id: string,
  artifact_id: string,
  revision_id?: number | null,
};

export type ChatArtifactRestoreRequest = {
  loupist_id: string,
  thread_id: string,
  artifact_id: string,
  revision_id: number,
  expected_current_revision: number,
};

export type ChatArtifactSelection = { artifact_id: string, revision: number };

export type ChatArtifactViewDto = {
  artifact: ChatArtifact,
  revision: ChatArtifactRevision,
  chart_dataset: ChartDataset | null,
  /**
   * A grid's cited evidence, from the reply that published the revision,
   * so its number popovers open wherever the grid is shown.
   */
  evidence_refs?: Array<ChatEvidenceRef>,
};

export type ChatAttachmentDto = {
  id: string,
  filename: string,
  media_type: string,
  size_bytes: number,
};

export type ChatAttachmentInput = { filename: string, media_type: string, data_base64: string };

export type ChatAttachmentReadDto = { data_url: string };

export type ChatAttachmentReadRequest = {
  loupist_id: string,
  thread_id: string,
  message_id: string,
  attachment_id: string,
};

/** "Not this time": leave one principle or method out of this chat's context, or bring it back. */
export type ChatBranchRequest = {
  loupist_id: string,
  thread_id: string,
  /** The user message being edited; everything from it on becomes an earlier version. */
  message_id: string,
  /**
   * A message just sent may still carry the renderer's id; then its place in the
   * chat and its exact text identify it.
   */
  position?: number | null,
  content?: string | null,
};

export type ChatControlModeDto = "steer";

export type ChatControlReceiptDto = {
  message_id: string,
  prompt: string,
  created_at: string,
  mode: ChatControlModeDto,
};

export type ChatControlRequestDto = { loupist_id: string, thread_id: string, prompt: string };

export type ChatCreateRequest = { loupist_id: string, title?: string | null };

export type ChatDeleteRequest = { loupist_id: string, thread_id: string };

export type ChatFeedbackRequest = {
  loupist_id: string,
  thread_id: string,
  message_id: string,
  rating?: ChatFeedbackRating | null,
  correction?: string | null,
};

export type ChatKnowledgePin = {
  selection?: SourceTextSelection,
  kind: string,
  recordId: string | null,
  revisionId: string | null,
  objectId: string,
  spanId: string | null,
  label: string,
};

export type ChatMemorySetAsideRequest = {
  loupist_id: string,
  thread_id: string,
  record_id: string,
  set_aside: boolean,
};

export type ChatMessageDto = {
  tool_refusals?: Array<ChatToolRefusal>,
  /**
   * A data read in this reply never reached its source (the network or the
   * provider was unreachable).
   */
  offline_reads?: boolean,
  /** Asset reads this reply's tool accepted but its commit did not save. */
  unsaved_theses?: Array<UnsavedThesis>,
  feedback: ChatFeedback | null,
  failure_kind?: ChatFailureKind,
  host_composed: boolean,
  /** Accepted live guidance, shown as a "Guided" row. */
  steer?: boolean,
  retry_after_seconds?: number,
  delivery?: HomeDelivery,
  /** The model Rust replaced with Auto for this answer (W17.5). */
  replaced_model?: string,
  alert?: HomeAlert,
  watch?: HomeWatch,
  earnings?: HomeEarnings,
  vocabulary?: HomeVocabulary,
  link?: HomeLink,
  handoff: HomeHandoff | null,
  /** The as-of date set for this request and its answer (`YYYY-MM-DD`). */
  as_of?: string,
  /** Number links Rust checked against their evidence (W16.3). */
  number_checks?: Array<NumberCheck>,
  /** Grid results linked as paragraphs of their own (W16.8). */
  sensitivity_cards?: Array<SensitivityCard>,
  /** What an as-of answer could not use (W16.5). */
  as_of_report?: AsOfReport,
  id: string,
  role: ChatMessageRoleDto,
  content: string,
  /**
   * `content` is the host's failure placeholder (`FAILED_CHAT_CONTENT`),
   * not model text; the renderer explains the failure from its run state.
   */
  placeholder?: boolean,
  created_at: string,
  status: ChatMessageStatus,
  memory_refs?: Array<MemoryRefDto>,
  lens_receipts?: Array<LensReceipt>,
  observed_exact_count: number,
  refs_truncated: boolean,
  refs_digest: string,
  memory_update?: MemoryUpdateResult,
  memory_revision?: string,
  execution_model?: ExecutionModelLock,
  agent_harness?: AgentHarnessSnapshot,
  attachments?: Array<ChatAttachmentDto>,
  artifact_refs?: Array<ChatArtifactRef>,
  evidence_refs?: Array<ChatEvidenceRef>,
  schedule_receipts?: Array<ScheduleReceipt>,
  progress?: ChatProgress,
  performance?: ChatPerformance,
};

/** The roles a stored Chat message may cross the renderer boundary with. */
export type ChatMessageRoleDto = "user" | "assistant";

export type ChatRenameRequest = { loupist_id: string, thread_id: string, title: string };

/** What a running reply is doing between progress steps. */
export type ChatRunPhase = "thinking" | "writing" | "tool";

export type ChatSendRequest = {
  run_id: string,
  loupist_id: string,
  thread_id: string,
  prompt: string,
  use_memory: boolean,
  update_memory: boolean,
  as_of?: string | null,
  model_profile_id: string,
  thinking_level: string,
  run_options: { [key in string]: string },
  /** Composer `@entry_id` tokens. Host keeps only enabled execute-ready ids. */
  mentioned_connection_ids?: Array<string>,
  attachments: Array<ChatAttachmentInput>,
  pins?: Array<ChatKnowledgePin>,
  selected_artifact?: ChatArtifactSelection | null,
};

export type ChatStreamEvent =
  | { "type": "started", run_id: string }
  | { "type": "memory", run_id: string, memories: Array<MemoryRefDto> }
  | {
    "type": "delta",
    run_id: string,
    text: string,
    /**
     * The live commentary this settled text replaces, so the renderer
     * drops its copy in the same update that adds the text.
     */
    settles?: string,
  }
  | { "type": "generation_finished", run_id: string, performance: ChatPerformance }
  | { "type": "title", run_id: string, title: string }
  | { "type": "progress", run_id: string, sequence: number, progress: ChatProgress }
  | { "type": "progress_patch", run_id: string, patch: ChatProgressPatch }
  | { "type": "phase", run_id: string, phase: ChatRunPhase }
  | { "type": "memory_update", run_id: string, result: MemoryUpdateResult }
  | { "type": "schedule", run_id: string, receipt: ScheduleReceipt }
  | { "type": "evidence", run_id: string, evidence: ChatEvidenceRef }
  | { "type": "artifact", run_id: string, artifact: ChatArtifactRef }
  | {
    "type": "completed",
    lens_receipts: Array<LensReceipt>,
    tool_refusals: Array<ChatToolRefusal>,
    offline_reads?: boolean,
    unsaved_theses?: Array<UnsavedThesis>,
    host_composed: boolean,
    run_id: string,
    message_id: string,
    final_text: string,
    created_at: string,
    execution_model: ExecutionModelLock,
    agent_harness: AgentHarnessSnapshot,
    performance: ChatPerformance,
    replaced_model?: string,
    as_of?: string,
    number_checks?: Array<NumberCheck>,
    sensitivity_cards?: Array<SensitivityCard>,
    as_of_report?: AsOfReport,
  }
  | { "type": "aborted", run_id: string }
  | {
    "type": "error",
    retry_after_seconds?: number,
    failure_kind: ChatFailureKind,
    run_id: string,
    message: string,
    message_id: string,
    final_text: string,
    /** `final_text` is the host's failure placeholder, not model text. */
    placeholder?: boolean,
    created_at: string,
    execution_model: ExecutionModelLock,
    agent_harness: AgentHarnessSnapshot,
    progress?: ChatProgress,
  };

export type ChatThreadDto = {
  id: string,
  loupist_id: string,
  /** The Loupist's home Thread: listed first, landed on, never renamed or deleted. */
  home: boolean,
  title: string,
  updated_at: string,
  use_memory: boolean,
  update_memory: boolean,
  /** Principle and method ids the user set aside for this chat. */
  set_aside_memory?: Array<string>,
  /** Edit forks (X18): the other versions of this chat's tail, without their messages. */
  versions?: Array<ChatVersionDto>,
  messages: Array<ChatMessageDto>,
};

/** One other version of a chat's tail, as the renderer lists it. */
export type ChatVersionDto = {
  id: string,
  after_message_id: string | null,
  first_message_at_ms: number,
  message_count: number,
};

export type ChatVersionSwitchRequest = { loupist_id: string, thread_id: string, version_id: string };

export type KnowledgeRefDto = {
  kind: string,
  recordId: string,
  revisionId: string,
  objectId: string,
  spanId?: string,
  receiptId?: string,
};

export type LoupistArtifactItemDto = { artifact: ChatArtifact, chat_title: string };

/** The Loupist's Work list: charts and documents across its chats. */
export type LoupistArtifactListRequest = { loupist_id: string, limit?: number | null };

export type LoupistCreateRequest = {
  name: string,
  focus?: string | null,
  avatar?: string | null,
  template?: string | null,
  /** Missing means Balanced. */
  tone?: LoupistTone | null,
};

export type LoupistDeleteRequest = { loupist_id: string };

export type LoupistImportRequest = { workspace_path: string };

export type LoupistProfileUpdateRequest = {
  loupistId: string,
  name?: string | null,
  focus?: string | null,
  instructions?: string | null,
  avatar?: string | null,
  tone?: LoupistTone | null,
};

export type LoupistReconnectRequest = { loupist_id: string, workspace_path: string };

export type LoupistSummary = {
  id: string,
  name: string,
  workspace_path: string,
  philosophy: string,
  instructions: string,
  avatar: string,
  tone: LoupistTone,
  /** The template the Loupist was made from; missing for imported folders and packs. */
  template?: LoupistTemplate,
  record_count: number,
  updated_at: string,
  accent: string,
  last_model_profile_id: string | null,
  enabled_skill_ids: Array<string>,
  availability: LoupistAvailability,
};

export type LoupistWorkspace = { loupist: LoupistSummary, threads: Array<ChatThreadDto> };

export type MemoryRefDto = {
  read_ref: string,
  kind: string,
  title: string,
  excerpt: string,
  as_of: string | null,
  section: string | null,
  access: string,
  knowledge?: KnowledgeRefDto,
};

export type RunStarted = { run_id: string };

export type SourceTextSelection = { startUtf16: number, endUtf16: number, text: string };
