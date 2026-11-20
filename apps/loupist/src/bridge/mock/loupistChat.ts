import { LOUPIST_AVATAR_IDS, LOUPIST_TONES } from "../../contracts/loupist";
import i18n from "@/i18n";
import type { EffectiveConfig } from "../../contracts/config";
import type { ConnectionView } from "../../contracts/knowledge";
import type { ModelCatalog } from "../../contracts/model";
import { commandError } from "./commandError";
import { executionModelLock } from "./models";
import { defaultRunOptions, defaultThinkingLevel } from "../../lib/modelSelection";
import type {
  AgentHarnessSnapshot,
  ChatArtifact,
  LoupistArtifactItem,
  ChartDataset,
  ChatArtifactRef,
  ChatArtifactRevision,
  ChatArtifactView,
  ChatCreateRequest,
  ChatEvidenceRef,
  ChatControlReceipt,
  ChatControlRequest,
  ChatDeleteRequest,
  ChatMessage,
  ChatProgress,
  ChatRenameRequest,
  ChatSendRequest,
  ChatStreamEvent,
  ChatThread,
  ExecutionModelLock,
  LoupistCreateRequest,
  LoupistProfileUpdateRequest,
  LoupistDeleteRequest,
  LoupistSummary,
  LoupistWorkspace,
  MemoryRef,
  MemoryUpdateResult,
  ResearchHandoffReceipt,
  ResearchHandoffRequest,
  StreamObserver,
} from "../../types";
import {
  DEFAULT_AGENT_SKILL_IDS,
  clone,
  ensureLoupistHasNoActiveRuns,
  finishRunActivity,
  makeId,
  registerRunActivity,
  shortPause,
  wait,
  waitForStreamResume,
  type MockBridgeState,
} from "./state";

import { knowledgeCatalog, mockFileSection } from "./knowledge";

/** What every completed mock run reports alongside its answer, as Rust always does. */
const COMPLETED_RUN: Pick<Extract<ChatStreamEvent, { type: "completed" }>, "lens_receipts" | "tool_refusals" | "host_composed" | "performance"> = {
  lens_receipts: [],
  tool_refusals: [],
  host_composed: false,
  performance: { setupMs: 40, generationMs: 400, totalMs: 460, sessionCache: "warm", inputTokens: 320, outputTokens: 96, cacheReadTokens: 0, cacheWriteTokens: 0 },
};


export const loupistList = async (
  state: MockBridgeState,
): Promise<LoupistSummary[]> => clone(state.loupists);

/** One home Thread per Loupist, as Rust guarantees: the id derives from the Loupist. */
export const homeThreadId = (loupistId: string) => `home-${loupistId}`;

export function ensureHomeThread(state: MockBridgeState, loupistId: string): ChatThread {
  state.threads[loupistId] ??= [];
  const existing = state.threads[loupistId].find((thread) => thread.home);
  if (existing) return existing;
  const home: ChatThread = { id: homeThreadId(loupistId), loupist_id: loupistId, home: true, title: "Thread", updated_at: new Date().toISOString(), use_memory: true, update_memory: true, messages: [] };
  state.threads[loupistId].push(home);
  return home;
}

export const loupistSelect = async (
  state: MockBridgeState,
  loupist_id: string,
): Promise<LoupistWorkspace> => {
  const loupist = state.loupists.find((item) => item.id === loupist_id);
  if (!loupist) throw new Error("Loupist not found.");
  ensureHomeThread(state, loupist_id);
  return {
    loupist: clone(loupist),
    threads: clone(state.threads[loupist_id] ?? []).map(listedAsRust),
  };
};

/** Rust keeps a scheduled run's session out of Research sessions until the user writes in it. */
const listedAsRust = (thread: ChatThread): ChatThread =>
  thread.messages.some((message) => message.scheduled) && thread.messages.every((message) => message.role !== "user" || message.scheduled)
    ? { ...thread, hide_from_recent: true } : thread;

export const loupistCreate = async (
  state: MockBridgeState,
  request: LoupistCreateRequest,
): Promise<LoupistSummary> => {
  validateProfileUpdate(request);
  const loupist: LoupistSummary = {
    id: makeId("loupist"),
    name: request.name.trim(),
    workspace_path: `/mock/app/loupists/${makeId("workspace")}`,
    philosophy: request.focus?.trim() ?? "",
    instructions: "",
    avatar: request.avatar ?? "initial",
    tone: request.tone ?? "balanced",
    ...(request.template ? { template: request.template } : {}),
    record_count: 0,
    updated_at: new Date().toISOString(),
    accent: "var(--gg-color-accent)",
    enabled_skill_ids: [...DEFAULT_AGENT_SKILL_IDS],
    availability: { status: "available" },
    last_model_profile_id: null,
  };
  state.loupists.push(loupist);
  state.threads[loupist.id] = [];
  await shortPause(state);
  return clone(loupist);
};

export const loupistDelete = async (
  state: MockBridgeState,
  request: LoupistDeleteRequest,
): Promise<void> => {
  ensureLoupistHasNoActiveRuns(state, request.loupist_id);
  const index = state.loupists.findIndex((item) => item.id === request.loupist_id);
  if (index < 0) throw new Error("Loupist not found.");
  const threadIds = new Set(
    (state.threads[request.loupist_id] ?? []).map((thread) => thread.id),
  );
  state.loupists.splice(index, 1);
  delete state.threads[request.loupist_id];
  threadIds.forEach((threadId) => delete state.artifacts[threadId]);
  await shortPause(state);
};

