import i18n from "@/i18n";
import { formatNumber } from "@/i18n/format";
import { useState } from "react";
import type {
  ChatApproval,
  ChatEvidenceRef,
  ChatFailureKind,
  ChatMessage,
  ChatPlan,
  ChatProgress,
  ChatProgressItem,
  ChatQuestion,
  MemoryRef,
  NumberCheck,
} from "../types";
import { citationSources, sourceIdentityLabel } from "./evidence";
import { exactMemoryReads, isInlineWorkEvidence } from "./memoryActivity";
import { readNoun } from "./progressLabels";
import { actionLabel, isActionItem, visibleTarget, withoutIdentifiers } from "./runStatus";

const t = i18n.getFixedT(null, "chat");

/** A run with no new progress for this long reads as "Still working" instead of a frozen label. */
export const STALL_AFTER_MS = 15_000;

/** A note under this many characters, in one paragraph, gives its place to the next text (DIRECTION 4.1). */
const SHORT_NOTE_CHARS = 280;

/** Seconds before the automatic retry after the first and second busy answer when the provider gave no hint; the third asks to try later. */
export const BUSY_RETRY_SECONDS = [20, 40] as const;

type ActionItem = Extract<ChatProgressItem, { kind: "tool" | "system" }>;

export type TurnInput = {
  message: ChatMessage;
  now: number;
  /** When the host last reported a step, phase, plan or approval change; streamed text does not count. */
  lastActivityAt: number;
  /** A local stop was requested and the host has not confirmed it. */
  stopping: boolean;
  /** Busy answers in a row in this Chat, counting this one, and whether this one can be retried. */
  retry: { attempt: number; offered: boolean };
  /**
   * The host finished, but the answer's last words are still being revealed:
   * the turn reads as running (Writing) until they are, at most 400 ms.
   */
  holding?: boolean;
};

/** Exactly one state applies; the live line and the footer derive from it. */
export type TurnState =
  | { kind: "running" }
  /** An approval or a question waits for the user. */
  | { kind: "awaiting_approval" }
  | { kind: "stopping" }
  | { kind: "complete" }
  | { kind: "stopped"; kept: "steps_and_text" | "steps_and_draft" | "steps" | "text" | "draft" | "nothing" }
  /** A busy answer counting down to its own retry (`retryPending`) has not failed yet. */
  | { kind: "failed"; failure: ChatFailureKind; retryPending: boolean };

/** The answer, or a note: text a tool call followed. */
export type ProseTone = "answer" | "note";

/** The glyph before the live label and before a step in the timeline (DIRECTION 6.3). */
export type StepGlyph = "chart" | "table" | "search" | "calculate" | "finance" | "memory" | "web" | "files" | "artifact" | "capability" | "compute" | "system";
export type LiveGlyph = StepGlyph | "phase" | "writing" | "stopping" | "waiting";

/** The one present-tense line of a running turn. */
export type LiveLine = {
  /** Changes when the label or target changes; "+N" and the clock are not part of it. */
  key: string;
  glyph: LiveGlyph;
  label: string;
  target: string | null;
  /** Other steps running beside the one named. */
  more: number;
  sheen: boolean;
  /** `step` holds its label for a moment after the step ends, so a quick next step never reads "Thinking". */
  kind: "starting" | "thinking" | "still" | "step" | "writing" | "waiting" | "stopping";
};

/** What the answer can cite: a site read, a data series, saved notes, calculations. */
export type LedgerChip = { key: string; kind: "source" | "data" | "memory" | "calculation"; label: string; count: number; evidence?: ChatEvidenceRef };

export type LedgerView = {
  /** The step count (or the plan position) while running; the receipt once settled. */
  text: string;
  /** " · 1 failed attempt", in both states; never an alarm. */
  failed: string | null;
  settled: boolean;
  chips: LedgerChip[];
};

export type TurnSegment =
  | { kind: "prose"; key: string; text: string; tone: ProseTone; streaming: boolean; checks?: NumberCheck[]; short?: boolean }
  | {
      kind: "ledger";
      key: string;
      ledger: LedgerView;
      /** Every counted step and note of the turn, in order; the plan and approvals for the timeline. */
      progress: ChatProgress;
      /** Calculations and queries whose result opens inline under the step. */
      evidence: ChatEvidenceRef[];
      /** Distinct citation subjects the turn's steps read. */
      sources: ChatEvidenceRef[];
      memoryReads: MemoryRef[];
      live: boolean;
    }
  /** A chart or table card, where its publish step started. `chartBlocks`/`gridBlocks` decide its state. */
  | { kind: "card"; key: string; itemId: string; tool: "chart_publish" | "grid_publish" }
  | { kind: "question"; key: string; question: ChatQuestion }
  | { kind: "approval"; key: string; approval: ChatApproval };

