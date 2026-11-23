import i18n from "@/i18n";
import type { WorldReadinessStatus } from "../knowledge/worldReadiness";
import { knowledgePinKey } from "../knowledge/location";
import type { ChatEvidenceRef, ChatMessage, ChatProgressItem, ChatToolRefusal, KnowledgePin, NumberCheck } from "../types";
import type { ChatApproval, ChatProgressCategory, ChatProgressOperation, ChatProgressStatus, ChatQuestion } from "../contracts/chat";
import type { GrowthEvent, KnowledgeMap } from "../contracts/knowledge";
import { addedMapItems } from "./chatMapData";
import { entityLabel } from "../knowledge/mapItems";
import { nodeColorKey } from "../knowledge/mapKinds";
import { chatReceipt } from "./receipt";
import { deliveredFiles } from "./deliveredFiles";
import { answerSources, type AnswerSource } from "./answerSources";
import { knownActivityLabel, readNounFor } from "./progressLabels";
import { connectionText } from "@/lib/connectionCopy";
import { actionLabel, visibleTarget } from "./runStatus";

/** `swatch` is the node's color on the Map, so a chip and its mark match. */
export type NameChip = { id: string; label: string; swatch: string };

/** Step keys are built from the host's tool and operation names at runtime, so they are not typed literals. */
const tDynamic = i18n.getFixedT(null, "chat") as unknown as (key: string, options?: Record<string, unknown>) => string;

const MAX_LEARNED_CHIPS = 5;

export type RailFile = {
  key: string;
  kind: "image" | "file";
  filename: string;
  sizeBytes?: number;
  /** A user attachment, read through the chat's attachment reader. */
  messageId?: string;
  attachmentId?: string;
  url?: string;
  /** A workspace file the Loupist linked in an answer. */
  path?: string;
  fromLoupist: boolean;
};

export type RailUsing = {
  key: string;
  kind: "pin" | "principle" | "method" | "connection";
  label: string;
  /** The pin's kind label, shown as meta. */
  detail?: string;
  pin?: KnowledgePin;
  recordId?: string;
};

export type RailLearned = {
  names: number;
  relationships: number;
  methods: number;
  chips: NameChip[];
  highlightIds: ReadonlySet<string>;
};

export type RailData = {
  files: RailFile[];
  using: RailUsing[];
  learned: RailLearned | null;
  learningPending: boolean;
  learningWaiting?: WorldReadinessStatus | "unknown";
  /** Connections the latest answer could not use without the user's OK. */
  needs: ChatToolRefusal[];
  /** Approval cards still waiting in the latest answer. */
  waiting: ChatApproval[];
  /** Question cards still waiting in the latest answer. */
  questions: ChatQuestion[];
  hasCards: boolean;
};

const fileName = (path: string) => path.split(/[\\/]/u).filter(Boolean).at(-1) ?? path;

/**
 * What the activity sheet shows about the whole chat: files and images, what the
 * Loupist is using, what it learned here, and what waits for the user, all from
 * data the chat already holds. Nothing here reads or writes beyond the chat's messages,
 * its growth events and the composer's pins.
 */
