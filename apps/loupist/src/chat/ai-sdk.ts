import i18n from "@/i18n";
import type { ChatTransport, FileUIPart, UIMessage, UIMessageChunk } from "ai";
import type {
  AgentHarnessSnapshot,
  ChatAttachment,
  ChatEvidenceRef,
  ChatArtifactRef,
  ChatMessage,
  ChatKnowledgePin,
  ChatPerformance,
  ChatProgress,
  ChatProgressItem,
  ChatProgressPatch,
  ChatStreamEvent,
  ChatThread,
  HostPostRecords,
  LoupistBridge,
  MemoryRef,
  MemoryUpdateResult,
  ScheduledRun,
} from "../types";
import { extractMentionedConnectionIds } from "./composerMentions";
import { commandErrorCode, errorMessage } from "../lib/errors";
import type { ChatAdmission } from "./submission";

const t = i18n.getFixedT(null, "chat");

// Typed from the contract, so tsc fails when a host-post record is missing here.
const HOST_POST_KEYS: Record<keyof HostPostRecords, true> = {
  delivery: true, alert: true, watch: true, earnings: true, vocabulary: true, handoff: true, link: true,
};
const HOST_POST_FIELDS = Object.keys(HOST_POST_KEYS) as (keyof HostPostRecords)[];

// Every stream event the renderer handles, typed from the contract so tsc fails when
// a Rust variant is missing here. An event of any other type is ignored, not fatal.
const STREAM_EVENT_TYPES: Record<ChatStreamEvent["type"], true> = {
  started: true, memory: true, schedule: true, evidence: true, delta: true, generation_finished: true, title: true,
  progress: true, progress_patch: true, phase: true, memory_update: true, artifact: true, completed: true,
  aborted: true, error: true,
};

/** Whether this build knows the event; a newer Rust may send types it does not. */
const isKnownStreamEvent = (event: { type?: unknown }): event is ChatStreamEvent =>
  typeof event.type === "string" && Object.hasOwn(STREAM_EVENT_TYPES, event.type);

const ignoreUnknownStreamEvent = (event: { type?: unknown }) => {
  if (import.meta.env.DEV) console.warn(`Ignoring chat stream event of unknown type ${JSON.stringify(event.type)}`);
};

const hostPostRecords = (source: HostPostRecords | undefined): HostPostRecords =>
  Object.fromEntries(HOST_POST_FIELDS.filter((field) => source?.[field]).map((field) => [field, source![field]]));

export type LoupistChatMetadata = HostPostRecords & {
  tool_refusals?: ChatMessage["tool_refusals"];
  offline_reads?: boolean;
  replaced_model?: string;
  unsaved_theses?: ChatMessage["unsaved_theses"];
  as_of?: string;
  number_checks?: ChatMessage["number_checks"];
  sensitivity_cards?: ChatMessage["sensitivity_cards"];
  as_of_report?: ChatMessage["as_of_report"];
  feedback?: ChatMessage["feedback"];
  native_message_id?: string;
  created_at?: string;
  status?: ChatMessage["status"];
  memory_revision?: string;
  lens_receipts?: ChatMessage["lens_receipts"];
  observed_exact_count?: number;
  refs_truncated?: boolean;
  refs_digest?: string;
  execution_model?: import("../types").ExecutionModelLock;
  agent_harness?: AgentHarnessSnapshot;
  failure_kind?: ChatMessage["failure_kind"];
  retry_after_seconds?: number;
  steer?: boolean;
  host_composed?: boolean;
  final_text?: string;
  /** `final_text` is the host's failure placeholder (`ChatMessage.placeholder`). */
  placeholder?: boolean;
  progress?: ChatProgress;
  phase?: ChatMessage["phase"];
  draft?: ChatMessage["draft"];
  answer_key?: string;
  performance?: ChatPerformance;
};

export type LoupistChatData = {
  progress: NonNullable<ChatMessage["progress"]>;
  attachment: ChatAttachment;
  memory: MemoryRef[];
  memory_update: MemoryUpdateResult;
  artifact: ChatArtifactRef;
  evidence: ChatEvidenceRef;
  schedule: import("../types").ScheduleReceipt;
  run: { run_id: string };
  performance: ChatPerformance;
};

export type LoupistUIMessage = UIMessage<LoupistChatMetadata, LoupistChatData>;

