import { useSyncExternalStore } from "react";
import type { LoupistBridge } from "../../types";
import type { KnowledgeMapNode } from "../../contracts/knowledge";

type LearningState = { acknowledged: ReadonlyMap<string, number>; glow?: { recordId: string; until: number }; version: number };
type Store = { value: LearningState; listeners: Set<() => void> };
const sessions = new WeakMap<LoupistBridge, Map<string, Store>>();
function storeFor(bridge: LoupistBridge, loupistId: string): Store {
  let loupists = sessions.get(bridge);
  if (!loupists) { loupists = new Map(); sessions.set(bridge, loupists); }
  let store = loupists.get(loupistId);
  if (!store) { store = { value: { acknowledged: new Map(), version: 0 }, listeners: new Set() }; loupists.set(loupistId, store); }
  return store;
}
function update(store: Store, patch: Partial<LearningState>) {
  store.value = { ...store.value, ...patch, version: store.value.version + 1 };
  store.listeners.forEach(listener => listener());
}
export function useLearningState(bridge: LoupistBridge, loupistId: string) {
  const store = storeFor(bridge, loupistId);
  return useSyncExternalStore(callback => { store.listeners.add(callback); return () => { store.listeners.delete(callback); }; }, () => store.value);
}
export function acknowledgeMethod(bridge: LoupistBridge, loupistId: string, recordId: string, learnedAtMs: number) {
  const store = storeFor(bridge, loupistId);
  const acknowledged = new Map(store.value.acknowledged); acknowledged.set(recordId, learnedAtMs);
  update(store, { acknowledged });
}
export function glowLearnedMethod(bridge: LoupistBridge, loupistId: string, recordId: string) {
  const store = storeFor(bridge, loupistId);
  const glow = { recordId, until: Date.now() + 4000 };
  update(store, { glow });
  setTimeout(() => { if (store.value.glow === glow) update(store, { glow: undefined }); }, 4000);
}
export function isNewMethod(node: Pick<KnowledgeMapNode, "kind" | "recordId" | "learnedAtMs">, acknowledged: ReadonlyMap<string, number>, now = Date.now()) {
  return node.kind === "lens_method" && !!node.recordId && node.learnedAtMs !== undefined && node.learnedAtMs <= now && now - node.learnedAtMs < 7 * 86400000 && (acknowledged.get(node.recordId) ?? -Infinity) < node.learnedAtMs;
}