/** How a settled turn ended, when not well. A busy answer carries the seconds until it retries itself. */
export type RunState =
  | { kind: "stopped"; label: string }
  | { kind: "failed"; failure: ChatFailureKind; retryInSeconds?: number; kept: boolean };

export type TurnFooter =
  | { kind: "live"; line: LiveLine; sinceMs: number }
  | {
      kind: "settled";
      /** Distinct citation subjects of the whole answer, numbered in this order. */
      sources: ChatEvidenceRef[];
      /** The answer as written, or null when there is nothing worth copying. */
      copyText: string | null;
      feedback: boolean;
      runState: RunState | null;
    };

export type TurnStatus = { state: TurnState; footer: TurnFooter };
export type TurnView = TurnStatus & { segments: TurnSegment[]; announce: string };

type Ledger = Extract<TurnSegment, { kind: "ledger" }>;

/** What a stopped reply kept (S4): finished steps and any text so far. */
export function visibleMessageContent(message: ChatMessage): string {
  if (message.role !== "assistant" || message.status !== "aborted") return message.content;
  const content = message.content.trimEnd();
  if (content === "Response stopped.") return "";
  return content.replace(/\r?\n(?:\r?\n)*Response stopped\.$/u, "").trimEnd();
}

/**
 * The host's terminal placeholder, explained by the run state instead of shown as prose.
 * Rust marks it, for stored replies too (`ChatMessageDto.placeholder`).
 */
const isFailurePlaceholder = (message: ChatMessage) =>
  message.role === "assistant" && message.status === "error" && message.placeholder === true;

const startedAt = (message: ChatMessage) => message.progress?.startedAtMs ?? Date.parse(message.created_at);

/** The plan and the question line stand for these steps; they are not counted and have no row. */
const HIDDEN_TOOLS = new Set(["plan_update", "ask_user"]);
const isCounted = (item: ChatProgressItem): item is ActionItem =>
  isActionItem(item) && !(item.kind === "tool" && item.toolName && HIDDEN_TOOLS.has(item.toolName));
const isCardTool = (tool: string | undefined): tool is "chart_publish" | "grid_publish" => tool === "chart_publish" || tool === "grid_publish";

/** Changes whenever the host reports a step, phase, plan or approval change. */
function activityKey(message: ChatMessage | undefined): string {
  if (!message) return "";
  const progress = message.progress;
  const items = (progress?.items ?? []).flatMap((item) =>
    isActionItem(item) ? [`${item.id}:${item.status}:${item.outputRevision ?? 0}`] : []);
  const plan = (progress?.plan?.items ?? []).map((item) => `${item.id}:${item.status}`);
  const approvals = (progress?.approvals ?? []).map((approval) => `${approval.requestId}:${approval.state}`);
  const questions = (progress?.questions ?? []).map((question) => `${question.requestId}:${question.state}`);
  return `${message.phase ?? ""}|${items.join(",")}|${plan.join(",")}|${approvals.join(",")}|${questions.join(",")}`;
}

/** When the host last reported progress for this message, on this renderer's clock. */
export function useLastActivity(message: ChatMessage | undefined): number {
  const key = activityKey(message);
  const [changed, setChanged] = useState(() => ({ key, at: Date.now() }));
  if (changed.key !== key) setChanged({ key, at: Date.now() });
  return changed.at;
}

/** A step Rust refused because the answer had read third-party text (`untrusted_text_paused`). */
export const pausedByRule = (item: ActionItem) =>
  item.kind === "tool" && item.status === "failed" && item.refusal === "untrusted_text_paused";

/** The rule a failed step was refused under, when Rust named one. */
export const refusedByRule = (item: ActionItem) =>
  item.kind === "tool" && item.status === "failed" ? item.refusal : undefined;

/**
 * Steps that started while a plan item was active nest under that item, in
 * activation order; steps before the first activation stay in the flat list.
 */