export type TauriChatRequestContext = {
  /** Local admission receipt, removed before crossing the native bridge. */
  onAdmission?: (receipt: ChatAdmission) => void;
  loupist_id: string;
  thread_id: string;
  privateChat: boolean;
  model_profile_id: string;
  thinking_level: string;
  run_options: Record<string, string>;
  pins?: ChatKnowledgePin[];
  selected_artifact?: Pick<ChatArtifactRef, "artifact_id" | "revision">;
  /** The composer's as-of date (`YYYY-MM-DD`); Rust enforces it. */
  as_of?: string;
};

export type ScheduledStreamConnection = {
  run: ScheduledRun;
  thread: ChatThread;
  connect: (observer: (event: ChatStreamEvent) => void) => void;
  release: () => void;
};

type ChatTitleHandler = (
  loupistId: string,
  threadId: string,
  title: string,
) => void;
type ChatArtifactHandler = (
  loupistId: string,
  threadId: string,
  artifact: ChatArtifactRef,
) => void;

const messageText = (message: LoupistUIMessage) =>
  message.parts
    .filter((part) => part.type === "text")
    .map((part) => part.text)
    .join("");

const UNKNOWN_MESSAGE_CREATED_AT = "1970-01-01T00:00:00.000Z";

export type ChatProgressStreamState = {
  progress: ChatProgress;
  sequence: number;
};

export const applyChatProgressSnapshot = (
  state: ChatProgressStreamState | undefined,
  progress: ChatProgress,
  sequence = 0,
): ChatProgressStreamState =>
  state && sequence <= state.sequence ? state : { progress, sequence };

export const applyChatProgressPatch = (
  state: ChatProgressStreamState | undefined,
  patch: ChatProgressPatch,
): ChatProgressStreamState | undefined => {
  if (!state || patch.sequence !== state.sequence + 1) return state;

  const removeIds = new Set(patch.removeItemIds ?? []);
  const upserts = patch.upsertItems ?? [];
  const upsertById = new Map(upserts.map((item) => [item.id, item]));
  const appends = new Map<string, string>();
  for (const append of patch.appendText ?? []) appends.set(append.id, (appends.get(append.id) ?? "") + append.text);
  const seen = new Set<string>();
  let itemsChanged = removeIds.size > 0;
  const items: ChatProgress["items"] = [];
  for (const item of state.progress.items) {
    if (removeIds.has(item.id)) continue;
    const replacement = upsertById.get(item.id);
    const appended = appends.get(item.id);
    if (replacement) {
      seen.add(item.id);
      if (replacement !== item) itemsChanged = true;
      items.push(replacement);
    } else if (appended !== undefined && item.kind === "commentary") {
      appends.delete(item.id);
      itemsChanged = true;
      items.push({ ...item, text: item.text + appended });
    } else {
      items.push(item);
    }
  }
  // An append for commentary this stream never saw is a gap: wait for the next snapshot.
  if (appends.size) return state;
  for (const item of upserts) {
    if (seen.has(item.id)) continue;
    items.push(item);
    itemsChanged = true;
  }
  return {
    sequence: patch.sequence,
    progress: {
      ...state.progress,
      ...(patch.finishedAtMs === undefined
        ? {}
        : { finishedAtMs: patch.finishedAtMs }),
      ...(patch.plan === undefined ? {} : { plan: patch.plan }),
      ...(patch.approvals === undefined ? {} : { approvals: patch.approvals }),
      ...(patch.questions === undefined ? {} : { questions: patch.questions }),
      items: itemsChanged ? items : state.progress.items,
    },
  };
};

const dataUrlPayload = (file: FileUIPart) => {
  const match = file.url.match(/^data:([^;,]+);base64,([A-Za-z0-9+/=]+)$/);
  if (!match) throw new Error(t("aiSdk.attachmentDataIsUnavailable"));
  return {
    filename: file.filename?.trim() || "attachment",
    media_type: file.mediaType || match[1],
    data_base64: match[2],
  };
};

/**
 * The store's copy of a thread, with the key each settled answer's Markdown node was
 * drawn under carried over from the live messages. The store never holds that key, so
 * without it a refresh remounts the answer and replays its reveal.
 */
