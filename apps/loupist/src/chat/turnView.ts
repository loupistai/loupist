import i18n from "@/i18n";
import { formatNumber } from "@/i18n/format";
import { useState } from "react";
import type {
  ChatEvidenceRef,
  ChatFailureKind,
  ChatMessage,
  ChatPlan,
  ChatProgress,
  ChatProgressItem,
  MemoryRef,
  NumberCheck,
} from "../types";
import { formatAsOf } from "./asOf";
import { citationSources, evidenceTitle } from "./evidence";
import { exactMemoryReads, isInlineWorkEvidence } from "./memoryActivity";
import { readNoun } from "./progressLabels";
import { actionLabel, isActionItem, scrubIdentifiers, withoutIdentifiers } from "./runStatus";
import { displayHost } from "../lib/displayHost";

const t = i18n.getFixedT(null, "chat");

/** A run with no new progress for this long reads as "Still working" instead of a frozen label. */
const HEARTBEAT_AFTER_MS = 15_000;

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
};

/** Exactly one state applies; every label derives from it. */
export type TurnState =
  | { kind: "starting" }
  /** `step` is the running step's own words, or the phase while no step runs. */
  | { kind: "working"; step: string; stepRunning: boolean; stalled: boolean }
  /** An approval or a question waits for the user. */
  | { kind: "awaiting_approval" }
  | { kind: "writing" }
  | { kind: "stopping" }
  | { kind: "complete" }
  | { kind: "stopped"; kept: "steps_and_text" | "steps_and_draft" | "steps" | "text" | "draft" | "nothing" }
  /** A busy answer counting down to its own retry (`retryPending`) has not failed yet. */
  | { kind: "failed"; failure: ChatFailureKind; retryPending: boolean };

/** Narration is a preamble: the answer's metrics in the secondary color. */
export type ProseTone = "answer" | "narration";

/** What colors the row: the running step, a settled summary, or a run or step that went wrong. */
export type ActivityTone = "live" | "settled" | "problem";

/** One line with a fixed height; the step list lives inside its details. */
type ActivityRowView =
  | { kind: "live"; label: string; stalled: boolean }
  | { kind: "summary"; label: string; issues: string | null; durationMs?: number };

export type TurnSegment =
  | { kind: "prose"; key: string; text: string; tone: ProseTone; streaming: boolean; checks?: NumberCheck[] }
  | {
      kind: "activity";
      key: string;
      row: ActivityRowView;
      tone: ActivityTone;
      progress: ChatProgress;
      /** Calculations and queries whose result opens inline under the step. */
      evidence: ChatEvidenceRef[];
      /** Distinct citation subjects this group of steps read. */
      sources: ChatEvidenceRef[];
      memoryReads: MemoryRef[];
      /** A failed or stopped step opens the details; the reader can still close them. */
      autoExpand: boolean;
    };

/** How a settled turn ended, when not well. A busy answer carries the seconds until it retries itself. */
export type RunState =
  | { kind: "stopped"; label: string }
  | { kind: "failed"; failure: ChatFailureKind; retryInSeconds?: number };

export type TurnFooter =
  | { kind: "status"; label: string; sinceMs: number; stalled: boolean }
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

type Group = Extract<TurnSegment, { kind: "activity" }>;

/** What a stopped reply kept (S4): finished steps and any text so far. */
export function visibleMessageContent(message: ChatMessage): string {
  if (message.role !== "assistant" || message.status !== "aborted") return message.content;
  const content = message.content.trimEnd();
  if (content === "Response stopped.") return "";
  return content.replace(/\r?\n(?:\r?\n)*Response stopped\.$/u, "").trimEnd();
}

/** The host's terminal placeholder, explained by the run state instead of shown as prose. */
const isFailurePlaceholder = (message: ChatMessage) =>
  message.role === "assistant" && message.status === "error" && message.content.trim() === "Response could not be completed.";

const startedAt = (message: ChatMessage) => message.progress?.startedAtMs ?? Date.parse(message.created_at);

const phaseLabel = (phase: ChatMessage["phase"]) => {
  if (phase === "writing") return t("turn.writing");
  if (phase === "thinking") return t("turn.thinkingItThrough");
  if (phase === "tool") return t("turn.waitingForNextStep");
  return t("turn.waitingForModel");
};