export function railData({ messages, customConnections = false, pins = [], map, events = [], learningPending = false, learningWaiting, chatId, language, workspacePath }: {
  messages: readonly ChatMessage[];
  customConnections?: boolean;
  pins?: readonly KnowledgePin[];
  map?: KnowledgeMap | null;
  events?: readonly GrowthEvent[];
  learningPending?: boolean;
  learningWaiting?: WorldReadinessStatus | "unknown";
  chatId: string;
  language: string;
  workspacePath?: string;
}): RailData {
  const files: RailFile[] = [];
  const seenPaths = new Set<string>();
  const using: RailUsing[] = pins.map((pin) => ({ key: `pin:${knowledgePinKey(pin)}`, kind: "pin" as const, label: pin.label, pin }));
  const seenRecords = new Set<string>();
  const connections = new Set<string>();
  for (const message of messages) {
    for (const attachment of message.attachments ?? []) {
      files.push({ key: `attachment:${message.id}:${attachment.id}`, kind: attachment.media_type.startsWith("image/") ? "image" : "file", filename: attachment.filename,
        sizeBytes: attachment.size_bytes, messageId: message.id, attachmentId: attachment.id, url: attachment.url, fromLoupist: false });
    }
    if (message.role !== "assistant") continue;
    if (message.content) {
      for (const path of deliveredFiles(message.content, workspacePath)) {
        if (seenPaths.has(path)) continue;
        seenPaths.add(path);
        files.push({ key: `delivered:${path}`, kind: /\.(png|jpe?g|gif|webp|svg)$/iu.test(path) ? "image" : "file", filename: fileName(path), path, fromLoupist: true });
      }
    }
    const receipt = chatReceipt(message);
    for (const principle of receipt.usedPrinciples) {
      if (seenRecords.has(principle.recordId)) continue;
      seenRecords.add(principle.recordId);
      using.push({ key: `principle:${principle.recordId}`, kind: "principle", label: principle.title, recordId: principle.recordId });
    }
    for (const method of receipt.usedMethods) {
      const recordId = method.recordId;
      if (!recordId || seenRecords.has(recordId)) continue;
      seenRecords.add(recordId);
      using.push({ key: `method:${recordId}`, kind: "method", label: method.title, recordId });
    }
    for (const item of message.progress?.items ?? []) {
      if (item.kind === "commentary" || !item.source) continue;
      if (item.source.kind !== "connector" && item.category !== "capability") continue;
      const name = item.source.name.trim();
      if (!name || connections.has(name)) continue;
      connections.add(name);
      using.push({ key: `connection:${name}`, kind: "connection", label: connectionText(name) });
    }
  }

  let learned: RailLearned | null = null;
  const own = events.filter((event) => event.originChatId === chatId);
  if (map && own.length) {
    const added = addedMapItems(map, own);
    const methods = new Set(own.filter((event) => event.kind === "lens_created").map((event) => event.recordId ?? event.eventId)).size;
    const chips: RailLearned["chips"] = [];
    const seen = new Set<string>();
    for (const event of [...own].sort((a, b) => b.atMs - a.atMs)) {
      if (chips.length >= MAX_LEARNED_CHIPS) break;
      if (event.kind !== "entity_added" || !event.subjectId || seen.has(event.subjectId)) continue;
      const node = map.nodes.find((candidate) => candidate.kind === "entity" && candidate.id === event.subjectId);
      if (!node) continue;
      seen.add(node.id);
      chips.push({ id: node.id, label: entityLabel(node, language), swatch: nodeColorKey(node) });
    }
    if (added.nodeIds.size || added.edgeIds.size || methods) {
      learned = { names: added.nodeIds.size, relationships: added.edgeIds.size, methods, chips, highlightIds: added.highlightIds };
    }
  }

  const latest = [...messages].reverse().find((message) => message.role === "assistant");
  const needs = (latest?.tool_refusals ?? []).filter((refusal) => customConnections && refusal.reason === "needs_approval");
  const waiting = (latest?.progress?.approvals ?? []).filter((approval) => customConnections && approval.state === "waiting");
  const questions = (latest?.progress?.questions ?? []).filter((question) => question.state === "waiting");
  return { files, using, learned, learningPending, learningWaiting, needs, waiting, questions, hasCards: files.length > 0 || using.length > 0 || learned !== null || learningPending || !!learningWaiting || needs.length > 0 || waiting.length > 0 || questions.length > 0 };
}

type ActionItem = Extract<ChatProgressItem, { kind: "tool" | "system" }>;

/** One step of a reply as the activity sheet lists it. */
export type ActivityStep = {
  id: string;
  /** What the step did, in the interface language. Never the tool's own name or the host's English. */
  label: string;
  /** What it read or worked on ("NVDA", a page, a file), when the host named it. */
  target?: string;
  href?: string;
  status: ChatProgressStatus;
  category: ChatProgressCategory;
  operation: ChatProgressOperation;
  /** Present only when the host stamped both ends of the step and they differ. */
  durationMs?: number;
  /** Why a step failed, stopped or was skipped, in plain words. */
  reason?: string;
  detailId?: string;
  outputRevision?: number;
  evidence: ChatEvidenceRef[];
};

/** Steps of the same kind, collapsed into one row; a step with a reason keeps a row of its own. */
export type ActivityGroup = {
  key: string;
  label: string;
  status: ChatProgressStatus;
  steps: ActivityStep[];
  /** The sum, only when every step in the group has a duration. */
  durationMs?: number;
};

export type ActivityTurn = {
  messageId: string;
  atMs: number;
  running: boolean;
  durationMs?: number;
  stepCount: number;
  groups: ActivityGroup[];
  sources: AnswerSource[];
  checks: { check: NumberCheck; evidence?: ChatEvidenceRef }[];
};