export const chatCreate = async (
  state: MockBridgeState,
  request: ChatCreateRequest,
): Promise<ChatThread> => {
  const thread: ChatThread = {
    id: makeId("thread"),
    loupist_id: request.loupist_id,
    title: request.title?.trim() || "",
    updated_at: new Date().toISOString(),
    use_memory: true,
    update_memory: true,
    messages: [],
  };
  state.threads[request.loupist_id] ??= [];
  state.threads[request.loupist_id].unshift(thread);
  return clone(thread);
};

function retainedLink(state: MockBridgeState, loupistId: string, connectionId: string) {
  const home = state.threads[loupistId]?.find((thread) => thread.home);
  const post = home?.messages.find((message) =>
    message.role === "assistant" && message.status === "complete" &&
    message.host_composed && message.link?.connectionId === connectionId,
  );
  if (!home || !post?.link) throw commandError("not_found", "Retained link not found.");
  return { home, link: post.link, createdAtMs: Date.parse(post.created_at) };
}

const linkKey = (loupistId: string, connectionId: string) => JSON.stringify([loupistId, connectionId]);

export function dismissHandoffLink(
  state: MockBridgeState, loupistId: string, connectionId: string,
): ConnectionView {
  const { link, createdAtMs } = retainedLink(state, loupistId, connectionId);
  const key = linkKey(loupistId, connectionId);
  state.handoffLinks.set(key, { ...state.handoffLinks.get(key), status: "dismissed" });
  return {
    connectionId, createdAtMs, status: "dismissed", score: 0,
    seedName: link.seedName, farName: link.farName,
    seedNodeId: link.nodeIds[0], farNodeId: link.nodeIds.at(-1) ?? "",
    path: link.path.map((hop, index) => ({
      ...hop, reversed: hop.reversed ?? false, nodeId: link.nodeIds[index + 1], claimId: "", fromRole: "", toRole: "",
    })),
  };
}

export const researchHandoffStart = async (
  state: MockBridgeState,
  request: ResearchHandoffRequest,
  catalog: ModelCatalog,
  chatDefaults: () => EffectiveConfig["chat"],
): Promise<ResearchHandoffReceipt> => {
  const acceptedKeys = ["loupistId", "connectionId", "modelProfileId", "thinkingLevel", "runOptions", "useMemory", "updateMemory"];
  if (Object.keys(request).some((key) => !acceptedKeys.includes(key))) {
    throw commandError("invalid_request", "Unknown research hand-off field.");
  }
  const { loupistId, connectionId, modelProfileId, thinkingLevel, runOptions, useMemory, updateMemory } = request;
  const profile = state.loupists.find((item) => item.id === loupistId);
  if (!profile) throw commandError("not_found", "Loupist not found.");
  const key = linkKey(loupistId, connectionId);
  const entry = state.handoffLinks.get(key);
  // A session at work carries its run; Bot posts nothing for it until it ends.
  const working = state.threads[loupistId]?.find((thread) => thread.handoff_run?.connection_id === connectionId);
  const options = {
    loupistId, connectionId,
    modelProfileId: modelProfileId ?? undefined, thinkingLevel: thinkingLevel ?? undefined,
    runOptions: runOptions == null ? undefined : Object.fromEntries(Object.entries(runOptions).sort()),
    useMemory: useMemory ?? undefined, updateMemory: updateMemory ?? undefined,
  };
  if (working?.handoff_run) {
    if (JSON.stringify(entry?.options) !== JSON.stringify(options)) {
      throw commandError("conflict", "This link already has a research request with different settings.");
    }
    return { chatId: working.id, handoffId: working.handoff_run.handoff_id };
  }
  const { home, link } = retainedLink(state, loupistId, connectionId);
  if (entry?.status === "dismissed" || entry?.status === "filed") {
    throw commandError("not_found", "Retained link not found.");
  }
  const defaults = chatDefaults();
  const modelId = modelProfileId ?? defaults.model_profile_id ?? profile.last_model_profile_id ?? "";
  const model = catalog.models.find((item) => item.id === modelId);
  const validControls = (controls: Record<string, string>, selected: typeof model) =>
    selected?.run_controls.length === Object.keys(controls).length
      && selected.run_controls.every((control) => control.choices.some((choice) => choice.id === controls[control.id]));
  const controls = runOptions ?? (model && !validControls(defaults.run_options, model)
    ? defaultRunOptions(model) : defaults.run_options);
  const level = thinkingLevel || (defaults.thinking_level && model?.thinking_levels.includes(defaults.thinking_level)
    ? defaults.thinking_level : defaultThinkingLevel(model?.thinking_levels ?? []));
  let execution;
  try {
    execution = executionModelLock(catalog, modelId, level, controls);
  } catch {
    throw commandError("invalid_request", "The selected Chat settings are unavailable.");
  }
  const admitted = catalog.models.find((item) => item.id === execution.profile_id);
  if ((modelId && !validControls(controls, admitted))
    || (thinkingLevel && thinkingLevel !== execution.thinking_level)
    || (runOptions != null && Object.entries(runOptions).some(([key, value]) =>
      execution.run_options[key] !== value || !admitted?.run_controls.some((control) =>
        control.id === key && control.choices.some((choice) => choice.id === value))))) {
    throw commandError("invalid_request", "The selected Chat settings are unavailable.");
  }
  const handoffId = makeId("handoff");
  const title = i18n.resolvedLanguage === "ko"
    ? `연결: ${link.seedName} ↔ ${link.farName}`
    : `Link: ${link.seedName} and ${link.farName}`;
  const created = new Date().toISOString();
  const linkPath = {
    seed: link.seedName, far: link.farName,
    path: link.path.map((hop, index) => {
      const prior = link.path[index - 1]?.name ?? link.seedName;
      return { from: hop.reversed ? hop.name : prior, relation: hop.relation, to: hop.reversed ? prior : hop.name };
    }),
  };
  const answer = "Research on this link is complete.";
  const session: ChatThread = {
    id: makeId("thread"), loupist_id: loupistId, title, updated_at: created,
    use_memory: useMemory ?? defaults.use_memory, update_memory: updateMemory ?? defaults.update_memory,
    messages: [
      { id: makeId("message"), role: "user", content: "The user chose “Look into it” on this Map link.",
        handoff_action: { kind: "link", connectionId }, handoff_note: { title, linkPath },
        status: "complete", created_at: created },
    ],
    handoff_run: { handoff_id: handoffId, run_id: makeId("handoff"), connection_id: connectionId },
  };
  state.threads[loupistId].unshift(session);
  state.handoffLinks.set(key, { status: "opened", options });
  // The session answers at once here; then Bot posts its result.
  await Promise.resolve();
  delete session.handoff_run;
  session.messages.push({ id: makeId("message"), role: "assistant", content: answer, status: "complete",
    created_at: created, execution_model: execution });
  home.messages.push({
    id: makeId("message"), role: "assistant", content: answer, status: "complete", created_at: created,
    host_composed: true, handoff: { handoffId, chatId: session.id, connectionId, title, status: "done" },
  });
  return { chatId: session.id, handoffId };
};