const lastRunning = (items: readonly ChatProgressItem[]) =>
  [...items].reverse().find((item): item is ActionItem => isActionItem(item) && item.status === "running");

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

/** Use host categories for the subject. Never infer what a shell command did from its text. */
function researchSubject(actions: ActionItem[], results: ChatEvidenceRef[], memoryReads: MemoryRef[]) {
  const labels = new Set<string>();
  for (const item of actions) {
    if (item.kind !== "tool") continue;
    if (item.operation === "calculate") labels.add(t("turn.research.calculations"));
    else if (item.category === "finance") {
      const leaf = item.toolName?.split("__").at(-1);
      if (leaf && i18n.exists(`chat:asOf.reads.${leaf}`)) {
        const noun = readNoun(leaf);
        labels.add(noun.charAt(0).toLocaleUpperCase(i18n.language) + noun.slice(1));
      } else labels.add(t("turn.research.finance"));
    } else if (item.category === "web") labels.add(t("turn.research.sources"));
    else if (item.category === "memory") labels.add(t("turn.research.memory"));
    else if (item.category === "artifact") labels.add(t("turn.research.outputs"));
    if (labels.size === 2) break;
  }
  if (!labels.size && memoryReads.length) labels.add(t("turn.research.memory"));
  if (!labels.size && results.some((result) => result.tool_name === "calculate")) labels.add(t("turn.research.calculations"));
  return [...labels].join(" · ") || t("turn.research.activity");
}

/** Keep a common verified domain beside the count. Mixed or unknown origins have no shared label. */
function sourcePreview(sources: ChatEvidenceRef[]) {
  const first = sources[0];
  if (!first) return "";
  const domains = new Set(sources.map((source) => displayHost(source.url)));
  const [domain] = domains;
  if (domains.size === 1 && domain) return domain;
  if (sources.length !== 1 || first.kind !== "original" || /[:/\\]/u.test(first.title)) return "";
  return scrubIdentifiers(evidenceTitle(first)).slice(0, 80);
}

function issueLabel(actions: ActionItem[], progress: ChatProgress, asOf: string | undefined) {
  // A step paused by the untrusted-text rule is not a failure; it is counted apart.
  const paused = actions.filter(pausedByRule).length;
  const failed = actions.filter((item) => item.status === "failed").length - paused;
  const stopped = actions.filter((item) => item.status === "stopped").length;
  const skipped = actions.filter((item) => item.status === "skipped").length;
  const approvals = progress.approvals ?? [];
  return [
    failed > 0 ? t("turn.research.failedAttempt", { count: failed }) : "",
    stopped > 0 ? t("turn.stoppedCount", { stoppedCount: stopped }) : "",
    paused > 0 ? t("turn.pausedCount", { pausedCount: paused }) : "",
    skipped > 0 ? asOf
      ? t("turn.research.unavailableAsOf", { count: skipped, date: formatAsOf(asOf) })
      : t("turn.research.unavailable", { count: skipped }) : "",
    approvals.some((item) => item.state === "declined") ? t("turn.research.declined") : "",
    approvals.some((item) => item.state === "expired") ? t("turn.research.expired") : "",
  ].filter(Boolean).join(" · ") || null;
}