export function fromStoreKeepingAnswers(current: readonly LoupistUIMessage[], stored: readonly ChatMessage[]): LoupistUIMessage[] {
  const answerKeys = new Map(current.flatMap((message) =>
    message.metadata?.answer_key ? [[message.metadata.native_message_id ?? message.id, message.metadata.answer_key]] : []));
  return stored.map((message) => {
    const ui = toLoupistUIMessage(message);
    const answerKey = answerKeys.get(message.id);
    return answerKey ? { ...ui, metadata: { ...ui.metadata, answer_key: answerKey } } : ui;
  });
}

const estimatedBase64Bytes = (url: string) => {
  const payload = url.split(",", 2)[1] ?? "";
  return Math.max(1, Math.floor((payload.replace(/=+$/u, "").length * 3) / 4));
};

export const toLoupistUIMessage = (message: ChatMessage): LoupistUIMessage => {
  return {
    id: message.id,
    role: message.role,
    metadata: {
      native_message_id: message.id,
      created_at: message.created_at,
      status: message.status ?? "complete",
      memory_revision: message.memory_revision,
      lens_receipts: message.lens_receipts,
      observed_exact_count: message.observed_exact_count,
      refs_truncated: message.refs_truncated,
      refs_digest: message.refs_digest,
      execution_model: message.execution_model,
      agent_harness: message.agent_harness,
      failure_kind: message.failure_kind,
      retry_after_seconds: message.retry_after_seconds,
      ...(message.placeholder ? { placeholder: true } : {}),
      steer: message.steer,
      host_composed: message.host_composed,
      ...hostPostRecords(message),
      tool_refusals: message.tool_refusals,
      offline_reads: message.offline_reads,
      replaced_model: message.replaced_model,
      unsaved_theses: message.unsaved_theses,
      as_of: message.as_of,
      number_checks: message.number_checks,
      sensitivity_cards: message.sensitivity_cards,
      as_of_report: message.as_of_report,
      performance: message.performance,
      feedback: message.feedback,
    },
    parts: [
      { type: "text", text: message.content, state: "done" },
      ...(message.attachments?.map((attachment) => ({
        type: "data-attachment" as const,
        id: attachment.id,
        data: attachment,
      })) ?? []),
      ...(message.memory_refs
        ? ([{ type: "data-memory", data: message.memory_refs }] as const)
        : []),
      ...(message.memory_update
        ? ([
            { type: "data-memory_update", data: message.memory_update },
          ] as const)
        : []),
      ...(message.schedule_receipts?.map((receipt) => ({
        type: "data-schedule" as const, id: receipt.task_id, data: receipt,
      })) ?? []),
      ...(message.evidence_refs?.map((evidence) => ({
        type: "data-evidence" as const,
        id: evidence.id,
        data: evidence,
      })) ?? []),
      ...(message.artifact_refs?.map((artifact) => ({
        type: "data-artifact" as const,
        id: `${artifact.artifact_id}:${artifact.revision}`,
        data: artifact,
      })) ?? []),
      ...(message.progress
        ? ([
            { type: "data-progress", id: "progress", data: message.progress },
          ] as const)
        : []),
    ],
  };
};

