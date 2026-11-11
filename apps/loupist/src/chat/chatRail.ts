import type { WorldReadinessStatus } from "../knowledge/worldReadiness";
import { knowledgePinKey } from "../knowledge/location";
import type { ChatMessage, ChatToolRefusal, KnowledgePin } from "../types";
import type { ChatApproval, ChatQuestion } from "../contracts/chat";
import type { GrowthEvent, KnowledgeMap } from "../contracts/knowledge";
import { addedMapItems } from "./chatMapData";
import { entityLabel } from "../knowledge/mapItems";
import { nodeColorKey } from "../knowledge/mapKinds";
import type { NameChip } from "./morningMessage";
import { chatReceipt } from "./receipt";
import { deliveredFiles } from "./deliveredFiles";

/** Leave a readable conversation beside the 300px context column and its outer gutter. */
const RAIL_MIN_WIDTH = 1120;
const MAX_LEARNED_CHIPS = 5;

export type RailMode = "rail" | "inline" | "none";

/** Keep context beside a wide conversation, or above it when a split leaves less room. */
export function railMode(width: number, hasCards: boolean): RailMode {
  if (!hasCards || width <= 0) return "none";
  if (width >= RAIL_MIN_WIDTH) return "rail";
  return "inline";
}

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
 * "In this chat" (context-rail.md R1): files and images, what the Loupist is
 * using, what it learned here, and what waits for the user, all from data the
 * chat already holds. Nothing here reads or writes beyond the chat's messages,
 * its growth events and the composer's pins.
 */
export function railData({ messages, pins = [], map, events = [], learningPending = false, learningWaiting, chatId, language, workspacePath }: {
  messages: readonly ChatMessage[];
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
      using.push({ key: `connection:${name}`, kind: "connection", label: name });
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
  const needs = (latest?.tool_refusals ?? []).filter((refusal) => refusal.reason === "needs_approval");
  const waiting = (latest?.progress?.approvals ?? []).filter((approval) => approval.state === "waiting");
  const questions = (latest?.progress?.questions ?? []).filter((question) => question.state === "waiting");
  return { files, using, learned, learningPending, learningWaiting, needs, waiting, questions, hasCards: files.length > 0 || using.length > 0 || learned !== null || learningPending || !!learningWaiting || needs.length > 0 || waiting.length > 0 || questions.length > 0 };
}