function summaryLabel(group: Group, running: boolean) {
  const { progress, sources, evidence, memoryReads } = group;
  const actions = progress.items.filter(isActionItem);
  const sections = planSections(progress.plan, progress.items);
  // A multi-stage plan stays inside the details. Its current stage is useful
  // during the run; afterwards a single attributed stage can still name its
  // own group, but the last stage cannot describe a whole turn.
  const stage = running
    ? progress.plan?.items.find((item) => item.status === "active")?.label
    : sections.nested.size === 1 && !sections.rest.length
      ? progress.plan?.items.find((item) => sections.nested.has(item.id))?.label
      : undefined;
  const planSubject = stage && scrubIdentifiers(stage);
  const sourceCount = sources.length ? t("turn.readSources", { count: sources.length, formattedCount: formatNumber(sources.length) }) : "";
  const subject = (running ? planSubject || sourceCount : sourceCount || planSubject) || researchSubject(actions, evidence, memoryReads);
  return [subject, subject !== sourceCount && sourceCount, sourcePreview(sources)].filter(Boolean).join(" · ");
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

function deriveState({ message, now, lastActivityAt, stopping, retry }: TurnInput, content: string): TurnState {
  const progress = message.progress;
  const items = progress?.items ?? [];
  if (message.status === "error") {
    const retryPending = message.failure_kind === "rate_limited" && retry.offered && retry.attempt <= BUSY_RETRY_SECONDS.length;
    return { kind: "failed", failure: message.failure_kind ?? "other", retryPending };
  }
  if (message.status === "aborted") return { kind: "stopped", kept: keptByStop(message, content) };
  if (message.status !== "streaming") return { kind: "complete" };
  if (stopping) return { kind: "stopping" };
  if ((progress?.approvals ?? []).some((approval) => approval.state === "waiting")
    || (progress?.questions ?? []).some((question) => question.state === "waiting")) return { kind: "awaiting_approval" };
  const running = lastRunning(items);
  if (running || message.phase === "tool" || message.phase === "thinking" || (message.phase !== "writing" && items.length)) {
    return { kind: "working", step: running ? actionLabel(running) : phaseLabel(message.phase), stepRunning: Boolean(running), stalled: now - lastActivityAt >= HEARTBEAT_AFTER_MS };
  }
  if (message.phase === "writing") return { kind: "writing" };
  return { kind: "starting" };
}

const footerLabel = (state: TurnState): string => {
  switch (state.kind) {
    case "starting": return t("turn.waitingForModel");
    case "working": return state.stalled ? t("turn.stillWorking") : state.stepRunning ? t("turn.working") : state.step;
    case "awaiting_approval": return t("turn.waitingForYou");
    case "writing": return t("turn.writing");
    case "stopping": return t("chatView.stopping");
    default: return "";
  }
};

function runState(state: TurnState, { message, retry }: TurnInput): RunState | null {
  if (state.kind === "stopped") return { kind: "stopped", label: stoppedLabel(state.kept, message.progress?.items ?? []) };
  if (state.kind !== "failed") return null;
  const retryInSeconds = state.retryPending ? message.retry_after_seconds ?? BUSY_RETRY_SECONDS[retry.attempt - 1] ?? BUSY_RETRY_SECONDS[0] : undefined;
  return { kind: "failed", failure: state.failure, retryInSeconds };
}

function status(input: TurnInput) {
  const { message } = input;
  const content = visibleMessageContent(message);
  const state = deriveState(input, content);
  const stalled = state.kind === "working" && state.stalled;
  const footer: TurnFooter = state.kind === "complete" || state.kind === "stopped" || state.kind === "failed"
    ? {
        kind: "settled",
        sources: citationSources(message.evidence_refs ?? []),
        copyText: content && !isFailurePlaceholder(message) ? content : null,
        feedback: state.kind === "complete",
        runState: runState(state, input),
      }
    : { kind: "status", label: footerLabel(state), sinceMs: startedAt(message), stalled };
  return { state, footer, content, stalled };
}

/** The state and footer alone, for a reader that redraws every second (the status pill). */
export function deriveTurnStatus(input: TurnInput): TurnStatus {
  const { state, footer } = status(input);
  return { state, footer };
}

/**
 * Everything an assistant turn shows, in reading order: narration and answer
 * prose keyed by the commentary that wrote them, one activity row per run of
 * steps, and a footer slot that keeps its height from the first frame to the
 * settled answer.
 */
export function deriveTurnView(input: TurnInput): TurnView {
  const { message } = input;
  const { state, footer, content, stalled } = status(input);
  const progress = message.progress;
  const startedAtMs = startedAt(message);
  const items = progress?.items ?? [];
  const streaming = message.status === "streaming";
  const evidenceRefs = message.evidence_refs ?? [];
  const lastItem = items.at(-1);
  // The draft is the answer while the host writes it, under the key the answer
  // keeps. Once its settled text has grown the answer, the live copy, which may
  // still be listed for one update, is not drawn again. A draft that closed
  // unsettled (a tool call followed, or the final text came another way) and
  // any older commentary stay narration, wherever a removal leaves them.
  const { draft, answer_key: answerKey } = message;
  const settledCopy = (item: ChatProgressItem) => item.id === draft?.id && draft.settled && content.length > draft.answerLength;
  const liveDraft = streaming && message.phase === "writing" && lastItem?.kind === "commentary" && lastItem.id === draft?.id && !settledCopy(lastItem)
    ? lastItem : undefined;

  const segments: TurnSegment[] = [];
  const groups: Group[] = [];
  const owners = new Map<string, Group>();
  const resultOwners = new Map<string, Group>();
  // A row created after the steps were read (a plan or Memory reads with no
  // step of their own) goes before the live answer, where it stays once the
  // answer settles.
  const group = (key: string): Group => {
    const next: Group = {
      kind: "activity",
      key,
      row: { kind: "summary", label: "", issues: null },
      tone: "settled",
      progress: { startedAtMs, finishedAtMs: progress?.finishedAtMs, items: [] },
      evidence: [], sources: [], memoryReads: [], autoExpand: false,
    };
    groups.push(next);
    const answerAt = segments.findIndex((segment) => segment.kind === "prose" && segment.tone === "answer");
    segments.splice(answerAt < 0 ? segments.length : answerAt, 0, next);
    return next;
  };
  let open: Group | undefined;
  for (const item of items) {
    if (item.kind === "commentary") {
      open = undefined;
      const text = withoutIdentifiers(item.text);
      if (item === liveDraft) {
        segments.push({ kind: "prose", key: answerKey ?? item.id, text: content ? `${content}\n\n${text}` : text, tone: "answer", streaming: true });
      } else if (!settledCopy(item) && text.trim()) {
        segments.push({ kind: "prose", key: item.id, text, tone: "narration", streaming: false });
      }
      continue;
    }
    if (!open) {
      open = group(groups.length ? `step-${item.id}` : "activity");
    }
    open.progress.items.push(item);
    owners.set(item.id, open);
    if (item.kind === "tool") for (const id of item.resultIds ?? []) resultOwners.set(id, open);
  }
  const first = () => groups[0] ?? group("activity");
  const memoryReads = exactMemoryReads(message.memory_refs).map(({ ref }) => ref);
  if (progress?.plan || memoryReads.length) {
    const head = first();
    head.progress.plan = progress?.plan;
    head.memoryReads = memoryReads;
  }
  for (const result of evidenceRefs) {
    if (isInlineWorkEvidence(result, message.memory_refs)) (resultOwners.get(result.id) ?? first()).evidence.push(result);
  }
  for (const approval of progress?.approvals ?? []) {
    const owner = (approval.itemId && owners.get(approval.itemId)) || first();
    (owner.progress.approvals ??= []).push(approval);
  }
  for (const question of progress?.questions ?? []) {
    const owner = (question.itemId && owners.get(question.itemId)) || first();
    (owner.progress.questions ??= []).push(question);
  }
  for (const source of evidenceRefs) resultOwners.get(source.id)?.sources.push(source);
  for (const entry of groups) {
    entry.sources = citationSources(entry.sources);
    const actions = entry.progress.items.filter(isActionItem);
    const running = streaming ? lastRunning(actions) : undefined;
    entry.row = running
      ? { kind: "live", label: actionLabel(running), stalled }
      : {
          kind: "summary",
          label: summaryLabel(entry, streaming),
          issues: issueLabel(actions, entry.progress, message.as_of),
          durationMs: !streaming && progress?.finishedAtMs !== undefined ? progress.finishedAtMs - startedAtMs : undefined,
        };
    const failed = actions.some((item) => item.status === "failed" && !pausedByRule(item));
    entry.autoExpand = failed || actions.some((item) => item.status === "stopped");
    entry.tone = state.kind === "failed" || failed ? "problem" : running ? "live" : "settled";
  }
  if (!liveDraft && content && !isFailurePlaceholder(message)) {
    segments.push({ kind: "prose", key: answerKey ?? "answer", text: content, tone: "answer", streaming, checks: message.number_checks });
  }

  const lastRow = [...groups].reverse().find((entry) => entry.row.kind === "summary")?.row;
  const announce = footer.kind === "status"
    ? state.kind === "working" && state.stepRunning ? `${footer.label} · ${state.step}` : footer.label
    : lastRow?.kind === "summary" ? [lastRow.label, lastRow.issues].filter(Boolean).join(" · ") : "";
  return { state, segments, footer, announce };
}
