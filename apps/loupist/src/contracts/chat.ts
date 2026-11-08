import type { ScheduleReceipt, ScheduledCadence, ScheduledRun, ScheduledTaskKind } from "./scheduled";
import type { LoupistSummary } from "./loupist";
import type { MemoryRef, MemoryUpdateResult } from "./memory";
import type { AgentHarnessSnapshot, ExecutionModelLock } from "./model";
import type { EarningsMeasure } from "./earnings";
import type { WatchEvent } from "./watch";
import type { LearnedDigest } from "./lens";

export type ChatProgressStatus =
  | "running"
  | "succeeded"
  | "failed"
  | "stopped"
  /** A source that cannot show what was known on the answer's as-of date; not a failure. */
  | "skipped";

export type ChatProgressCategory =
  | "web"
  | "memory"
  | "capability"
  | "finance"
  | "files"
  | "artifact"
  | "compute"
  | "system";

export type ChatProgressSourceKind =
  | "web"
  | "memory"
  | "files"
  | "finance"
  | "connector"
  | "artifact"
  | "compute"
  | "capability";

export type ChatProgressSource = {
  kind: ChatProgressSourceKind;
  name: string;
};

export type ChatRunPhase = "thinking" | "writing" | "tool";

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
      target?: string;
      href?: string;
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
      startedAtMs?: number;
      finishedAtMs?: number;
    };

/**
 * Why Rust refused a failed step. `untrusted_text_paused`: the answer read third-party text,
 * so a command without a verified sandbox, an instruction write, a connection, a schedule
 * change or a new site stayed off. `protected_path`: the path is inside the app's private
 * data, a credential store or a git directory's hooks or config. `outside_workspace`: the
 * path is outside the Loupist folder.
 */
export type ChatStepRefusal = "untrusted_text_paused" | "protected_path" | "outside_workspace";

export type ChatPlanStatus = "pending" | "active" | "done" | "not_done";

export type ChatPlanItem = {
  id: string;
  label: string;
  status: ChatPlanStatus;
  /** When the item first became active; steps that start after it nest under it. */
  activatedAtMs?: number;
};

/** The agent's checklist for multi-step work, projected by the host from `plan_update`. */
export type ChatPlan = { items: ChatPlanItem[] };

/**
 * A read-only connection tool the reply waits to use until the user decides (composer D3),
 * or, with `kind: web_site`, a new website after the reply read third-party text (`target`
 * names the site and path), which can be allowed only once.
 */
export type ChatApproval = {
  requestId: string;
  kind?: "web_site";
  /** The progress step that waits, when known. */
  itemId?: string;
  connectionId: string;
  connectionName: string;
  toolName: string;
  /** Plain values from the call (company, period); never raw arguments. */
  target?: string;
  state: "waiting" | "allowed_once" | "always_allowed" | "declined" | "expired";
  requestedAtMs: number;
};

export type ChatApprovalDecision = "once" | "always" | "deny";

/** One question the agent asked mid-reply (`ask_user`): 2 to 4 options and always a free-text answer. */
export type ChatQuestion = {
  requestId: string;
  /** The progress step that waits, when known. */
  itemId?: string;
  question: string;
  options: { label: string; description?: string }[];
  /** Index into `options` the agent suggests and assumes when nobody answers. */
  recommended?: number;
  state: "waiting" | "answered" | "expired";
  answer?: { choice: number } | { text: string };
  requestedAtMs: number;
};