export const fromLoupistUIMessage = (message: LoupistUIMessage): ChatMessage => {
  const status = message.metadata?.status ?? "complete";
  const text: string[] = [];
  const artifactRefs: ChatArtifactRef[] = [];
  const attachmentParts: ChatAttachment[] = [];
  const fileAttachments: ChatAttachment[] = [];
  const scheduleReceipts: LoupistChatData["schedule"][] = [];
  const evidenceRefs: ChatEvidenceRef[] = [];
  let memoryRefs: MemoryRef[] | undefined;
  let memoryUpdate: MemoryUpdateResult | undefined;
  let progress = message.metadata?.progress;
  for (const part of message.parts) {
    switch (part.type) {
      case "text": text.push(part.text); break;
      case "data-memory": memoryRefs ??= part.data; break;
      case "data-memory_update": memoryUpdate ??= part.data; break;
      case "data-progress": progress ??= part.data; break;
      case "data-artifact": artifactRefs.push(part.data); break;
      case "data-attachment": attachmentParts.push(part.data); break;
      case "data-schedule": scheduleReceipts.push(part.data); break;
      case "data-evidence": evidenceRefs.push(part.data); break;
      case "file": {
        const index = fileAttachments.length + 1;
        fileAttachments.push({
          id: `${message.id}-attachment-${index}`,
          filename: part.filename ?? `attachment-${index}`,
          media_type: part.mediaType,
          size_bytes: estimatedBase64Bytes(part.url),
          url: part.url,
        });
        break;
      }
    }
  }

  return {
    id: message.metadata?.native_message_id ?? message.id,
    role: message.role === "assistant" ? "assistant" : "user",
    content:
      message.role === "assistant" && message.metadata?.final_text !== undefined
        ? message.metadata.final_text
        : text.join(""),
    ...(message.metadata?.placeholder ? { placeholder: true } : {}),
    created_at: message.metadata?.created_at ?? UNKNOWN_MESSAGE_CREATED_AT,
    status,
    memory_refs: memoryRefs,
    lens_receipts: message.metadata?.lens_receipts,
    schedule_receipts: scheduleReceipts,
    evidence_refs: evidenceRefs,
    memory_update: memoryUpdate,
    memory_revision: message.metadata?.memory_revision,
    observed_exact_count: message.metadata?.observed_exact_count,
    refs_truncated: message.metadata?.refs_truncated,
    refs_digest: message.metadata?.refs_digest,
    execution_model: message.metadata?.execution_model,
    agent_harness: message.metadata?.agent_harness,
    failure_kind: message.metadata?.failure_kind,
    retry_after_seconds: message.metadata?.retry_after_seconds,
    steer: message.metadata?.steer,
    host_composed: message.metadata?.host_composed,
    ...hostPostRecords(message.metadata),
    tool_refusals: message.metadata?.tool_refusals,
    offline_reads: message.metadata?.offline_reads,
    replaced_model: message.metadata?.replaced_model,
    ...(message.metadata?.unsaved_theses?.length ? { unsaved_theses: message.metadata.unsaved_theses } : {}),
    ...(message.metadata?.as_of ? { as_of: message.metadata.as_of } : {}),
    ...(message.metadata?.number_checks?.length ? { number_checks: message.metadata.number_checks } : {}),
    ...(message.metadata?.sensitivity_cards?.length ? { sensitivity_cards: message.metadata.sensitivity_cards } : {}),
    ...(message.metadata?.as_of_report ? { as_of_report: message.metadata.as_of_report } : {}),
    performance: message.metadata?.performance,
    feedback: message.metadata?.feedback,
    progress,
    phase: message.metadata?.phase,
    draft: message.metadata?.draft,
    answer_key: message.metadata?.answer_key,
    attachments:
      attachmentParts.length > 0
        ? attachmentParts
        : fileAttachments.length > 0
          ? fileAttachments
          : undefined,
    artifact_refs: artifactRefs.length ? artifactRefs : undefined,
  };
};

export class TauriChatTransport implements ChatTransport<LoupistUIMessage> {
  constructor(
    private readonly bridge: LoupistBridge,
    private readonly onTitle?: ChatTitleHandler,
    private readonly onArtifact?: ChatArtifactHandler,
  ) {}

  async sendMessages({
    messages,
    abortSignal,
    body,
  }: Parameters<ChatTransport<LoupistUIMessage>["sendMessages"]>[0]) {
    const context = body as TauriChatRequestContext | undefined;
    if (!context?.loupist_id || !context.thread_id) {
      throw new Error(t("aiSdk.loupistAndThreadAreRequiredForANativeChat"));
    }

    const lastUserMessage = [...messages]
      .reverse()
      .find((message) => message.role === "user");
    const prompt = lastUserMessage ? messageText(lastUserMessage).trim() : "";
    const attachments = lastUserMessage
      ? lastUserMessage.parts
          .filter((part) => part.type === "file")
          .map(dataUrlPayload)
      : [];
    if (!prompt && attachments.length === 0) {
      throw new Error(t("aiSdk.aMessageOrAttachmentIsRequired"));
    }

    const { onAdmission, privateChat, ...nativeContext } = context;
    const runId = `chat-ui-${crypto.randomUUID()}`;
    return this.nativeStream(context, abortSignal, runId, (onEvent) => this.bridge.chatSend({
      ...nativeContext,
      use_memory: !privateChat,
      update_memory: !privateChat,
      run_id: runId,
      prompt,
      mentioned_connection_ids: extractMentionedConnectionIds(prompt),
      attachments,
    }, onEvent), onAdmission);
  }