export const planSections = (plan: ChatPlan | undefined, items: ChatProgressItem[]) => {
  const nested = new Map<string, ChatProgressItem[]>();
  if (!plan) return { nested, rest: items };
  const windows = plan.items
    .flatMap((item) => item.activatedAtMs === undefined ? [] : [{ id: item.id, start: item.activatedAtMs }])
    .sort((left, right) => left.start - right.start);
  const owner = (at?: number) => {
    let found: string | undefined;
    if (at === undefined) return found;
    for (const window of windows) {
      if (at < window.start) break;
      found = window.id;
    }
    return found;
  };
  const rest: ChatProgressItem[] = [];
  for (const item of items) {
    const id = item.kind !== "commentary" ? owner(item.startedAtMs) : undefined;
    if (!id) rest.push(item);
    else {
      const steps = nested.get(id);
      if (steps) steps.push(item);
      else nested.set(id, [item]);
    }
  }
  return { nested, rest };
};

/** One rule for the live line, the chips and the timeline (DIRECTION 6.3). */
export function stepGlyph(item: ActionItem): StepGlyph {
  const tool = item.kind === "tool" ? item.toolName : undefined;
  if (tool === "chart_publish") return "chart";
  if (tool === "grid_publish") return "table";
  if (item.operation === "search") return "search";
  if (item.operation === "calculate" || tool === "run_python") return "calculate";
  return item.category;
}

/** "34s", "1m 12s" (KO "34초", "1분 12초"). */
export function receiptDuration(milliseconds: number): string {
  const total = Math.max(1, Math.round(milliseconds / 1_000));
  if (total < 60) return t("ledger.seconds", { s: formatNumber(total) });
  return t("ledger.minutes", { m: formatNumber(Math.floor(total / 60)), s: formatNumber(total % 60) });
}

const capitalized = (value: string) => value.charAt(0).toLocaleUpperCase(i18n.language) + value.slice(1);

/**
 * What the answer can cite, in the order it arrived: each distinct site a step
 * read, data series, saved notes and calculations. A step whose result is a
 * citation source gets the source chip only. Repeats count beside the label.
 */
function ledgerChips(actions: ActionItem[], evidenceById: Map<string, ChatEvidenceRef>): LedgerChip[] {
  const chips = new Map<string, LedgerChip>();
  const add = (chip: Omit<LedgerChip, "count">) => {
    const existing = chips.get(chip.label);
    if (existing) existing.count += 1;
    else chips.set(chip.label, { ...chip, count: 1 });
  };
  for (const item of actions) {
    if (item.status !== "succeeded" || item.kind !== "tool") continue;
    const sources = citationSources((item.resultIds ?? []).flatMap((id) => evidenceById.get(id) ?? []));
    if (sources.length) {
      for (const source of sources) {
        // Saved notes read through a citation are the notes, never a product name.
        if (source.tool_name.startsWith("world")) add({ key: `memory:${item.id}`, kind: "memory", label: capitalized(readNoun("world_read")) });
        else add({ key: `source:${source.id}`, kind: "source", label: sourceIdentityLabel(source), evidence: source });
      }
      continue;
    }
    const tool = item.toolName ?? "";
    if (item.operation === "calculate" || tool === "run_python") add({ key: `calculation:${item.id}`, kind: "calculation", label: capitalized(readNoun("calculate")) });
    else if (item.category === "memory" && item.operation === "read") add({ key: `memory:${item.id}`, kind: "memory", label: capitalized(readNoun(tool)) });
    else if (item.category === "finance" && item.operation === "read") add({ key: `data:${item.id}`, kind: "data", label: capitalized(readNoun(tool)) });
  }
  return [...chips.values()];
}

