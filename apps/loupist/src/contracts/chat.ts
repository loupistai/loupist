import type { ScheduleReceipt } from "./scheduled";
import type { LoupistSummary } from "./loupist";
import type { MemoryRef, MemoryUpdateResult } from "./memory";
import type { AgentHarnessSnapshot, ExecutionModelLock } from "./model";
import type { EarningsMeasure } from "./earnings";
import type { WatchEvent } from "./watch";
import type { LearnedDigest } from "./lens";
import type * as Wire from "./generated";

export type ChatProgressStatus = Wire.ChatProgressStatus;

export type ChatProgressCategory = Wire.ChatProgressCategory;

export type ChatProgressSourceKind = Wire.ChatProgressSourceKind;

export type ChatProgressSource = Wire.ChatProgressSource;

export type ChatRunPhase = Wire.ChatRunPhase;

export type ChatProgressOperation = Wire.ChatProgressOperation;

export type ChatProgressItem =
  | { id: string; kind: "commentary"; text: string }
  | {
      id: string;
      kind: "tool" | "system";
      /**
       * The host's name for the tool a `tool` step called (`chart_publish`,
       * `mcp__openbb__equity_price_quote`); steps are recognized and localized by it.
       * Absent on system steps and on steps recorded before it existed.
       */
      toolName?: string;
      category: ChatProgressCategory;
      operation: ChatProgressOperation;
      action: string;
      running?: string;
      target: string | null;
      href: string | null;
      source?: ChatProgressSource;
      summary?: string;
      resultIds?: string[];
      /** World names this step read, attributed by the host. */
      entityIds?: string[];
      outputRevision?: number;
      detailId?: string;
      filePath?: string;
      status: ChatProgressStatus;
      /** On a failed step the host refused under a security rule: which rule. */
      refusal?: ChatStepRefusal;
      startedAtMs: number;
      finishedAtMs: number | null;
    };

/**
 * Why Rust refused a failed step. `untrusted_text_paused`: the answer read third-party text,
 * so a command, an instruction write, a connection, a schedule
 * change or a new site stayed off. `protected_path`: the path is inside the app's private
 * data, a credential store or a git directory's hooks or config. `outside_workspace`: the
 * path is outside the Loupist folder.
 */
export type ChatStepRefusal = Wire.ChatStepRefusal;

export type ChatPlanStatus = Wire.ChatPlanStatus;

export type ChatPlanItem = Wire.ChatPlanItem;

/** The agent's checklist for multi-step work, projected by the host from `plan_update`. */
export type ChatPlan = Wire.ChatPlan;

/** A read-only connection tool the reply waits to use until the user decides (composer D3). */
export type ChatApproval = Wire.ChatApproval;

export type ChatApprovalDecision = "once" | "always" | "deny";

/** One question the agent asked mid-reply (`ask_user`): 2 to 4 options and always a free-text answer. */
export type ChatQuestion = Wire.ChatQuestion;

export type ChatProgress = {
  startedAtMs: number;
  finishedAtMs: number | null;
  items: ChatProgressItem[];
  plan?: ChatPlan;
  approvals?: ChatApproval[];
  questions?: ChatQuestion[];
};

export type ChatProgressPatch = {
  sequence: number;
  upsertItems?: ChatProgressItem[];
  /** Text appended to commentary already published; an id is here or in `upsertItems`, never both. */
  appendText?: { id: string; text: string }[];
  removeItemIds?: string[];
  finishedAtMs?: number;
  /** The whole plan whenever it changed. */
  plan?: ChatPlan;
  /** Every approval whenever one changed. */
  approvals?: ChatApproval[];
  /** Every question whenever one changed. */
  questions?: ChatQuestion[];
};

export type ChatArtifactKind = Wire.ChatArtifactKind;

export type ChatArtifactRef = {
  /** Renderer selection; included in the user message, never in the authority receipt. */
  selected_range?: { start: string; end: string };
  artifact_id: string;
  revision: number;
  kind: ChatArtifactKind;
  title: string;
  digest: string;
};

export type ChatArtifact = Wire.ChatArtifact;

export type ChartColumn = Wire.ChartColumn;

export type ChartResultReceipt = Wire.ChartResultReceipt;

export type ChartDatasetLineage =
  | { kind: "from_dataset"; dataset_id: string; revision: number; digest: string; rows_pointer: string; columns: Array<ChartColumn & { pointer: string }> }
  | {
      kind: "from_result";
      receipt: ChartResultReceipt;
      rows_pointer: string;
      columns: Array<ChartColumn & { pointer: string }>;
    }
  | {
      kind: "agent_authored";
      upstream_receipts: ChartResultReceipt[];
    };