  private nativeStream(
    context: { loupist_id: string; thread_id: string },
    abortSignal: AbortSignal | undefined,
    runId: string,
    connect: (observer: (event: ChatStreamEvent) => void) => Promise<{ run_id: string }>,
    onAdmission?: (receipt: ChatAdmission) => void,
    publishArtifacts = true,
  ) {
    let admissionDelivered = false;
    const admit = (receipt: ChatAdmission) => {
      if (admissionDelivered) return;
      admissionDelivered = true;
      onAdmission?.(receipt);
    };

    return new ReadableStream<UIMessageChunk>({
      start: (controller) => {
        const responseId = `assistant-${crypto.randomUUID()}`;
        const textId = `text-${responseId}`;
        let started = false;
        let finished = false;
        let hasText = false;
        let textStarted = false;
        let abortRequested = false;
        let abortInFlight = false;
        let abortConfirmed = false;
        let abortAttempts = 0;
        let abortRetry: number | undefined;
        let progressState: ChatProgressStreamState | undefined;
        let answerLength = 0;
        let draft: ChatMessage["draft"];
        let answerKey: string | undefined;
        const publishedCommentary = new Set<string>();

        const enqueue = (chunk: UIMessageChunk) => {
          if (!finished && !abortSignal?.aborted) controller.enqueue(chunk);
        };
        const appendAnswer = (delta: string) => {
          hasText = true;
          startText();
          answerLength += delta.length;
          enqueue({ type: "text-delta", id: textId, delta });
        };
        const publishProgress = () => {
          if (!progressState) return;
          enqueue({ type: "data-progress", id: `progress-${responseId}`, data: progressState.progress });
        };
        // The first publish of a commentary carries the whole item, so the newest
        // one in a patch or snapshot is the draft the host is writing now.
        const noteDraft = (items: readonly ChatProgressItem[]) => {
          const fresh = items.filter((item) => item.kind === "commentary" && !publishedCommentary.has(item.id));
          for (const item of fresh) publishedCommentary.add(item.id);
          const newest = fresh.at(-1);
          if (!newest) return;
          draft = { id: newest.id, answerLength, settled: false };
          enqueue({ type: "message-metadata", messageMetadata: { draft } });
        };
        // Said before the settled text arrives, so the view keeps drawing the draft until
        // the answer has grown, then its key, and never both copies at once.
        const noteSettle = (settled: string) => {
          const metadata: Partial<LoupistUIMessage["metadata"] & object> = {};
          if (draft?.id === settled) metadata.draft = draft = { ...draft, settled: true };
          if (answerKey === undefined) metadata.answer_key = answerKey = settled;
          if (Object.keys(metadata).length) enqueue({ type: "message-metadata", messageMetadata: metadata });
        };
        const startResponse = (createdAt = new Date().toISOString()) => {
          if (started) return;
          started = true;
          enqueue({
            type: "start",
            messageId: responseId,
            messageMetadata: { created_at: createdAt, status: "streaming" },
          });
        };
        const startText = () => {
          if (textStarted) return;
          textStarted = true;
          enqueue({ type: "text-start", id: textId });
        };
        const close = () => {
          if (finished) return;
          finished = true;
          if (abortRetry !== undefined) window.clearTimeout(abortRetry);
          if (!abortSignal?.aborted) controller.close();
          abortSignal?.removeEventListener("abort", abortNativeRun);
        };
        const fail = (
          message: string,
          terminal?: Extract<ChatStreamEvent, { type: "error" }>,
        ) => {
          startResponse(terminal?.created_at);
          const finalText = terminal?.final_text ?? message;
          if (!hasText) appendAnswer(finalText);
          if (terminal?.progress) {
            enqueue({
              type: "data-progress",
              id: `progress-${responseId}`,
              data: terminal.progress,
            });
          }
          enqueue({
            type: "message-metadata",
            messageMetadata: {
              status: "error",
              ...(terminal
                ? {
                    native_message_id: terminal.message_id,
                    final_text: terminal.final_text,
                    ...(terminal.placeholder ? { placeholder: true } : {}),
                    created_at: terminal.created_at,
                    execution_model: terminal.execution_model,
                    agent_harness: terminal.agent_harness,
                    failure_kind: terminal.failure_kind,
                    retry_after_seconds: terminal.retry_after_seconds,
                  }
                : {}),
            },
          });
          if (textStarted) enqueue({ type: "text-end", id: textId });
          enqueue({ type: "error", errorText: message });
          close();
        };
        const abortNativeRun = () => {
          abortRequested = true;
          if (
            finished ||
            abortConfirmed ||
            abortInFlight ||
            abortAttempts >= 50
          ) {
            return;
          }
          abortInFlight = true;
          abortAttempts += 1;
          void this.bridge
            .chatAbort(runId)
            .then(() => {
              abortConfirmed = true;
            })
            .catch(() => {
              if (!finished && abortRequested && abortAttempts < 50) {
                abortRetry = window.setTimeout(abortNativeRun, 100);
              }
            })
            .finally(() => {
              abortInFlight = false;
            });
        };

        abortSignal?.addEventListener("abort", abortNativeRun, { once: true });

        const onEvent = (event: ChatStreamEvent) => {
          if (finished) return;
          if (!isKnownStreamEvent(event)) { ignoreUnknownStreamEvent(event); return; }
          if (event.run_id !== runId) return;
          admit({ accepted: true, runId });
          if (event.type === "started") {
            startResponse();
            enqueue({
              type: "data-run",
              data: { run_id: event.run_id },
              transient: true,
            });
            if (abortSignal?.aborted) abortNativeRun();
            return;
          }

          if (event.type === "title") {
            if (!abortSignal?.aborted) {
              this.onTitle?.(context.loupist_id, context.thread_id, event.title);
            }
            return;
          }

          startResponse(
            event.type === "completed" || event.type === "error"
              ? event.created_at
              : undefined,
          );
          if (event.type === "schedule") {
            enqueue({ type: "data-schedule", id: event.receipt.task_id, data: event.receipt });
            return;
          }
          if (event.type === "evidence") {
            enqueue({ type: "data-evidence", id: event.evidence.id, data: event.evidence });
            return;
          }
          if (event.type === "memory") {
            enqueue({ type: "data-memory", data: event.memories });
            return;
          }
          if (event.type === "generation_finished") {
            enqueue({
              type: "data-performance",
              id: `performance-${responseId}`,
              data: event.performance,
              transient: true,
            });
            return;
          }
          if (event.type === "delta") {
            if (event.settles) noteSettle(event.settles);
            appendAnswer(event.text);
            if (event.settles) {
              // The view stops drawing the settled draft once the answer grew past
              // it, so the copy may share a projection with the text. It still
              // leaves here: a stop before Rust's removal patch would otherwise
              // keep it as a draft beside the text it already settled into.
              const settled = event.settles;
              if (progressState?.progress.items.some((item) => item.id === settled)) {
                progressState = { ...progressState, progress: { ...progressState.progress, items: progressState.progress.items.filter((item) => item.id !== settled) } };
                publishProgress();
              }
            }
            return;
          }
          if (event.type === "phase") {
            enqueue({ type: "message-metadata", messageMetadata: { phase: event.phase } });
            return;
          }
          if (event.type === "progress" || event.type === "progress_patch") {
            const next = event.type === "progress"
              ? applyChatProgressSnapshot(progressState, event.progress, event.sequence)
              : applyChatProgressPatch(progressState, event.patch);
            if (!next || next === progressState) return;
            progressState = next;
            noteDraft(event.type === "progress" ? event.progress.items : event.patch.upsertItems ?? []);
            publishProgress();
            return;
          }
          if (event.type === "memory_update") {
            enqueue({ type: "data-memory_update", data: event.result });
            return;
          }
          if (event.type === "artifact") {
            enqueue({
              type: "data-artifact",
              id: `${event.artifact.artifact_id}:${event.artifact.revision}`,
              data: event.artifact,
            });
            if (publishArtifacts && !abortSignal?.aborted) {
              this.onArtifact?.(
                context.loupist_id,
                context.thread_id,
                event.artifact,
              );
            }
            return;
          }
          if (event.type === "completed") {
            if (!hasText && event.final_text) appendAnswer(event.final_text);
            enqueue({
              type: "message-metadata",
              messageMetadata: {
                native_message_id: event.message_id,
                final_text: event.final_text,
                created_at: event.created_at,
                status: "complete",
                lens_receipts: event.lens_receipts,
                execution_model: event.execution_model,
                agent_harness: event.agent_harness,
                host_composed: event.host_composed,
                tool_refusals: event.tool_refusals,
                offline_reads: event.offline_reads,
                replaced_model: event.replaced_model,
                unsaved_theses: event.unsaved_theses,
                as_of: event.as_of,
                number_checks: event.number_checks,
                sensitivity_cards: event.sensitivity_cards,
                as_of_report: event.as_of_report,
                performance: event.performance,
              },
            });
            if (textStarted) enqueue({ type: "text-end", id: textId });
            enqueue({ type: "finish", finishReason: "stop" });
            close();
            return;
          }
          if (event.type === "aborted") {
            if (!hasText) {
              hasText = true;
              startText();
              enqueue({
                type: "text-delta",
                id: textId,
                delta: t("aiSdk.responseStopped"),
              });
            }
            enqueue({
              type: "message-metadata",
              messageMetadata: {
                status: "aborted",
              },
            });
            if (textStarted) enqueue({ type: "text-end", id: textId });
            enqueue({ type: "abort", reason: t("aiSdk.stoppedByUser") });
            close();
            return;
          }
          if (event.type === "error") {
            fail(event.message, event);
            return;
          }
          event satisfies never;
        };

        const rejectStart = (message: string) => {
          if (admissionDelivered) { fail(message); return; }
          admit({ accepted: false, error: message });
          finished = true;
          if (abortRetry !== undefined) window.clearTimeout(abortRetry);
          abortSignal?.removeEventListener("abort", abortNativeRun);
          controller.error(new Error(message));
        };

        void connect(onEvent)
          .then(({ run_id: acknowledgedRunId }) => {
            if (acknowledgedRunId !== runId) {
              rejectStart(t("aiSdk.thisReplyGotOutOfStepWithTheConversation"));
              return;
            }
            admit({ accepted: true, runId });
            if (abortSignal?.aborted) abortNativeRun();
          })
          .catch((cause) => {
            // Rust refuses a file over its limit by code; the reader gets the limit in their language.
            const tooLarge = commandErrorCode(cause) === "attachment_too_large";
            const message = tooLarge ? t("chatComposer.attachmentTooLarge") : errorMessage(cause, t("aiSdk.couldNotStartTheResponse"));
            if (finished) return;
            rejectStart(message);
          });
      },
    });
  }