export type ChatProgress = {
  startedAtMs: number;
  finishedAtMs?: number;
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

export type ChatArtifactKind = "markdown" | "chart" | "grid";

export type ChatArtifactRef = {
  /** Renderer selection; included in the user message, never in the authority receipt. */
  selected_range?: { start: string; end: string };
  artifact_id: string;
  revision: number;
  kind: ChatArtifactKind;
  title: string;
  digest: string;
};

export type ChatArtifact = {
  id: string;
  chat_session_id: string;
  kind: ChatArtifactKind;
  title: string;
  current_revision: number;
  created_at_ms: number;
  updated_at_ms: number;
};

export type ChartColumn = {
  id: string;
  label: string;
  kind: "string" | "number" | "boolean" | "date" | "datetime";
};

export type ChartResultReceipt = {
  result_ref: string;
  runtime_id: string;
  tool_name: string;
  provider: string | null;
  request_digest: string;
  response_digest: string;
  retrieved_at: string;
  warnings: string[];
  upstream_result_refs: string[];
};

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

export type ChartStudy = {
  module_id:
    | "AVP"
    | "AO"
    | "BIAS"
    | "BOLL"
    | "BRAR"
    | "BBI"
    | "CCI"
    | "CR"
    | "DMA"
    | "DMI"
    | "EMV"
    | "EMA"
    | "MTM"
    | "MA"
    | "MACD"
    | "OBV"
    | "PVT"
    | "PSY"
    | "ROC"
    | "RSI"
    | "SMA"
    | "KDJ"
    | "SAR"
    | "TRIX"
    | "VOL"
    | "VR"
    | "WR";
  calc_params: number[];
};

export type ChartDrawing = {
  kind:
    | "segment"
    | "ray"
    | "line"
    | "horizontal_line"
    | "vertical_line"
    | "price_line"
    | "fibonacci"
    | "horizontal_segment"
    | "horizontal_ray"
    | "vertical_segment"
    | "vertical_ray"
    | "parallel_line"
    | "price_channel"
    | "annotation"
    | "rectangle"
    | "arrow"
    | "measure"
    | "fibonacci_extension"
    | "long_position"
    | "short_position";
  points: Array<{ timestamp: string | number; value: number }>;
  color?: string;
  line_width?: number;
  line_style?: "solid" | "dashed";
  label?: string;
};

export type ChartCustomSeries = {
  id: string;
  label: string;
  column: string;
  pane?: string;
  color?: string;
};

export type ChartDocument = {
  dataset_id: string;
  dataset_digest: string;
  view:
    | {
        kind: "candlestick";
        symbol: string;
        interval: string;
        time: string;
        open: string;
        high: string;
        low: string;
        close: string;
        volume?: string;
        turnover?: string;
        price_precision?: number;
      }
    | {
        kind: "line" | "area" | "bar" | "scatter";
        x: string;
        y: string[];
        color?: string;
        semantic_types: Record<string, string>;
        title?: string;
        subtitle?: string;
      };
  custom_series?: ChartCustomSeries[];
  studies: ChartStudy[];
  drawings: ChartDrawing[];
  note?: string;
};

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
export type GridUnit = "amount" | "percent" | "multiple" | "number" | "date";
/** Why a grid cell is empty; never a zero. */
export type GridEmptyReason = "not_reported" | "not_applicable" | "unavailable" | "not_checked";
/** Why sources disagree (finance-korea-retail-disagreement.md §3.3). */
export type ConflictReason = "basis" | "period" | "preliminary_vs_final" | "estimate_vs_reported" | "currency_or_unit" | "adjusted_vs_reported" | "timing" | "unclear";

/** One asset row; `asset_id` is set when Rust resolved the asset page. */
export type GridRow = { symbol: string; market: string; label: string; asset_id?: string };
export type GridColumn = { id: string; label: string; period?: string; basis?: string; unit: GridUnit };
/** A value with the evidence it cites and Rust's check of it; `sort` is its base-unit value. */
export type GridValueCell = { value: string; evidence_id: string; check: NumberCheck; sort?: number };
export type GridEmptyCell = { empty: GridEmptyReason; note?: string };
export type GridCell = GridValueCell | GridEmptyCell;

export type AssetsGrid = {
  layout: "assets";
  subtitle?: string;
  notes?: string[];
  rows: GridRow[];
  columns: GridColumn[];
  cells: GridCell[][];
  as_of?: string;
};

/** One source's report; `dated` comes from the evidence, `check` only for a number. */
export type ConflictReport = { evidence_id: string; says: string; basis?: string; dated?: string; check?: NumberCheck };

export type ConflictGrid = {
  layout: "conflict";
  measure: { label: string; entity_label: string; period?: string };
  reports: ConflictReport[];
  reason: ConflictReason;
  explanation: string;
  used?: number;
  used_reason?: string;
  world_entity_id?: string;
  /** Rust found a disputed claim about the entity: the card links the Map. */
  map_link?: boolean;
  as_of?: string;
};

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
export type ChatArtifactVersion = {
  revision_id: number;
  title: string;
  digest: string;
  source_message_id: string;
  created_at_ms: number;
  restored_from_revision_id?: number;
};

export type ChatArtifactRestoreRequest = {
  loupist_id: string;
  thread_id: string;
  artifact_id: string;
  revision_id: number;
  expected_current_revision: number;
};

export type ChatArtifactView = {
  artifact: ChatArtifact;
  revision: ChatArtifactRevision;
  chart_dataset?: ChartDataset;
  /** A grid's cited evidence, from the reply that published the revision. */
  evidence_refs?: ChatEvidenceRef[];
};

/** Immutable, host-issued excerpt from an acknowledged tool result. */
export type ChatEvidenceRef = {
  id: string;
  title: string;
  kind: "original" | "discovery" | "derived" | "local_file" | "saved_context";
  url?: string;
  provider?: string;
  tool_name: string;
  read_ref?: string;
  retrieved_at: string;
  published_at?: string;
  as_of?: string;
  coverage?: string;
  excerpt: string;
  excerpt_truncated: boolean;
  excerpt_match?: { start: number; end: number };
  content_sha256: string;
  warnings: string[];
  dataset_ref?: { dataset_id: string; revision: number; digest: string };
  model_delivery?: {
    coverage: "bounded_sample";
    excerpt: string;
    excerpt_truncated: boolean;
    content_sha256: string;
  };
  upstream_evidence_ids?: string[];
};

export type ChatFeedback = { rating: "up" | "down" | null; correction: string | null; updatedAt: number };
export type ChatFeedbackRequest = { loupist_id: string; thread_id: string; message_id: string; rating: ChatFeedback["rating"]; correction: string | null };

export type ChatFailureKind = "empty_reply" | "provider_error" | "rate_limited" | "needs_reconnect" | "plan_limit" | "plan_not_eligible" | "out_of_credit" | "model_gone" | "offline" | "provider_unavailable" | "context_too_long" | "provider_timeout" | "pi_exited" | "protocol_error" | "output_limit" | "memory_commit_failed" | "store_failed" | "cancelled_by_conflict" | "other";

export type ChatToolRefusal = { connection_id: string; connection_name: string; tool_name: string; reason: "needs_approval" | "blocked_unattended" | "taint" | "sensitive_memory" | "declined" };

/** A research session the Bot handed off (Rust `HomeHandoff`): one card per hand-off, its status changing in place. */
export type HomeHandoff = {
  handoffId: string;
  chatId: string;
  title: string;
  status: "running" | "done" | "failed" | "stopped";
  learned?: LearnedDigest;
};

/** One hop of a discovered link's path: the name reached and the relation walked, against its claim when `reversed`. */
export type HomeLinkHop = { name: string; relation: string; reversed?: boolean };

/**
 * A link discovery found new to this Loupist (Rust `HomeLink`): seed to far end
 * through the path's names. `nodeIds` (seed first) highlight the path on the Map.
 */
export type HomeLink = {
  connectionId: string;
  seedName: string;
  farName: string;
  path: HomeLinkHop[];
  nodeIds: string[];
};

/** A scheduled result the host posted into the Loupist's home Thread. */
export type HomeDelivery = {
  run_id: string;
  task_id: string;
  task_title: string;
  task_kind: ScheduledTaskKind;
  /** Null once the task was deleted. */
  schedule: ScheduledCadence | null;
  run_thread_id: string | null;
  status: ScheduledRun["status"];
  finished_at_ms: number;
  learned?: LearnedDigest;
};

/** A crossed red line the host announced in the home Thread (Rust `HomeAlert`). Names and the user's claim only. */
export type HomeAlert = {
  asset_id: string;
  move_id: string;
  name: string;
  symbol: string;
  market: string;
  claim: string;
  /** The observed value with its unit ("27%"). */
  value: string;
  /** When the user wrote the line. */
  set_at_ms: number;
  fact_date: string;
  source_url?: string;
  source_title: string;
};

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
export type UnsavedThesis = { assetId: string; name: string; reason: string };

/** How Rust found a number link against its evidence (finance-show-the-math.md §7.2). */
export type NumberCheckStatus = "matched" | "rounded" | "not_found" | "differs";
export type NumberCheckKind = "calculated" | "passage" | "data" | "step";

/** One number link in a finished answer, checked by Rust. The renderer only reads it. */
/** How Rust says a `calculate` input is printed (`CalcInputUnit`): an amount carries its ISO currency and power-of-ten scale. */
export type CalcInputUnit = {
  kind: "number" | "percent" | "points" | "basis_points" | "multiple" | "amount" | "price";
  currency?: string;
  scale?: number;
  /** A unit Rust could not place ("stores"), printed after the number. */
  text?: string;
};

/** One `calculate` input as Rust copied it into a check: its formula name (`label`), unit and source. */
export type CalcInput = { name: string; label: string; unit: CalcInputUnit; display: string; evidence_id?: string; status?: NumberCheckStatus };

export type NumberCheck = {
  /** Index among the answer's `[text](#evidence-…)` links, in order. */
  link_index: number;
  text: string;
  evidence_id: string;
  kind: NumberCheckKind;
  status: NumberCheckStatus;
  /** `differs`: the calculated value nearest to the printed number. */
  nearest?: string;
  /** `rounded`: the value as the evidence prints it. */
  exact?: string;
  /** The matched data cell (JSON Pointer into the retained result). */
  pointer?: string;
  /** The matched passage text (UTF-8 byte offsets into the evidence excerpt). */
  span?: { start: number; end: number };
  /** Up to four fields of the matched data row. */
  fields?: { key: string; value: string; matched?: boolean }[];
  /** The host's own calculation: title, formula lines and inputs. */
  calc?: {
    title: string;
    formulas: string[];
    inputs: CalcInput[];
    /** Rows × columns of a DCF grid, which has no single value. */
    grid?: [number, number];
  };
};

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
export type AsOfReport = {
  cutoff: string;
  refused?: { tool: string; connection?: string }[];
  empty?: { tool: string; target?: string }[];
  left_out: { facts: number; sources: number; notes: number; methods: number };
};

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
  performance?: ChatPerformance;
};