export const chatRename = async (
  state: MockBridgeState,
  request: ChatRenameRequest,
): Promise<ChatThread> => {
  const runId = makeId("chat-mutation");
  registerRunActivity(state, {
    run_id: runId,
    loupist_id: request.loupist_id,
    kind: "chat_mutation",
    target: request.thread_id,
    started_at_ms: Date.now(),
  });
  try {
    const thread = state.threads[request.loupist_id]?.find(
      (item) => item.id === request.thread_id,
    );
    if (!thread) throw new Error("Chat thread not found.");
    if (thread.home) throw new Error("The home Thread keeps its name.");
    const title = request.title.trim();
    if (!title) throw new Error("Chat title is required.");
    await shortPause(state);
    thread.title = title;
    return clone(thread);
  } finally {
    finishRunActivity(state, runId);
  }
};

export const chatDelete = async (
  state: MockBridgeState,
  request: ChatDeleteRequest,
): Promise<void> => {
  const runId = makeId("chat-mutation");
  registerRunActivity(state, {
    run_id: runId,
    loupist_id: request.loupist_id,
    kind: "chat_mutation",
    target: request.thread_id,
    started_at_ms: Date.now(),
  });
  try {
    const threads = state.threads[request.loupist_id] ?? [];
    const index = threads.findIndex((item) => item.id === request.thread_id);
    if (index < 0) throw new Error("Chat thread not found.");
    if (threads[index].home) throw new Error("The home Thread cannot be deleted.");
    await shortPause(state);
    threads.splice(index, 1);
    delete state.artifacts[request.thread_id];
  } finally {
    finishRunActivity(state, runId);
  }
};

export const chatAttachmentRead = async (
  state: MockBridgeState,
  loupist_id: string,
  thread_id: string,
  message_id: string,
  attachment_id: string,
): Promise<{ data_url: string }> => {
  const message = state.threads[loupist_id]
    ?.find((thread) => thread.id === thread_id)
    ?.messages.find((item) => item.id === message_id);
  const attachment = message?.attachments?.find(
    (item) => item.id === attachment_id,
  );
  if (!attachment?.url) throw new Error("Chat attachment not found.");
  return { data_url: attachment.url };
};

export const chatArtifactList = async (
  state: MockBridgeState,
  loupist_id: string,
  thread_id: string,
): Promise<ChatArtifact[]> => {
  const thread = state.threads[loupist_id]?.find((item) => item.id === thread_id);
  if (!thread) throw new Error("Chat thread not found.");
  return clone(
    (state.artifacts[thread_id] ?? [])
      .map((entry) => entry.artifact)
      .sort((left, right) => right.updated_at_ms - left.updated_at_ms),
  );
};

export const loupistArtifactList = async (
  state: MockBridgeState,
  request: { loupist_id: string; limit?: number },
): Promise<LoupistArtifactItem[]> => {
  const threads = state.threads[request.loupist_id] ?? [];
  const items = threads.flatMap((thread) => (state.artifacts[thread.id] ?? []).map((entry) => ({ artifact: entry.artifact, chat_title: thread.title })));
  return clone(items.sort((left, right) => right.artifact.updated_at_ms - left.artifact.updated_at_ms).slice(0, request.limit ?? 50));
};

type MockArtifactEntry = MockBridgeState["artifacts"][string][number];

const retainedVersions = (entry: MockArtifactEntry) => {
  entry.history ??= [];
  if (!entry.history.some((view) => view.revision.revision === entry.content.revision)) {
    entry.history.unshift(clone({ artifact: entry.artifact, revision: entry.content, chart_dataset: entry.dataset ?? null }));
  }
  let bytes = 0;
  entry.history = entry.history.sort((a, b) => b.revision.revision - a.revision.revision).filter((view, index) => {
    bytes += new TextEncoder().encode(JSON.stringify(view)).length;
    return index < 20 && bytes <= 32 * 1024 * 1024;
  });
  return entry.history;
};

const ownedArtifact = (state: MockBridgeState, loupist_id: string, thread_id: string, artifact_id: string) => {
  const thread = state.threads[loupist_id]?.find((item) => item.id === thread_id);
  if (!thread) throw new Error("Chat thread not found.");
  const entry = (state.artifacts[thread_id] ?? []).find((item) => item.artifact.id === artifact_id);
  if (!entry) throw new Error("Artifact not found.");
  return entry;
};