function ledgerView(ledger: Ledger, state: TurnState, running: boolean, durationMs: number | undefined, evidenceRefs: readonly ChatEvidenceRef[]): LedgerView {
  const actions = ledger.progress.items.filter(isCounted);
  const failedCount = actions.filter((item) => item.status === "failed" && !pausedByRule(item)).length;
  const failed = failedCount > 0 ? t("turn.research.failedAttempt", { count: failedCount }) : null;
  const evidenceById = new Map(evidenceRefs.map((item) => [item.id, item]));
  const chips = ledgerChips(actions, evidenceById);
  const steps = (count: number) => count > 0 ? t("ledger.steps", { count, formattedCount: formatNumber(count) }) : "";
  if (running) {
    const plan = ledger.progress.plan;
    const active = plan?.items.findIndex((item) => item.status === "active") ?? -1;
    const done = plan ? plan.items.map((item) => item.status).lastIndexOf("done") : -1;
    const at = active >= 0 ? active : done;
    const text = plan && at >= 0
      ? t("ledger.plan", { k: formatNumber(at + 1), n: formatNumber(plan.items.length), label: withoutIdentifiers(plan.items[at]!.label) || plan.items[at]!.label })
      : steps(actions.length) || t("chatProgressTimeline.consultedNotes", { count: ledger.memoryReads.length, formattedCount: formatNumber(ledger.memoryReads.length) });
    return { text, failed, settled: false, chips };
  }
  const stopped = state.kind === "stopped";
  const counted = stopped ? actions.filter((item) => item.status === "succeeded").length : actions.length;
  const lead = durationMs === undefined ? "" : stopped
    ? t("ledger.stoppedAfter", { duration: receiptDuration(durationMs) })
    : t("ledger.worked", { duration: receiptDuration(durationMs) });
  const sources = ledger.sources.length ? t("ledger.sources", { count: ledger.sources.length, formattedCount: formatNumber(ledger.sources.length) }) : "";
  const text = [lead, steps(counted), sources].filter(Boolean).join(" · ")
    || t("chatProgressTimeline.consultedNotes", { count: ledger.memoryReads.length, formattedCount: formatNumber(ledger.memoryReads.length) });
  return { text, failed, settled: true, chips };
}

function keptByStop(message: ChatMessage, content: string): Extract<TurnState, { kind: "stopped" }>["kept"] {
  const items = message.progress?.items ?? [];
  const steps = items.some((item) => isActionItem(item) && item.status === "succeeded");
  const text = content.trim().length > 0;
  const draft = items.some((item) => item.kind === "commentary" && item.text.trim().length > 0);
  if (steps && text) return "steps_and_text";
  if (steps && draft) return "steps_and_draft";
  if (steps) return "steps";
  if (text) return "text";
  return draft ? "draft" : "nothing";
}

/** What a stopped reply kept (S4), in the reader's words. */
function stoppedLabel(kept: Extract<TurnState, { kind: "stopped" }>["kept"], items: readonly ChatProgressItem[]) {
  const steps = items.filter((item) => isActionItem(item) && item.status === "succeeded").length;
  const values = { count: steps, formattedCount: formatNumber(steps) };
  switch (kept) {
    case "steps_and_text": return t("chatMessageCard.stoppedKept", values);
    case "steps_and_draft": return t("chatMessageCard.stoppedKeptStepsAndDraft", values);
    case "steps": return t("chatMessageCard.stoppedKeptSteps", values);
    case "text": return t("chatMessageCard.stoppedKeptText");
    case "draft": return t("chatMessageCard.stoppedKeptDraft");
    case "nothing": return t("chatMessageCard.stoppedBeforeTheAnswerFinished");
  }
}

function deriveState({ message, stopping, retry, holding }: TurnInput, content: string): TurnState {
  const progress = message.progress;
  if (message.status === "error") {
    const retryPending = message.failure_kind === "rate_limited" && retry.offered && retry.attempt <= BUSY_RETRY_SECONDS.length;
    return { kind: "failed", failure: message.failure_kind ?? "other", retryPending };
  }
  if (message.status === "aborted") return { kind: "stopped", kept: keptByStop(message, content) };
  if (message.status !== "streaming" && !holding) return { kind: "complete" };
  if (stopping) return { kind: "stopping" };
  if ((progress?.approvals ?? []).some((approval) => approval.state === "waiting")
    || (progress?.questions ?? []).some((question) => question.state === "waiting")) return { kind: "awaiting_approval" };
  return { kind: "running" };
}

/** The live line's state table (DIRECTION 4.3); the first match wins. Dwell and the 800 ms hold are the line's own. */
function liveLine(state: TurnState, { message, now, lastActivityAt, holding }: TurnInput, content: string): LiveLine {
  const line = (kind: LiveLine["kind"], glyph: LiveGlyph, label: string, extra: Partial<LiveLine> = {}): LiveLine => {
    const target = extra.target ?? null;
    return { kind, glyph, label, target, more: extra.more ?? 0, sheen: extra.sheen ?? true, key: `${label}|${target ?? ""}` };
  };
  if (state.kind === "stopping") return line("stopping", "stopping", t("chatView.stopping"), { sheen: false });
  if (state.kind === "awaiting_approval") return line("waiting", "waiting", t("turn.waitingForYou"), { sheen: false });
  const stalled = now - lastActivityAt >= STALL_AFTER_MS;
  const items = message.progress?.items ?? [];
  const running = holding ? [] : items.filter((item): item is ActionItem => isCounted(item) && item.status === "running");
  const newest = running.at(-1);
  if (newest) {
    const target = stalled ? t("turn.stillWorking") : visibleTarget(newest.target) ?? null;
    return line("step", stepGlyph(newest), actionLabel(newest), { target, more: running.length - 1 });
  }
  if (holding || message.phase === "writing") return line("writing", "writing", t("turn.writing"));
  if (stalled) return line("still", "phase", t("turn.stillWorking"));
  if (!message.phase && !items.length && !content) return line("starting", "phase", t("turn.starting"));
  return line("thinking", "phase", t("turn.thinking"));
}