/** The plan and the question line stand for these steps in the answer; they are not steps of their own. */
const HIDDEN_TOOLS = new Set(["plan_update", "ask_user"]);
const toolBase = (tool: string | undefined) => tool?.split("__").at(-1);
const isIdentifier = (value: string) => /^[a-z][a-z0-9]*(?:_[a-z0-9]+)+$/u.test(value);

/**
 * A step's title in the interface language: the sheet's own label for a tool the
 * catalog lacks, the catalog's label, a read's noun, the connection's name, or a
 * generic label for its kind. The tool's raw name and the host's English never show.
 */
export function activityStepLabel(item: ActionItem, form: "running" | "done" = item.status === "running" ? "running" : "done"): string {
  if (item.kind === "system") return item.operation === "compact" ? actionLabel(item) : tDynamic(`activitySheet.step.operation.${item.operation}`);
  const known = knownActivityLabel(item.toolName, form);
  if (known) return known;
  const connection = item.source?.kind === "connector" ? connectionText(item.source.name.trim()) : "";
  if (connection) return tDynamic(form === "running" ? "activitySheet.step.runningConnection" : "activitySheet.step.usedConnection", { name: connection });
  return tDynamic(`activitySheet.step.operation.${item.operation}`);
}

/** The step's target without internal identifiers; tool names in it read as their nouns or drop out. */
function activityTarget(item: ActionItem) {
  // A connection the host names in the target ("Web Research · …") reads in the interface language.
  const target = visibleTarget(item.target)?.split(" · ").map(connectionText).join(" · ");
  if (!target) return undefined;
  const parts = target.split(/\s*,\s*/u);
  if (!parts.some(isIdentifier)) return target;
  const named = parts.flatMap((part) => !isIdentifier(part) ? [part] : readNounFor(part) ?? []);
  return named.length ? named.join(", ") : undefined;
}

const sameTool = (a: string, b: string | undefined) => !!b && (a === b || b.endsWith(`__${a}`) || a.endsWith(`__${b}`));

const REFUSAL_REASONS = { untrusted_text_paused: "paused", protected_path: "protectedPath", outside_workspace: "outsideWorkspace" } as const;

const PUBLISH_TOOLS = new Set(["grid_publish", "chart_publish", "artifact_publish"]);

/**
 * A publish or revise step that failed for an artifact the reply already shows
 * (a staged draft while it streams, or a stored reference): the card keeps the
 * earlier version. The target is the title, as the card reads it. A later
 * publish of that title that succeeded is what the card shows, so the failure
 * then left nothing on screen of its own.
 */
function revisionKeptEarlier(item: ActionItem, message: ChatMessage) {
  if (item.kind !== "tool" || !PUBLISH_TOOLS.has(toolBase(item.toolName) ?? "")) return false;
  const title = item.target?.trim();
  if (!title) return false;
  const steps = message.progress?.items ?? [];
  const later = steps.slice(steps.findIndex((candidate) => candidate.id === item.id) + 1);
  if (later.some((candidate) => candidate.kind === "tool" && candidate.status === "succeeded"
    && PUBLISH_TOOLS.has(toolBase(candidate.toolName) ?? "") && candidate.target?.trim() === title)) return false;
  const shown = [
    ...(message.artifact_refs ?? []).map((artifact) => artifact.title),
    ...(message.status === "streaming" ? (message.staged_artifacts ?? []).map((view) => view.artifact.title) : []),
  ];
  return shown.some((shownTitle) => shownTitle.trim() === title);
}

function stepReason(item: ActionItem, message: ChatMessage): string | undefined {
  if (item.status === "stopped") return tDynamic("activitySheet.reason.stopped");
  if (item.status === "skipped") return message.as_of ? tDynamic("activitySheet.reason.skipped") : undefined;
  if (item.status !== "failed") return undefined;
  if (item.kind === "tool" && item.refusal) {
    if (item.refusal === "sec_contact_required") {
      return `${i18n.t("connections:catalog.sec_edgar_contact_email")}. ${i18n.t("connections:readiness.setupNote")}.`;
    }
    return tDynamic(`activitySheet.reason.${REFUSAL_REASONS[item.refusal]}`);
  }
  const refusal = item.kind === "tool" ? message.tool_refusals?.find((candidate) => sameTool(candidate.tool_name, item.toolName)) : undefined;
  if (refusal) return tDynamic(`activitySheet.reason.refusal.${refusal.reason}`);
  if (revisionKeptEarlier(item, message)) return tDynamic("activitySheet.reason.revisionNotApplied");
  const kind = failureKindReason(item);
  if (kind) return kind;
  if (message.offline_reads && (item.category === "finance" || item.category === "web" || item.source?.kind === "connector")) return tDynamic("activitySheet.reason.offline");
  return tDynamic("activitySheet.reason.generic");
}