export const chatArtifactRead = async (
  state: MockBridgeState,
  loupist_id: string,
  thread_id: string,
  artifact_id: string,
  revision_id?: number,
): Promise<ChatArtifactView> => {
  const entry = ownedArtifact(state, loupist_id, thread_id, artifact_id);
  const view = retainedVersions(entry).find((item) => item.revision.revision === (revision_id ?? entry.content.revision));
  if (!view) throw new Error("Artifact revision is no longer available.");
  return clone({ ...view, artifact: { ...view.artifact, current_revision: entry.content.revision } });
};

export const chatArtifactVersions = async (
  state: MockBridgeState, loupist_id: string, thread_id: string, artifact_id: string,
): Promise<import("../../contracts/chat").ChatArtifactVersion[]> => {
  const entry = ownedArtifact(state, loupist_id, thread_id, artifact_id);
  return clone(retainedVersions(entry).map((view) => ({
    revision_id: view.revision.revision, title: view.artifact.title, digest: view.revision.digest,
    source_message_id: view.revision.source_message_id, created_at_ms: view.revision.created_at_ms,
    restored_from_revision_id: view.restored_from_revision_id,
  })));
};

export const chatArtifactRestore = async (
  state: MockBridgeState, request: import("../../contracts/chat").ChatArtifactRestoreRequest,
): Promise<ChatArtifactView> => {
  const entry = ownedArtifact(state, request.loupist_id, request.thread_id, request.artifact_id);
  if (entry.content.revision !== request.expected_current_revision) throw new Error("Artifact changed before restore.");
  if ([...state.run_activities.values()].some((run) => run.loupist_id === request.loupist_id && run.target === request.thread_id && ["chat", "chat_mutation"].includes(run.kind))) {
    throw new Error("This exact Loupist run target is already active.");
  }
  const retained = retainedVersions(entry).find((view) => view.revision.revision === request.revision_id);
  if (!retained) throw new Error("Artifact revision is no longer available.");
  const previous = clone(retained);
  const revision = entry.content.revision + 1;
  const timestamp = Math.max(Date.now(), entry.artifact.updated_at_ms);
  entry.artifact = { ...previous.artifact, current_revision: revision, updated_at_ms: timestamp };
  entry.content = { ...previous.revision, revision, created_at_ms: timestamp };
  entry.dataset = previous.chart_dataset ?? undefined;
  entry.history!.unshift(clone({ ...previous, artifact: entry.artifact, revision: entry.content, restored_from_revision_id: request.revision_id }));
  retainedVersions(entry);
  return chatArtifactRead(state, request.loupist_id, request.thread_id, request.artifact_id);
};

const mockArtifactForPrompt = (
  state: MockBridgeState,
  request: ChatSendRequest,
  message_id: string,
  created_at: string,
): ChatArtifactRef | undefined => {
  const entries = (state.artifacts[request.thread_id] ??= []);
  const referencedArtifactId = request.selected_artifact?.artifact_id ?? /@artifact\/([^\s]+)/u.exec(request.prompt)?.[1];
  const referenced = referencedArtifactId
    ? entries.find((entry) => entry.artifact.id === referencedArtifactId)
    : undefined;
  const revising =
    Boolean(referenced) && /\brevise\b|\bupdate\b|수정|추가/iu.test(request.prompt);
  const requestsArtifactOutput =
    /\bartifact\b|\bdocument\b|markdown|문서|\bchart\b|차트/iu.test(
      request.prompt,
    );
  if (!requestsArtifactOutput && !revising) return undefined;

  const wantsChart =
    /\bchart\b|차트/iu.test(request.prompt) ||
    (revising && referenced?.artifact.kind === "chart");
  const wantsMarkdown =
    /\bartifact\b|\bdocument\b|markdown|문서/iu.test(
    request.prompt,
    ) || (revising && referenced?.artifact.kind === "markdown");
  if (!wantsChart && !wantsMarkdown) return undefined;

  const artifact_id = revising ? referenced!.artifact.id : makeId("artifact");
  const revision = revising ? referenced!.artifact.current_revision + 1 : 1;
  const timestamp = Date.parse(created_at);
  const chartDataset: ChartDataset | undefined = wantsChart ? {
    id: makeId("dataset"),
    columns: [
      { id: "date", label: "Date", kind: "date" },
      { id: "open", label: "Open", kind: "number" },
      { id: "high", label: "High", kind: "number" },
      { id: "low", label: "Low", kind: "number" },
      { id: "close", label: "Close", kind: "number" },
      { id: "volume", label: "Volume", kind: "number" },
    ],
    rows: [
      ["2026-08-04", 100, 105, 98, 103, 1200],
      ["2026-08-05", 103, 108, 101, 106, 1800],
      ["2026-08-06", 106, 109, 102, 104, 1500],
    ],
    lineage: { kind: "agent_authored", upstream_receipts: [] },
    digest: "d".repeat(64),
  } : undefined;
  const payload: ChatArtifactRevision["payload"] = wantsChart
    ? {
        kind: "chart",
        schema: "loupist-chart/1",
        chart: {
          dataset_id: chartDataset!.id,
          dataset_digest: chartDataset!.digest,
          view: {
            kind: "candlestick",
            symbol: "MOCK",
            interval: "1d",
            time: "date",
            open: "open",
            high: "high",
            low: "low",
            close: "close",
            volume: "volume",
            price_precision: 2,
            turnover: null,
          },
          studies: [{ module_id: "VOL", calc_params: [] }],
          drawings: [],
          note: "Daily prices and volume.",
          custom_series: [],
        },
      }
    : {
        kind: "markdown",
        schema: "loupist-markdown/1",
        markdown: [
          "# Research note",
          "",
          "## Summary",
          "",
          `This document was created for: **${request.prompt}**`,
          "",
          "- Separate observations from interpretation.",
          "- Keep unresolved claims explicit.",
        ].join("\n"),
      };
  const digest = String(revision).repeat(64).slice(0, 64);
  const artifactRevision: ChatArtifactRevision = {
    artifact_id,
    revision,
    payload,
    digest,
    source_message_id: message_id,
    created_at_ms: timestamp,
  };
  const title = wantsChart ? "Price and volume" : "Research note";
  if (revising) {
    retainedVersions(referenced!);
    referenced!.artifact = {
      ...referenced!.artifact,
      title,
      current_revision: revision,
      updated_at_ms: timestamp,
    };
    referenced!.content = artifactRevision;
    referenced!.dataset = chartDataset;
    retainedVersions(referenced!);
  } else {
    entries.push({
      artifact: {
        id: artifact_id,
        chat_session_id: request.thread_id,
        kind: payload.kind,
        title,
        current_revision: revision,
        created_at_ms: timestamp,
        updated_at_ms: timestamp,
      },
      content: artifactRevision,
      dataset: chartDataset,
    });
  }
  return { artifact_id, revision, kind: payload.kind, title, digest };
};