function runState(state: TurnState, { message, retry }: TurnInput, content: string): RunState | null {
  if (state.kind === "stopped") return { kind: "stopped", label: stoppedLabel(state.kept, message.progress?.items ?? []) };
  if (state.kind !== "failed") return null;
  const retryInSeconds = state.retryPending ? message.retry_after_seconds ?? BUSY_RETRY_SECONDS[retry.attempt - 1] ?? BUSY_RETRY_SECONDS[0] : undefined;
  const kept = (message.progress?.items ?? []).some((item) => isCounted(item) && item.status === "succeeded")
    || Boolean(content.trim() && !isFailurePlaceholder(message));
  return { kind: "failed", failure: state.failure, retryInSeconds, kept };
}

function status(input: TurnInput) {
  const { message } = input;
  const content = visibleMessageContent(message);
  const state = deriveState(input, content);
  const footer: TurnFooter = state.kind === "complete" || state.kind === "stopped" || state.kind === "failed"
    ? {
        kind: "settled",
        sources: citationSources(message.evidence_refs ?? []),
        copyText: content && !isFailurePlaceholder(message) ? content : null,
        feedback: state.kind === "complete",
        runState: runState(state, input, content),
      }
    : { kind: "live", line: liveLine(state, input, content), sinceMs: startedAt(message) };
  return { state, footer, content };
}

/** The state and footer alone, for a reader that redraws every second (the status pill). */
export function deriveTurnStatus(input: TurnInput): TurnStatus {
  const { state, footer } = status(input);
  return { state, footer };
}