export type ChartDataset = {
  id: string;
  columns: ChartColumn[];
  rows: unknown[][];
  lineage: ChartDatasetLineage;
  digest: string;
};

export type ChartStudy = Wire.ChartStudy;

export type ChartDrawing = Wire.ChartDrawing;

export type ChartCustomSeries = Wire.ChartCustomSeries;

export type ChartDocument = Wire.ChartDocument;

export type ChatArtifactPayload =
  | {
      kind: "markdown";
      schema: "loupist-markdown/1";
      markdown: string;
    }
  | {
      kind: "chart";
      schema: "loupist-chart/1";
      chart: ChartDocument;
    }
  | {
      kind: "grid";
      schema: "loupist-grid/1";
      grid: GridDocument;
    };

/** A measure column's unit (finance-grid-answers.md §3.2). */
export type GridUnit = Wire.GridUnit;
/** Why a grid cell is empty; never a zero. */
export type GridEmptyReason = Wire.GridEmptyReason;
/** Why sources disagree (finance-korea-retail-disagreement.md §3.3). */
export type ConflictReason = Wire.ConflictReason;

/** One asset row; `asset_id` is set when Rust resolved the asset page. */
export type GridRow = Wire.GridRow;
export type GridColumn = Wire.GridColumn;
/** A value with the evidence it cites and Rust's check of it; `sort` is its base-unit value. */
export type GridValueCell = { value: string; evidence_id: string; check: NumberCheck; sort?: number };
export type GridEmptyCell = { empty: GridEmptyReason; note?: string };
export type GridCell = Wire.GridCell;

export type AssetsGrid = { layout: "assets" } & Wire.AssetsGrid;

/** One source's report; `dated` comes from the evidence, `check` only for a number. */
export type ConflictReport = Wire.ConflictReport;

export type ConflictGrid = { layout: "conflict" } & Wire.ConflictGrid;

export type GridDocument = AssetsGrid | ConflictGrid;

/** One row of the Loupist's Work list: a chart or document and the chat it came from. */
export type LoupistArtifactItem = { artifact: ChatArtifact; chat_title: string };

export type ChatArtifactRevision = {
  artifact_id: string;
  revision: number;
  payload: ChatArtifactPayload;
  digest: string;
  source_message_id: string;
  created_at_ms: number;
};

/** Stable revision numbers scoped to one artifact, newest first; at most 20 / 32 MiB. */
export type ChatArtifactVersion = Wire.ChatArtifactVersion;

export type ChatArtifactRestoreRequest = Wire.ChatArtifactRestoreRequest;

export type ChatArtifactView = {
  artifact: ChatArtifact;
  revision: ChatArtifactRevision;
  chart_dataset: ChartDataset | null;
  /** A grid's cited evidence, from the reply that published the revision. */
  evidence_refs?: ChatEvidenceRef[];
};

/** Immutable, host-issued excerpt from an acknowledged tool result. */
export type ChatEvidenceRef = Wire.ChatEvidenceRef;

export type ChatFeedback = Wire.ChatFeedback;
export type ChatFeedbackRequest = { loupist_id: string; thread_id: string; message_id: string; rating: ChatFeedback["rating"]; correction: string | null };

export type ChatFailureKind = Wire.ChatFailureKind;

export type ChatToolRefusal = Wire.ChatToolRefusal;

/** A research session the Bot handed off (Rust `HomeHandoff`): one card per hand-off, its status changing in place. */
export type HomeHandoff = {
  handoffId: string;
  chatId: string;
  title: string;
  status: "running" | "done" | "failed" | "stopped";
  learned?: LearnedDigest;
};

/** One hop of a discovered link's path: the name reached and the relation walked, against its claim when `reversed`. */
export type HomeLinkHop = Wire.HomeLinkHop;

/**
 * A link discovery found new to this Loupist (Rust `HomeLink`): seed to far end
 * through the path's names. `nodeIds` (seed first) highlight the path on the Map.
 */
export type HomeLink = Wire.HomeLink;

/** A scheduled result the host posted into the Loupist's home Thread. */
export type HomeDelivery = Wire.HomeDelivery;

/** A crossed red line the host announced in the home Thread (Rust `HomeAlert`). Names and the user's claim only. */
export type HomeAlert = Wire.HomeAlert;