/** A question about NVIDIA: the mock agent reads its quote. */
const asksAboutNvidia = (prompt: string) => /nvidia|nvda|엔비디아/iu.test(prompt);

/** NVIDIA's quote as a bundled read returns it, retained like any tool result. */
const nvidiaQuote = (retrieved_at: string): ChatEvidenceRef => ({
  id: "ev_quote_nvda", title: "NVDA · Quote", kind: "original", provider: "yfinance", tool_name: "mcp__openbb__equity_price_quote", retrieved_at,
  excerpt: JSON.stringify({ results: [{ symbol: "NVDA", last_price: 162.85, prev_close: 166.19, change_percent: -2.01, currency: "USD", last_timestamp: "2026-11-13T21:00:00Z" }] }),
  excerpt_truncated: false, content_sha256: "c".repeat(64), warnings: [],
});

/** The few formal sentences the mock agent answers with, in the interface language; the quote is its source. */
const nvidiaAnswer = () => typeof document !== "undefined" && document.documentElement.lang.toLowerCase().startsWith("ko")
  ? "엔비디아는 금요일 162.85달러에 마감했습니다. 하루 2.01% 하락했습니다. 하락 원인으로 확인된 회사 뉴스는 없습니다."
  : "NVIDIA closed at $162.85 on Friday. It fell 2.01% on the day. No company news was found that explains the drop.";

const mockChatTitle = (prompt: string) => {
  const title = prompt.split("\n", 1)[0]!.trim().replace(/\s+/gu, " ");
  const characters = Array.from(title);
  if (characters.length <= 40) return title;

  // Rust reserves the 40th code point for the ellipsis and keeps whole graphemes.
  const budget = characters.slice(0, 39).join("").length;
  let wordEnd: number | undefined;
  let graphemeEnd = 0;
  for (const { index, segment } of new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(title)) {
    if (index > budget) break;
    graphemeEnd = index;
    if (segment.startsWith(" ")) wordEnd = index;
  }
  return title.slice(0, wordEnd ?? graphemeEnd) + "…";
};

export const chatSend = async (
  state: MockBridgeState,
  request: ChatSendRequest,
  observer: StreamObserver<ChatStreamEvent>,
  capability_ids: string[],
  execution_model: ExecutionModelLock,
): Promise<{ run_id: string }> => {
  const run_id = request.run_id;
  const controller = new AbortController();
  registerRunActivity(state, {
    run_id,
    loupist_id: request.loupist_id,
    kind: "chat",
    target: request.thread_id,
    started_at_ms: Date.now(),
  });
  state.chat_runs.set(run_id, controller);
  window.setTimeout(() => {
    void streamChat(
      state,
      run_id,
      request,
      observer,
      controller,
      capability_ids,
      execution_model,
    );
  }, 0);
  return { run_id };
};

const submitChatControl = async (
  state: MockBridgeState,
  request: ChatControlRequest,
): Promise<ChatControlReceipt> => {
  const active = [...state.run_activities.values()].some(
    (run) =>
      run.kind === "chat" &&
      run.loupist_id === request.loupist_id &&
      run.target === request.thread_id,
  );
  if (!active) throw new Error("Active Chat run not found.");
  const thread = state.threads[request.loupist_id]?.find(
    (item) => item.id === request.thread_id,
  );
  if (!thread) throw new Error("Chat thread not found.");
  const prompt = request.prompt.trim();
  if (!prompt) throw new Error("Prompt is required.");
  const receipt: ChatControlReceipt = {
    message_id: makeId("message"),
    prompt,
    created_at: new Date().toISOString(),
    mode: "steer",
  };
  thread.messages.push({
    id: receipt.message_id,
    role: "user",
    content: prompt,
    created_at: receipt.created_at,
    status: "complete",
  });
  thread.updated_at = receipt.created_at;
  return receipt;
};

export const chatSteer = (
  state: MockBridgeState,
  request: ChatControlRequest,
) => submitChatControl(state, request);