  async prepareScheduled(run: ScheduledRun): Promise<ScheduledStreamConnection | null> {
    if (!run.thread_id) return null;
    const request = { loupist_id: run.loupist_id, task_id: run.task_id, run_id: run.run_id };
    const buffered: ChatStreamEvent[] = [];
    let receive = (event: ChatStreamEvent) => { buffered.push(event); };
    const release = () => { receive = () => {}; buffered.length = 0; };
    let attached;
    try { attached = await this.bridge.scheduledTaskWatch(request, (event) => receive(event)); }
    catch (cause) { release(); throw cause; }
    if (!attached) { release(); return null; }
    if (attached.run.run_id !== run.run_id || attached.run.task_id !== run.task_id || attached.run.loupist_id !== run.loupist_id || attached.thread.id !== run.thread_id || attached.thread.loupist_id !== run.loupist_id) {
      release();
      throw new Error(t("aiSdk.theScheduledStreamBelongsToAnotherChat"));
    }
    return { run: attached.run, thread: attached.thread, release, connect: (observer) => {
      receive = observer;
      for (const event of buffered) observer(event);
      buffered.length = 0;
    } };
  }

  async reconnectToStream({ body, abortSignal }: Parameters<ChatTransport<LoupistUIMessage>["reconnectToStream"]>[0]) {
    const connection = (body as { scheduledConnection?: ScheduledStreamConnection } | undefined)?.scheduledConnection;
    if (!connection?.run.thread_id) return null;
    if (abortSignal?.aborted) {
      connection.release();
      return null;
    }
    const run = connection.run;
    return this.nativeStream({ loupist_id: run.loupist_id, thread_id: run.thread_id! }, abortSignal, run.run_id, async (observer) => {
      connection.connect(observer);
      return { run_id: run.run_id };
    }, undefined, false);
  }
}