/**
 * Watcher events the host posted in one Thread message (Rust `HomeWatch`): at
 * most five, one sentence each, then "+{more} more". Names and dated facts only,
 * never holdings.
 */
export type HomeWatch = {
  event_ids: string[];
  events: WatchEvent[];
  /** Events of the same check beyond the five shown (opens Schedule › Watching). */
  more: number;
};

/** New kinds of things the map tracks after a preset or a background review. Labels are display words. */
export type HomeVocabulary = {
  changeId: string;
  producer: "preset" | "chat" | "review";
  labels: string[];
  /** The same kinds in Korean, in the same order, when each has one. */
  labelsKo?: string[];
};
/**
 * A results snapshot the host posted (Rust `HomeEarnings`): "{name} reported
 * {period} results." with the two most material measures, or a failed
 * before-capture with Try again. Successful before-captures post nothing.
 */
export type HomeEarnings = {
  asset_id: string;
  event_id: string;
  name: string;
  symbol: string;
  market: string;
  period: string;
  outcome: "reported" | "failed";
  /** The side to capture again for a failed run. */
  side?: "before" | "after";
  /** At most two measures, as the card's table renders them. */
  measures: EarningsMeasure[];
};

/** A thesis update Rust did not save at commit; `reason` is its refusal code. */
export type UnsavedThesis = Wire.UnsavedThesis;

/** How Rust found a number link against its evidence (finance-show-the-math.md §7.2). */
export type NumberCheckStatus = Wire.NumberCheckStatus;
export type NumberCheckKind = Wire.NumberCheckKind;

/** One number link in a finished answer, checked by Rust. The renderer only reads it. */
/** How Rust says a `calculate` input is printed (`CalcInputUnit`): an amount carries its ISO currency and power-of-ten scale. */
export type CalcInputUnit = Wire.CalcInputUnit;

export type NumberCheck = Wire.NumberCheck;

/**
 * A `dcf_grid` or `reverse_dcf` result as Rust copied it from the host's own
 * calculation (finance-korea-retail-disagreement.md §2.2). Never one point value.
 */
export type SensitivityGrid = {
  operation: "reverse_dcf" | "dcf_grid";
  /** Discount rates (%), one per row. */
  row_values: number[];
  /** Long-run growth rates (%), one per column. */
  column_values: number[];
  /** rows × columns: implied yearly growth (%) for reverse_dcf, value for dcf_grid; null where reverse_dcf found no growth within −50%…100%. */
  values: (number | null)[][];
  /** "%" for reverse_dcf; "per share" or the amount unit for dcf_grid. */
  unit: string;
  forecast_years: number;
  /** reverse_dcf with a reference growth (usually a past CAGR); `years` and `label` describe the period it covers when known. */
  reference?: { value_pct: number; label?: string; years?: number; evidence_id?: string; status?: NumberCheckStatus };
  /** Cells that need more growth than the reference (reverse_dcf with a reference only). */
  above_reference?: boolean[][];
  /** The price the reverse grid solves for: as the input printed it, its currency and date when known, and its source. */
  price?: { value: number; display: string; currency?: string; date?: string; evidence_id?: string; status?: NumberCheckStatus };
  /** Currency and scale of the amount inputs as the calculation was given them ("USD millions"). */
  amount_unit?: string;
  /** cash_flow, shares, net_debt (and growth_pct for dcf_grid), with the label the calculation was given and their sources. */
  inputs: { name: string; label?: string; display: string; evidence_id?: string; status?: NumberCheckStatus }[];
  formulas: string[];
};

/** A paragraph of the answer that is only a link to a grid result renders as this card in place of the paragraph. */
export type SensitivityCard = SensitivityGrid & {
  /** Index among the answer's `[text](#evidence-…)` links, in order (the paragraph's own link). */
  link_index: number;
  evidence_id: string;
  /** The link text: the company or asset the grid is about (at most 80 characters). */
  subject: string;
};

/** What an as-of answer could not use (finance-point-in-time.md §7). */
export type AsOfReport = Wire.AsOfReport;

/** The record each kind of host-posted Thread message carries. */
export type HostPostRecords = {
  /** Present on host-posted Thread messages. */
  delivery?: HomeDelivery;
  /** Present on a host-posted crossed-line message. */
  alert?: HomeAlert;
  /** Present on a host-posted watcher message. */
  watch?: HomeWatch;
  /** Present on a host-posted results message. */
  earnings?: HomeEarnings;
  /** Present on a host-posted "your map now also tracks" message. */
  vocabulary?: HomeVocabulary;
  /** Present on a host-posted research hand-off card. */
  handoff?: HomeHandoff;
  /** Present on a host-posted "New link" card from discovery. */
  link?: HomeLink;
};