export type ChatPerformance = {
  setupMs: number;
  firstTextMs?: number;
  generationMs: number;
  totalMs: number;
  sessionCache: "cold" | "warm";
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
};

export type ChatAttachment = {
  id: string;
  filename: string;
  media_type: string;
  size_bytes: number;
  url?: string;
};

export type ChatAttachmentInput = {
  filename: string;
  media_type: string;
  data_base64: string;
};

export type ChatMemorySetAsideRequest = { loupist_id: string; thread_id: string; record_id: string; set_aside: boolean };

export type ResearchHandoffRequest = {
  loupistId: string;
  title: string;
  question: string;
  brief?: string;
  pins?: Array<{ lens: string } | { world: string }>;
  requestKey: string;
};
export type ResearchHandoffReceipt = { chatId: string; handoffId: string };

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
export type ChatVersionSwitchRequest = { loupist_id: string; thread_id: string; version_id: string };

export type LoupistWorkspace = {
  loupist: LoupistSummary;
  threads: ChatThread[];
};

export type ChatCreateRequest = {
  loupist_id: string;
  title?: string;
};

export type ChatRenameRequest = {
  loupist_id: string;
  thread_id: string;
  title: string;
};

export type ChatDeleteRequest = {
  loupist_id: string;
  thread_id: string;
};