export const chatRetryLast = (
  state: MockBridgeState,
  request: { loupist_id: string; thread_id: string },
) => {
  const active = [...state.run_activities.values()].some(
    (run) => run.kind === "chat" && run.loupist_id === request.loupist_id && run.target === request.thread_id,
  );
  if (active) throw new Error("Wait until the current response finishes.");
  const thread = state.threads[request.loupist_id]?.find((item) => item.id === request.thread_id);
  if (!thread) throw new Error("Chat thread not found.");
  const assistant = thread.messages.at(-1);
  const user = thread.messages.slice(0, -1).reverse().find(message => !message.steer);
  if (!assistant || assistant.role !== "assistant" || (assistant.status !== "error" && assistant.status !== "aborted") || !user || user.role !== "user") {
    throw new Error("Only the latest failed or stopped reply can be tried again.");
  }
  // As in Rust, a scheduled run's request is run again by its task.
  if (user.scheduled) throw new Error("Run the scheduled task again with Run now");
  const prompt = user.content;
  thread.messages = thread.messages.filter(message => message.id !== user.id && message.id !== assistant.id);
  thread.updated_at = new Date().toISOString();
  return { prompt, ...(user.as_of ? { as_of: user.as_of } : {}) };
};

const streamChat = async (
  state: MockBridgeState,
  run_id: string,
  request: ChatSendRequest,
  observer: StreamObserver<ChatStreamEvent>,
  controller: AbortController,
  capability_ids: string[],
  execution_model: ExecutionModelLock,
) => {
  const thread = state.threads[request.loupist_id]?.find(
    (item) => item.id === request.thread_id,
  );
  const created_at = new Date().toISOString();
  const message_id = makeId("assistant");
  const loupist = state.loupists.find((item) => item.id === request.loupist_id);
  const agent_harness: AgentHarnessSnapshot = {
    schema: "loupist-harness/1",
    mode: "chat",
    skill_ids: [...(loupist?.enabled_skill_ids ?? [])],
    capability_ids: [...capability_ids],
    digest: "e".repeat(64),
  };
  let content = "";
  let memory_refs: MemoryRef[] = [];
  const progress: ChatProgress = {
    startedAtMs: Date.now(),
    items: [],
    finishedAtMs: null,
  };
  let progressSequence = 0;
  const emitProgress = () => {
    observer({
      type: "progress",
      run_id,
      sequence: progressSequence,
      progress: clone(progress),
    });
    progressSequence += 1;
  };
  const setProgress = (
    id: string,
    action: string,
    status: "running" | "succeeded",
  ) => {
    const existing = progress.items.find(
      (item) => item.kind !== "commentary" && item.id === id,
    );
    if (existing && existing.kind !== "commentary") {
      existing.status = status;
      if (status === "succeeded") existing.finishedAtMs = Date.now();
    } else {
      progress.items.push({
        id,
        kind: id === "title" ? "system" : "tool",
        category: id.startsWith("memory-") ? "memory" : "system",
        operation: id.endsWith("search")
          ? "search"
          : id.endsWith("read")
            ? "read"
            : "generic",
        action,
        target: null,
        href: null,
        status,
        startedAtMs: Date.now(),
        finishedAtMs: status === "succeeded" ? Date.now() : null,
      });
    }
    emitProgress();
  };

  try {
    observer({ type: "started", run_id });
    if (thread) {
      thread.use_memory = request.use_memory;
      thread.update_memory = request.update_memory;
      thread.messages.push({
        id: makeId("user"),
        role: "user",
        content: request.prompt,
        created_at,
        status: "complete",
        ...(request.as_of ? { as_of: request.as_of } : {}),
        attachments: request.attachments.map((attachment) => ({
          id: makeId("attachment"),
          filename: attachment.filename,
          media_type: attachment.media_type,
          size_bytes: Math.floor((attachment.data_base64.length * 3) / 4),
          url: `data:${attachment.media_type};base64,${attachment.data_base64}`,
        })),
      });
    }
    if (state.chat_stream_gate) await waitForStreamResume(state, controller.signal);
    if (request.prompt.includes("patch timeline")) {
      emitProgress();
      const steps: Array<Extract<ChatProgress["items"][number], { kind: "tool" | "system" }>> = [
        { id: "web", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", running: "Searching the web", source: { kind: "web", name: "Web" }, target: "margin outlook", summary: "8 results", href: null, status: "succeeded", startedAtMs: Date.now(), finishedAtMs: Date.now() },
        { id: "mcp", kind: "tool", toolName: "mcp__openbb__equity_price_quote", category: "finance", operation: "read", action: "Read Equity price quote", running: "Reading Equity price quote", source: { kind: "connector", name: "Market data" }, target: "AAPL", summary: "1 results", href: null, status: "succeeded", startedAtMs: Date.now(), finishedAtMs: Date.now() },
        { id: "dart", kind: "tool", toolName: "dart_filing_search", category: "finance", operation: "read", action: "Read filings", running: "Reading filings", source: { kind: "connector", name: "OpenDART" }, target: "005930", summary: "3 results", href: null, status: "succeeded", startedAtMs: Date.now(), finishedAtMs: Date.now() },
        { id: "python", kind: "tool", toolName: "run_python", category: "compute", operation: "execute", action: "Ran Python", running: "Running Python", summary: "exit 2", target: null, href: null, status: "failed", startedAtMs: Date.now(), finishedAtMs: Date.now() },
        { id: "file", kind: "tool", toolName: "edit", category: "files", operation: "edit", action: "Edited a file", source: { kind: "files", name: "Files" }, filePath: "notes/margin.md", target: null, href: null, status: "succeeded", startedAtMs: Date.now(), finishedAtMs: Date.now() },
        { id: "memory", kind: "tool", toolName: "world_read", category: "memory", operation: "read", action: "Read saved notes", source: { kind: "memory", name: "Memory" }, target: null, href: null, status: "succeeded", startedAtMs: Date.now(), finishedAtMs: Date.now() },
        { id: "chart", kind: "tool", toolName: "chart_publish", category: "artifact", operation: "publish", action: "Made a chart", source: { kind: "artifact", name: "Documents" }, target: null, href: null, status: "succeeded", startedAtMs: Date.now(), finishedAtMs: Date.now() },
      ];
      for (const step of steps) {
        await wait(state.delay_ms, controller.signal);
        progress.items.push(step);
        progressSequence += 1;
        observer({ type: "progress_patch", run_id, patch: { sequence: progressSequence, upsertItems: [clone(step)] } });
      }
      if (request.prompt.includes("abort")) throw new DOMException("Stopped", "AbortError");
      content = "Patched timeline settled.";
      progress.finishedAtMs = Date.now();
      observer({ type: "delta", run_id, text: content });
      observer({ type: "completed", ...COMPLETED_RUN, run_id, message_id, final_text: content, created_at, execution_model: clone(execution_model), agent_harness: clone(agent_harness) });
      if (thread) {
        thread.messages.push({
          id: message_id,
          role: "assistant",
          content,
          created_at,
          status: "complete",
          execution_model: clone(execution_model),
          agent_harness,
          progress: clone(progress),
        });
        thread.updated_at = created_at;
      }
      return;
    }
    if (request.use_memory) {
      await wait(state.delay_ms, controller.signal);
      setProgress("memory-search", "Searched Memory", "running");
      await wait(state.delay_ms, controller.signal);
      setProgress("memory-search", "Searched Memory", "succeeded");
      setProgress("memory-read", "Read saved notes", "running");
      const catalog = await knowledgeCatalog(state, { loupistId: request.loupist_id });
      memory_refs = catalog.lenses.slice(0, 2).map((lens, index) => ({
        read_ref: lens.recordId,
        kind: "lens",
        title: lens.title,
        excerpt: lens.title,
        as_of: null,
        section: index === 0 ? "Method" : null,
        access: index === 0 ? "exact_read" : "search_discovered",
        knowledge: {
          kind: "lens",
          recordId: lens.recordId,
          revisionId: lens.revisionId,
          objectId: lens.recordId,
        },
      }));
      observer({
        type: "memory",
        run_id,
        memories: clone(memory_refs),
      });
      setProgress("memory-read", "Read saved notes", "succeeded");
    } else {
      await wait(state.delay_ms, controller.signal);
    }

    // A question that names NVIDIA reads a bundled quote, as the agent would pick it; the answer cites it.
    const read = asksAboutNvidia(request.prompt) ? nvidiaQuote(created_at) : undefined;
    if (read) {
      setProgress("quote-read", "Read the quote", "running");
      await wait(state.delay_ms, controller.signal);
      observer({ type: "evidence", run_id, evidence: clone(read) });
      setProgress("quote-read", "Read the quote", "succeeded");
    }

    const response = read
      ? nvidiaAnswer()
      : request.use_memory
      ? [
          "### Assessment",
          "",
          `I reviewed “${request.prompt}” against what the Loupist has already saved.`,
          "",
          "- **What we know:** The current evidence is not enough to declare lasting damage to business quality.",
          "- **How to frame it:** Separate the drivers into price, volume, one-time items, and reinvestment.",
          "- **Next check:** Revisit underlying margins and customer retention in the next filing.",
        ].join("\n")
      : [
          "### Assessment",
          "",
          `I reviewed “${request.prompt}” using only this conversation.`,
          "",
          "- **What we know:** Separate observed facts from assumptions before drawing a conclusion.",
          "- **Unresolved:** Leave unverified points open.",
          "- **Next check:** Let the Loupist remember this chat to compare this view with your existing criteria.",
        ].join("\n");

    const chunks = response.match(/[\s\S]{1,24}/gu) ?? [response];
    for (const text of chunks) {
      await wait(state.delay_ms, controller.signal);
      content += text;
      observer({ type: "delta", run_id, text });
    }

    let memory_update: MemoryUpdateResult | undefined;
    if (request.update_memory) {
      await wait(state.delay_ms, controller.signal);
      const commitId = makeId("memory-update");
      const catalog = await knowledgeCatalog(state, { loupistId: request.loupist_id });
      const module = catalog.lenses.find((lens) => lens.lensKind === "method");
      const learned = module
        ? mockFileSection(state, request.loupist_id, module.recordId, {
            key: "review-lesson", heading: "Review lesson", markdown: "Separate recurring changes from one-time movement.",
            chatId: request.thread_id, messageId: message_id,
          })
        : undefined;
      const update: MemoryUpdateResult = {
        status: learned ? "applied" : "no_change",
        commitId: learned ? commitId : null,
        changes: learned ? [
          {
            recordId: learned.recordId,
            kind: "Lens",
            operation: "revise",
            title: learned.title,
            knowledge: {
              kind: "lens",
              recordId: learned.recordId,
              revisionId: learned.revisionId,
              objectId: learned.recordId,
            },
            lesson: "Separate recurring earnings quality from one-time margin movement.",
            basis: "Current Chat evidence",
            futureUse: "This will change the checks used in later earnings research.",
          },
        ] : [],
      };
      memory_update = update;
      observer({ type: "memory_update", run_id, result: clone(update) });
    }

    if (thread?.title === "") {
      setProgress("title", "Named this chat", "running");
      await wait(state.delay_ms, controller.signal);
      const title = mockChatTitle(request.prompt);
      thread.title = title;
      observer({ type: "title", run_id, title });
      setProgress("title", "Named this chat", "succeeded");
    }

    const artifact = mockArtifactForPrompt(
      state,
      request,
      message_id,
      created_at,
    );
    if (artifact) {
      // As the host does: a chart is delivered as its call succeeds, its reference only at commit.
      if (artifact.kind !== "markdown") {
        observer({ type: "artifact_staged", run_id, view: clone(await chatArtifactRead(state, request.loupist_id, request.thread_id, artifact.artifact_id, artifact.revision)) });
      }
      observer({ type: "artifact", run_id, artifact: clone(artifact) });
    }

    progress.finishedAtMs = Date.now();
    // A data read that never reached its server (device offline).
    const offline_reads = request.prompt.includes("offline data read") || undefined;
    // An as-of request: the answer keeps the date and reports what it left out.
    const asOf = request.as_of
      ? { as_of: request.as_of, as_of_report: { cutoff: request.as_of, left_out: { facts: 0, sources: 0, notes: 0, methods: 0 } } }
      : {};
    observer({
      type: "completed", ...COMPLETED_RUN, offline_reads, ...asOf,
      run_id,
      message_id,
      final_text: content,
      created_at,
      execution_model: clone(execution_model),
      agent_harness: clone(agent_harness),
    });
    if (thread) {
      const assistant: ChatMessage = {
        tool_refusals: [],
        offline_reads,
        ...asOf,
        id: message_id,
        role: "assistant",
        content,
        created_at,
        status: "complete",
        memory_refs: clone(memory_refs),
        memory_update,
        memory_revision: request.use_memory
          ? "mock-tree-revision"
          : undefined,
        execution_model: clone(execution_model),
        agent_harness,
        artifact_refs: artifact ? [artifact] : undefined,
        ...(read ? { evidence_refs: [clone(read)] } : {}),
        progress: clone(progress),
      };
      thread.messages.push(assistant);
      thread.updated_at = created_at;
    }
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") {
      progress.finishedAtMs = Date.now();
      for (const item of progress.items) {
        if (item.kind !== "commentary" && item.status === "running") {
          item.status = "stopped";
        }
      }
      if (thread) {
        thread.messages.push({
          id: message_id,
          role: "assistant",
          content: content || "Response stopped.",
          created_at,
          status: "aborted",
          memory_refs: clone(memory_refs),
          memory_revision: request.use_memory
            ? "mock-tree-revision"
            : undefined,
          execution_model: clone(execution_model),
          agent_harness,
          progress: clone(progress),
        });
        thread.updated_at = created_at;
      }
      observer({
        type: "aborted",
        run_id,
      });
    } else {
      progress.finishedAtMs = Date.now();
      for (const item of progress.items) {
        if (item.kind !== "commentary" && item.status === "running") {
          item.status = "failed";
          item.finishedAtMs = progress.finishedAtMs;
        }
      }
      if (thread) {
        thread.messages.push({
          id: message_id,
          role: "assistant",
          content: "Response could not be completed.",
          placeholder: true,
          created_at,
          status: "error",
          memory_revision: request.use_memory
            ? "mock-tree-revision"
            : undefined,
          execution_model: clone(execution_model),
          agent_harness,
          progress: clone(progress),
        });
        thread.updated_at = created_at;
      }
      observer({
        type: "error",
        failure_kind: "other",
        run_id,
        message: "Response could not be completed.",
        message_id,
        final_text: "Response could not be completed.",
        placeholder: true,
        created_at,
        execution_model: clone(execution_model),
        agent_harness: clone(agent_harness),
        progress: clone(progress),
      });
    }
  } finally {
    state.chat_runs.delete(run_id);
    finishRunActivity(state, run_id);
  }
};