/** A note is short when it is one paragraph under 280 characters: no blank line, heading, list, quote, table or fence. */
export const isShortNote = (text: string) => {
  const trimmed = text.trim();
  return trimmed.length < SHORT_NOTE_CHARS && !/\n\s*\n/u.test(trimmed) && !/(?:^|\n)\s*(?:#|[-*+>]\s|\d+[.)]\s|\||```|~~~)/u.test(trimmed);
};

/**
 * Everything an assistant turn shows, in the order it happened: notes, one
 * ledger for every step of the turn (where the first step happened), cards
 * where their publish step started, questions and approvals where they were
 * asked, the answer, and a footer slot that keeps its height from the live
 * line to the settled action row. New things are appended; nothing is
 * inserted above what is on screen, except that a short note gives its place
 * to the text that follows it.
 */
export function deriveTurnView(input: TurnInput): TurnView {
  const { message } = input;
  const { state, footer, content } = status(input);
  const progress = message.progress;
  const startedAtMs = startedAt(message);
  const items = progress?.items ?? [];
  const streaming = message.status === "streaming";
  const running = footer.kind === "live";
  const evidenceRefs = message.evidence_refs ?? [];
  const lastItem = items.at(-1);
  // The draft is the answer while the host writes it, under the key the answer
  // keeps. Once its settled text has grown the answer, the live copy, which may
  // still be listed for one update, is not drawn again. A draft that closed
  // unsettled (a tool call followed, or the final text came another way) and
  // any older commentary stay notes, wherever a removal leaves them.
  const { draft, answer_key: answerKey } = message;
  const settledCopy = (item: ChatProgressItem) => item.id === draft?.id && draft.settled && content.length > draft.answerLength;
  const liveDraft = streaming && message.phase === "writing" && lastItem?.kind === "commentary" && lastItem.id === draft?.id && !settledCopy(lastItem)
    ? lastItem : undefined;

  const segments: TurnSegment[] = [];
  const pushProse = (segment: Extract<TurnSegment, { kind: "prose" }>) => {
    const last = segments.at(-1);
    // Only where the ledger's timeline keeps it: a turn with no steps keeps its note on the page.
    if (last?.kind === "prose" && last.tone === "note" && last.short && segments.some((segment) => segment.kind === "ledger")) segments.pop();
    segments.push(segment);
  };
  // Notes and counted steps, in the order they happened: the ledger's timeline.
  const timeline: ChatProgressItem[] = [];
  let ledger: Ledger | undefined;
  const placeLedger = (at = segments.length) => {
    if (ledger) return ledger;
    ledger = {
      kind: "ledger",
      key: "activity",
      ledger: { text: "", failed: null, settled: false, chips: [] },
      progress: { startedAtMs, finishedAtMs: progress?.finishedAtMs ?? null, items: timeline },
      evidence: [], sources: [], memoryReads: [], live: running,
    };
    segments.splice(at, 0, ledger);
    return ledger;
  };
  const questions = progress?.questions ?? [];
  const approvals = progress?.approvals ?? [];
  const placed = new Set<string>();
  const placeAsks = (itemId: string) => {
    for (const question of questions) if (question.itemId === itemId && !placed.has(question.requestId)) {
      placed.add(question.requestId);
      segments.push({ kind: "question", key: `question:${question.requestId}`, question });
    }
    for (const approval of approvals) if (approval.itemId === itemId && !placed.has(approval.requestId)) {
      placed.add(approval.requestId);
      segments.push({ kind: "approval", key: `approval:${approval.requestId}`, approval });
    }
  };
  for (const item of items) {
    if (item.kind === "commentary") {
      const text = withoutIdentifiers(item.text);
      if (item === liveDraft) {
        pushProse({ kind: "prose", key: answerKey ?? item.id, text: content ? `${content}\n\n${text}` : text, tone: "answer", streaming: true });
      } else if (!settledCopy(item) && text.trim()) {
        // Every note is listed in the timeline, whether it stays on the page or gives its place.
        timeline.push(item);
        pushProse({ kind: "prose", key: item.id, text, tone: "note", streaming: false, short: isShortNote(text) });
      }
      continue;
    }
    const step: ActionItem = item;
    if (isCounted(step)) {
      placeLedger();
      timeline.push(step);
      if (step.kind === "tool" && isCardTool(step.toolName)) segments.push({ kind: "card", key: `card:${step.id}`, itemId: step.id, tool: step.toolName });
    } else if (item.kind === "tool" && item.toolName === "plan_update" && progress?.plan) placeLedger();
    placeAsks(item.id);
  }
  const memoryReads = exactMemoryReads(message.memory_refs).map(({ ref }) => ref);
  // Results a citation reveals in the work trace need a ledger even when no step names them.
  const workResults = evidenceRefs.some((result) => isInlineWorkEvidence(result, message.memory_refs));
  if (progress?.plan || memoryReads.length || workResults) {
    // A plan or Memory reads with no step of their own go before the live answer, where they stay once it settles.
    const answerAt = segments.findIndex((segment) => segment.kind === "prose" && segment.tone === "answer");
    const placedLedger = placeLedger(answerAt < 0 ? segments.length : answerAt);
    placedLedger.progress.plan = progress?.plan;
    placedLedger.memoryReads = memoryReads;
  }
  // A question or approval no step names is appended where it was asked: the end of the turn so far.
  for (const question of questions) if (!placed.has(question.requestId)) segments.push({ kind: "question", key: `question:${question.requestId}`, question });
  for (const approval of approvals) if (!placed.has(approval.requestId)) segments.push({ kind: "approval", key: `approval:${approval.requestId}`, approval });
  if (ledger) {
    const linked = new Set(ledger.progress.items.flatMap((item) => item.kind === "tool" ? item.resultIds ?? [] : []));
    ledger.sources = citationSources(evidenceRefs.filter((source) => linked.has(source.id)));
    ledger.evidence = evidenceRefs.filter((result) => isInlineWorkEvidence(result, message.memory_refs));
    const durationMs = progress?.finishedAtMs != null ? progress.finishedAtMs - startedAtMs : message.performance?.totalMs;
    ledger.ledger = ledgerView(ledger, state, running, durationMs, evidenceRefs);
  }
  if (!liveDraft && content && !isFailurePlaceholder(message)) {
    pushProse({ kind: "prose", key: answerKey ?? "answer", text: content, tone: "answer", streaming, checks: message.number_checks });
  }

  const announce = footer.kind === "live"
    ? [footer.line.label, footer.line.target].filter(Boolean).join(" · ")
    : ledger ? [ledger.ledger.text, ledger.ledger.failed].filter(Boolean).join(" · ") : "";
  return { state, segments, footer, announce };
}