export type ChatKnowledgePin = {
  selection?: import("./knowledge").SourceTextSelection;
  kind: string;
  recordId?: string;
  revisionId?: string;
  objectId: string;
  spanId?: string;
  label: string;
};

export type ChatArtifactSelection = Pick<ChatArtifactRef, "artifact_id" | "revision">;

export type ChatRetryRequest = { loupist_id: string; thread_id: string };

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
      sequence?: number;
      progress: ChatProgress;
    }
  | { type: "progress_patch"; run_id: string; patch: ChatProgressPatch }
  | { type: "phase"; run_id: string; phase: ChatRunPhase }
  | {
      type: "memory_update";
      run_id: string;
      result: MemoryUpdateResult;
    }
  | {
      type: "artifact";
      run_id: string;
      artifact: ChatArtifactRef;
    }
  | {
      type: "completed";
      lens_receipts?: import("./memory").LensReceipt[];
      tool_refusals?: ChatToolRefusal[];
      offline_reads?: boolean;
      unsaved_theses?: UnsavedThesis[];
      host_composed?: boolean;
      run_id: string;
      message_id: string;
      final_text: string;
      created_at: string;
      execution_model: ExecutionModelLock;
      agent_harness: AgentHarnessSnapshot;
      performance?: ChatPerformance;
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
      failure_kind?: ChatFailureKind;
      /** For a busy answer: the provider's bounded retry hint in seconds. */
      retry_after_seconds?: number;
      run_id: string;
      message: string;
      message_id: string;
      final_text: string;
      created_at: string;
      execution_model: ExecutionModelLock;
      agent_harness: AgentHarnessSnapshot;
      progress?: ChatProgress;
    };

export const CHART_SCHEMA = "loupist-chart/1";
export const GRID_SCHEMA = "loupist-grid/1";