export const chatAbort = async (state: MockBridgeState, run_id: string) => {
  const run = state.chat_runs.get(run_id);
  if (!run) throw new Error("Active Chat run not found.");
  run.abort();
};

function validateProfileUpdate(request: Omit<LoupistProfileUpdateRequest, "loupistId">) {
  if (request.name !== undefined && (!request.name.trim() || [...request.name.trim()].length > 60 || request.name.includes("\0"))) throw new Error("Name must contain 1–60 characters after trimming.");
  if (request.focus !== undefined && ([...request.focus].length > 280 || request.focus.includes("\0"))) throw new Error("Focus must contain at most 280 characters.");
  if (request.instructions !== undefined && ([...request.instructions].length > 4000 || request.instructions.includes("\0"))) throw new Error("Instructions must contain at most 4,000 characters.");
  if (request.avatar !== undefined && !LOUPIST_AVATAR_IDS.includes(request.avatar)) throw new Error("Choose an available avatar.");
  if (request.tone !== undefined && !LOUPIST_TONES.includes(request.tone)) throw new Error("Choose an available tone.");
}

export const loupistProfileUpdate = async (state: MockBridgeState, request: LoupistProfileUpdateRequest): Promise<LoupistSummary> => {
  validateProfileUpdate(request);
  const loupist = state.loupists.find(item => item.id === request.loupistId);
  if (!loupist) throw new Error("Loupist not found.");
  if (loupist.availability.status !== "available") throw new Error("Reconnect or recover this Loupist before editing its profile.");
  if (request.name !== undefined) loupist.name = request.name.trim();
  if (request.focus !== undefined) loupist.philosophy = request.focus;
  if (request.instructions !== undefined) loupist.instructions = request.instructions;
  if (request.avatar !== undefined) loupist.avatar = request.avatar;
  if (request.tone !== undefined) loupist.tone = request.tone;
  loupist.updated_at = new Date().toISOString();
  return clone(loupist);
};
