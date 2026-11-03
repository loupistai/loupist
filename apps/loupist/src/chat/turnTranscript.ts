import { citationSources } from "./evidence";
import type { ChatEvidenceRef, ChatPlan, ChatProgress, ChatProgressItem } from "../types";

/**
 * Steps that started while a plan item was active nest under that item, in
 * activation order; steps before the first activation and commentary stay in
 * the flat list below the plan.
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

type Work = {
  kind: "work";
  key: string;
  progress: ChatProgress;
  evidence: ChatEvidenceRef[];
  sources: ChatEvidenceRef[];
};
type Part = Work | Extract<ChatProgressItem, { kind: "commentary" }>;

/** Keep public speech in reading order. Each receipt and approval belongs to one disclosure. */
export function turnTranscript(progress: ChatProgress, results: ChatEvidenceRef[], hasMemory: boolean, evidenceRefs: readonly ChatEvidenceRef[] = []): Part[] {
  const parts: Part[] = [];
  const owners = new Map<string, Work>();
  const resultOwners = new Map<string, Work>();
  function work(key: string): Work {
    const group: Work = { kind: "work", key, progress: {
      startedAtMs: progress.startedAtMs, finishedAtMs: progress.finishedAtMs, items: [],
    }, evidence: [], sources: [] };
    parts.push(group);
    return group;
  }
  function add(group: Work, item: ChatProgressItem) {
    group.progress.items.push(item);
    owners.set(item.id, group);
    if (item.kind === "tool") for (const id of item.resultIds ?? []) resultOwners.set(id, group);
  }

  let first: Work | undefined;
  let group: Work | undefined;
  for (const item of progress.items) {
    if (item.kind === "commentary") {
      parts.push(item);
      group = undefined;
    } else {
      group ??= work(first ? `step-${item.id}` : "activity");
      first ??= group;
      add(group, item);
    }
  }
  const fallback = () => first ??= work("activity");
  if (progress.plan || hasMemory) fallback().progress.plan = progress.plan;
  for (const result of results) (resultOwners.get(result.id) ?? fallback()).evidence.push(result);
  for (const approval of progress.approvals ?? []) {
    const group = (approval.itemId && owners.get(approval.itemId)) || fallback();
    (group.progress.approvals ??= []).push(approval);
  }
  for (const source of evidenceRefs) resultOwners.get(source.id)?.sources.push(source);
  for (const part of parts) {
    if (part.kind === "work") part.sources = citationSources(part.sources);
  }
  return parts;
}