export type ChatMessage = HostPostRecords & {
  tool_refusals?: ChatToolRefusal[];
  feedback?: ChatFeedback | null;
  failure_kind?: ChatFailureKind;
  /** For a busy (`rate_limited`) answer: how long the provider asked to wait, bounded by Rust. */
  retry_after_seconds?: number;
  host_composed?: boolean;
  /** Guidance accepted while a reply ran; it shows as a "Guided" row, not a bubble. */
  steer?: boolean;
  /** A data read in this reply never reached its source (the network or that provider was unreachable). */
  offline_reads?: boolean;
  /** The chosen model's name when its provider no longer offered it and Rust answered on Auto (W17.5). */
  replaced_model?: string;
  /** Asset reads this reply's tool accepted but its commit did not save (the page changed first). */
  unsaved_theses?: UnsavedThesis[];
  /** The as-of date the request was sent with (`YYYY-MM-DD`), on the request and its answer. */
  as_of?: string;
  /** Number links Rust checked against their evidence when the answer finished. */
  number_checks?: NumberCheck[];
  /** Grid results the answer linked as paragraphs of their own, copied by Rust when the answer finished. */
  sensitivity_cards?: SensitivityCard[];
  /** What an as-of answer could not use. */
  as_of_report?: AsOfReport;
  id: string;
  role: "user" | "assistant";
  content: string;
  /** `content` is the host's failure placeholder, not model text; the run state explains the failure. */
  placeholder?: boolean;
  created_at: string;
  status?: "streaming" | "complete" | "aborted" | "error";
  memory_refs?: MemoryRef[];
  lens_receipts?: import("./memory").LensReceipt[];
  evidence_refs?: ChatEvidenceRef[];
  schedule_receipts?: ScheduleReceipt[];
  observed_exact_count?: number;
  refs_truncated?: boolean;
  refs_digest?: string;
  memory_update?: MemoryUpdateResult;
  memory_revision?: string;
  execution_model?: ExecutionModelLock;
  agent_harness?: AgentHarnessSnapshot;
  progress?: ChatProgress;
  /** Live run phase. Not stored with the Chat message. */
  phase?: ChatRunPhase;
  /**
   * The commentary the host is writing now, how long the answer was when it
   * began, and whether its settled text is on its way into the answer. Not
   * stored with the Chat message.
   */
  draft?: { id: string; answerLength: number; settled: boolean };
  /**
   * The id of the draft the answer first settled from. The answer's Markdown
   * node keeps this key from draft to answer and across a refresh from the
   * store. Not stored with the Chat message.
   */
  answer_key?: string;
  attachments?: ChatAttachment[];
  artifact_refs?: ChatArtifactRef[];
  /**
   * Charts and tables this streaming reply published, as the host delivered
   * them before the commit, so their cards fill in place. Shown only while
   * the reply streams or once it completes; not stored with the Chat message.
   */
  staged_artifacts?: ChatArtifactView[];
  performance?: ChatPerformance;
};

export type ChatPerformance = Wire.ChatPerformance;

export type ChatAttachment = {
  id: string;
  filename: string;
  media_type: string;
  size_bytes: number;
  url?: string;
};

export type ChatAttachmentInput = Wire.ChatAttachmentInput;

export type ChatMemorySetAsideRequest = Wire.ChatMemorySetAsideRequest;

export type ResearchHandoffRequest = {
  loupistId: string;
  title: string;
  question: string;
  brief?: string;
  pins?: Array<{ lens: string } | { world: string }>;
  requestKey: string;
};
export type ResearchHandoffReceipt = Wire.ResearchHandoffReceipt;

export type ChatThread = {
  id: string;
  loupist_id: string;
  /** The Loupist's home Thread: listed first, landed on, never renamed or deleted. */
  home?: boolean;
  title: string;
  updated_at: string;
  use_memory: boolean;
  update_memory: boolean;
  /** Principles and methods the user set aside for this chat ("Not this time"). */
  set_aside_memory?: string[];
  /** Edit forks (X18): the other versions of this chat's tail. */
  versions?: ChatVersionSummary[];
  messages: ChatMessage[];
};

/** Another version of a chat's tail; it shares every message up to `after_message_id`. */
export type ChatVersionSummary = {
  id: string;
  after_message_id: string | null;
  first_message_at_ms: number;
  message_count: number;
};