/** The line a failed step's kind has of its own; undefined for a provider error or an unrecorded kind. */
function failureKindReason(item: ActionItem): string | undefined {
  if (item.failureReason === "unreachable") return tDynamic("activitySheet.reason.offline");
  if (item.failureReason === "timeout") return tDynamic("chatMessageCard.provider_timeout");
  if (item.failureReason === "invalid_input") return tDynamic("activitySheet.reason.invalidInput");
  if (item.failureReason === "empty_result") return tDynamic("activitySheet.reason.emptyResult");
  return undefined;
}

/**
 * Why a failed market-data read was not used, for its row in the answer's steps:
 * the line its failure kind has, else the generic failure line. Undefined for
 * any other step.
 */
export function failedReadReason(item: ActionItem): string | undefined {
  if (item.kind !== "tool" || item.status !== "failed" || item.category !== "finance" || item.refusal) return undefined;
  return failureKindReason(item) ?? tDynamic("activitySheet.reason.generic");
}

const stepDuration = (item: ActionItem) =>
  item.finishedAtMs != null && item.finishedAtMs >= item.startedAtMs ? item.finishedAtMs - item.startedAtMs : undefined;

/**
 * One reply's activity: every step in order, steps of the same kind collapsed
 * into one group at the place the first one ran, every failure on its own row
 * with its reason, the sources it cited and the figures Rust checked. Durations
 * and reasons show only where the host recorded them.
 */
export function activityTurn(message: ChatMessage): ActivityTurn {
  const evidenceById = new Map((message.evidence_refs ?? []).map((item) => [item.id, item]));
  const groups: ActivityGroup[] = [];
  const byKind = new Map<string, ActivityGroup>();
  let stepCount = 0;
  for (const item of message.progress?.items ?? []) {
    if (item.kind === "commentary" || (item.kind === "tool" && item.toolName && HIDDEN_TOOLS.has(item.toolName))) continue;
    stepCount += 1;
    const step: ActivityStep = {
      id: item.id, label: activityStepLabel(item), target: activityTarget(item), href: item.href ?? undefined,
      status: item.status, category: item.category, operation: item.operation, durationMs: stepDuration(item), reason: stepReason(item, message),
      detailId: item.kind === "tool" ? item.detailId : undefined, outputRevision: item.kind === "tool" ? item.outputRevision : undefined,
      evidence: item.kind === "tool" ? (item.resultIds ?? []).flatMap((id) => evidenceById.get(id) ?? []) : [],
    };
    if (step.reason) {
      groups.push({ key: item.id, label: step.label, status: step.status, steps: [step], durationMs: step.durationMs });
      continue;
    }
    const kind = activityStepLabel(item, "done");
    const group = byKind.get(kind);
    if (!group) {
      const created: ActivityGroup = { key: `kind:${item.id}`, label: step.label, status: step.status, steps: [step], durationMs: step.durationMs };
      byKind.set(kind, created);
      groups.push(created);
      continue;
    }
    group.steps.push(step);
    const running = group.steps.find((member) => member.status === "running");
    group.status = running ? "running" : "succeeded";
    group.label = running ? running.label : kind;
    group.durationMs = group.steps.every((member) => member.durationMs !== undefined)
      ? group.steps.reduce((sum, member) => sum + member.durationMs!, 0) : undefined;
  }
  const progress = message.progress;
  const durationMs = progress?.finishedAtMs != null ? progress.finishedAtMs - progress.startedAtMs : message.performance?.totalMs;
  return {
    messageId: message.id,
    atMs: progress?.startedAtMs ?? Date.parse(message.created_at),
    running: message.status === "streaming",
    durationMs: durationMs && durationMs > 0 ? durationMs : undefined,
    stepCount, groups,
    sources: answerSources(message),
    checks: (message.number_checks ?? []).map((check) => ({ check, evidence: evidenceById.get(check.evidence_id) })),
  };
}

/** Replies the sheet can show, oldest first: answers, never steers or the user's own messages. */
export const activityMessages = (messages: readonly ChatMessage[]) =>
  messages.filter((message) => message.role === "assistant" && !message.steer && !hostPost(message));

/** A message the host posted on its own (a result, an alert, a "now also tracks" line): no reply, no steps. */
const hostPost = (message: ChatMessage) => Boolean(message.delivery || message.alert || message.vocabulary || message.handoff || message.link)
  || (message.host_composed === true && !message.progress?.items.length);
