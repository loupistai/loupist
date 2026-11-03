import type { ChatMessage, KnowledgeReference } from "../../types";
import type { GrowthEvent, KnowledgeMap } from "../../contracts/knowledge";
import { claimIdForEdge } from "../map/MapList";

/** Receipts identify this chat's jobs. Older receipts can only be followed through growth events. */
export function chatSourceWork(messages: readonly ChatMessage[]) {
  const jobIds = new Set<string>();
  const legacyMessages = new Set<string>();
  for (const message of messages) {
    if (message.role !== "assistant" || message.status === "streaming" || message.memory_update?.status !== "applied") continue;
    for (const change of message.memory_update.changes) {
      if (change.kind !== "Source" || change.operation === "proposed") continue;
      if (change.worldStatus === "completed") continue;
      if (change.worldJobId) jobIds.add(change.worldJobId);
      else if (!change.worldStatus) legacyMessages.add(message.id);
    }
  }
  return { jobIds, legacyMessages };
}

export function addedMapItems(map: KnowledgeMap, events: readonly GrowthEvent[]) {
  const nodeIds = new Set<string>();
  const edgeIds = new Set<string>();
  for (const event of events) {
    if (!event.subjectId) continue;
    if (event.kind === "entity_added") {
      if (map.nodes.some(node => node.kind === "entity" && node.id === event.subjectId)) nodeIds.add(event.subjectId);
    } else if (event.kind === "claim_added") {
      for (const edge of map.edges) if (edge.kind === "claim" && claimIdForEdge(edge) === event.subjectId) edgeIds.add(edge.id);
    }
  }
  const highlightIds = new Set(nodeIds);
  for (const edge of map.edges) if (edgeIds.has(edge.id)) { highlightIds.add(edge.source); highlightIds.add(edge.target); }
  return { nodeIds, edgeIds, highlightIds, count: nodeIds.size + edgeIds.size };
}

/** Only host-issued references and chat-attributed events seed the local graph. */
/** `liveIds`: names the running reply's steps read; the thread holds that reply only once it ends. */
export function chatMapData(map: KnowledgeMap, messages: readonly ChatMessage[], events: readonly GrowthEvent[], chatId: string, liveIds: readonly string[] = []) {
  const scopedEvents = events.filter(event => event.originChatId === chatId);
  const seeds = addedMapItems(map, scopedEvents).highlightIds;
  const includeReference = (reference?: KnowledgeReference) => {
    if (!reference) return;
    if (reference.kind === "world_node") seeds.add(reference.objectId);
    if (reference.kind === "world_claim") for (const edge of map.edges) {
      if (edge.kind === "claim" && claimIdForEdge(edge) === reference.objectId) { seeds.add(edge.source); seeds.add(edge.target); }
    }
  };
  for (const message of messages) {
    for (const ref of message.memory_refs ?? []) if (ref.access === "exact_read") includeReference(ref.knowledge);
    // World names a step read, attributed by the host; they stay on this chat's map after the run.
    for (const id of stepEntityIds(message)) if (map.nodes.some(node => node.kind === "entity" && node.id === id)) seeds.add(id);
    if (message.memory_update?.status === "applied") for (const change of message.memory_update.changes) {
      if (change.operation !== "proposed") includeReference(change.knowledge);
    }
  }
  for (const id of liveIds) if (map.nodes.some(node => node.kind === "entity" && node.id === id)) seeds.add(id);
  const ids = new Set(seeds);
  for (const edge of map.edges) if (seeds.has(edge.source) || seeds.has(edge.target)) { ids.add(edge.source); ids.add(edge.target); }
  const nodes = map.nodes.filter(node => node.kind === "entity" && ids.has(node.id));
  const shown = new Set(nodes.map(node => node.id));
  const edges = map.edges.filter(edge => shown.has(edge.source) && shown.has(edge.target));
  return { nodes, edges, events: scopedEvents };
}

/** Every World name the host attributed to this message's steps, in step order. */
export function stepEntityIds(message: ChatMessage): string[] {
  const ids: string[] = [];
  for (const item of message.progress?.items ?? []) {
    if (item.kind === "commentary") continue;
    for (const id of item.entityIds ?? []) if (!ids.includes(id)) ids.push(id);
  }
  return ids;
}