/** `position` and `content` find a just-sent message that still carries the renderer's own id. */
export type ChatBranchRequest = { loupist_id: string; thread_id: string; message_id: string; position?: number; content?: string };
export type ChatVersionSwitchRequest = Wire.ChatVersionSwitchRequest;

export type LoupistWorkspace = {
  loupist: LoupistSummary;
  threads: ChatThread[];
};

export type ChatCreateRequest = {
  loupist_id: string;
  title?: string;
};

export type ChatRenameRequest = Wire.ChatRenameRequest;

export type ChatDeleteRequest = Wire.ChatDeleteRequest;

export type ChatKnowledgePin = {
  selection?: import("./knowledge").SourceTextSelection;
  kind: string;
  recordId: string | null;
  revisionId: string | null;
  objectId: string;
  spanId: string | null;
  label: string;
};

export type ChatArtifactSelection = Wire.ChatArtifactSelection;

export type ChatRetryRequest = Wire.ChatRetryRequest;

export type ChatSendRequest = {
  run_id: string;
  loupist_id: string;
  thread_id: string;
  prompt: string;
  use_memory: boolean;
  update_memory: boolean;
  as_of?: string;
  model_profile_id: string;
  thinking_level: string;
  run_options: Record<string, string>;
  /** Composer `@entry_id` tokens; Rust keeps only enabled execute-ready ones. */
  mentioned_connection_ids?: string[];
  attachments: ChatAttachmentInput[];
  pins?: ChatKnowledgePin[];
  selected_artifact?: ChatArtifactSelection;
};

export type ChatControlRequest = {
  loupist_id: string;
  thread_id: string;
  prompt: string;
};

export type ChatControlReceipt = {
  message_id: string;
  prompt: string;
  created_at: string;
  mode: "steer";
};

/** One event on a Chat run's stream, as the renderer reads it (`ChatStreamEvent` in Rust). */
export type ChatStreamEvent =
  | { type: "started"; run_id: string }
  | { type: "memory"; run_id: string; memories: MemoryRef[] }
  | { type: "schedule"; run_id: string; receipt: ScheduleReceipt }
  | { type: "evidence"; run_id: string; evidence: ChatEvidenceRef }
  | {
      type: "delta";
      run_id: string;
      text: string;
      /** The live commentary this settled text replaces; its copy leaves in the same update. */
      settles?: string;
    }
  | {
      type: "generation_finished";
      run_id: string;
      performance: ChatPerformance;
    }
  | { type: "title"; run_id: string; title: string }
  | {
      type: "progress";
      run_id: string;
      sequence: number;
      progress: ChatProgress;
    }
  | { type: "progress_patch"; run_id: string; patch: ChatProgressPatch }
  | { type: "phase"; run_id: string; phase: ChatRunPhase }
  | {
      type: "memory_update";
      run_id: string;
      result: MemoryUpdateResult;
    }
  /** A chart or table delivered before the reply commits; `artifact` follows only if it does. */
  | { type: "artifact_staged"; run_id: string; view: ChatArtifactView }
  | {
      type: "artifact";
      run_id: string;
      artifact: ChatArtifactRef;
    }
  | {
      type: "completed";
      lens_receipts: import("./memory").LensReceipt[];
      tool_refusals: ChatToolRefusal[];
      offline_reads?: boolean;
      unsaved_theses?: UnsavedThesis[];
      host_composed: boolean;
      run_id: string;
      message_id: string;
      final_text: string;
      created_at: string;
      execution_model: ExecutionModelLock;
      agent_harness: AgentHarnessSnapshot;
      performance: ChatPerformance;
      replaced_model?: string;
      as_of?: string;
      number_checks?: NumberCheck[];
      /** The answer's sensitivity cards, as `ChatMessage.sensitivity_cards`. */
      sensitivity_cards?: SensitivityCard[];
      as_of_report?: AsOfReport;
    }
  | { type: "aborted"; run_id: string }
  | {
      type: "error";
      failure_kind: ChatFailureKind;
      /** For a busy answer: the provider's bounded retry hint in seconds. */
      retry_after_seconds?: number;
      run_id: string;
      message: string;
      message_id: string;
      final_text: string;
      /** `final_text` is the host's failure placeholder, not model text. */
      placeholder?: boolean;
      created_at: string;
      execution_model: ExecutionModelLock;
      agent_harness: AgentHarnessSnapshot;
      progress?: ChatProgress;
    };


export const CHART_SCHEMA = "loupist-chart/1";
export const GRID_SCHEMA = "loupist-grid/1";
