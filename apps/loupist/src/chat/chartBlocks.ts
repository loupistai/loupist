import type { ChatArtifactKind, ChatArtifactRef, ChatArtifactView, ChatMessage, ChatProgressItem } from "../types";

type ToolItem = Extract<ChatProgressItem, { kind: "tool" | "system" }>;

/**
 * `stepId` is the publish step the card belongs to, so the card stands where that step started.
 * `staged` is the content the host delivered before the reply commits: the card draws it
 * instead of reading the store, which has nothing until the commit.
 */
export type ChartBlock =
  | { kind: "ready"; key: string; artifact: ChatArtifactRef; updating: boolean; stepId?: string; staged?: ChatArtifactView }
  | { kind: "building"; key: string; title: string; stepId: string }
  | { kind: "failed"; key: string; title: string; stepId: string };

/** A grid's reserved, filled or failed block takes the same states as a chart's. */
export type GridBlock = ChartBlock;

/**
 * Two derivations of one block that draw the same card. Blocks are derived
 * again on every update of a streaming reply; a card compares them by value so
 * a table already filled is not redrawn for each word of the answer.
 */
export function sameBlock(left: ChartBlock, right: ChartBlock): boolean {
  if (left === right) return true;
  if (left.key !== right.key || left.kind !== right.kind || left.stepId !== right.stepId) return false;
  if (left.kind !== "ready" || right.kind !== "ready") return left.kind !== "ready" && right.kind !== "ready" && left.title === right.title;
  const [a, b] = [left.artifact, right.artifact];
  return left.updating === right.updating && left.staged === right.staged && a.artifact_id === b.artifact_id && a.revision === b.revision && a.digest === b.digest && a.title === b.title && a.kind === b.kind;
}

/** The host names each step's tool, so a chart step is told from a grid or document step exactly. */
const isPublish = (tool: string) => (item: ChatProgressItem): item is ToolItem =>
  item.kind === "tool" && item.toolName === tool;

const titleOf = (item: ToolItem) => item.target?.trim() ?? "";

const stagedRef = (view: ChatArtifactView): ChatArtifactRef => ({
  artifact_id: view.artifact.id,
  revision: view.revision.revision,
  kind: view.revision.payload.kind,
  title: view.artifact.title,
  digest: view.revision.digest,
});

/** The staged content of exactly this revision, if the host delivered it. */
const stagedFor = (staged: Map<string, ChatArtifactView>, artifact: ChatArtifactRef): { staged?: ChatArtifactView } => {
  const view = staged.get(`${artifact.artifact_id}:${artifact.revision}`);
  return view && view.revision.digest === artifact.digest ? { staged: view } : {};
};

/**
 * Cards of one artifact kind an answer shows inline, in a stable order:
 * published ones first (latest revision of each), then ones still being built
 * or that failed. While the reply streams, a card the host delivered before the
 * commit counts as published, so it fills as its step succeeds; a stopped or
 * failed reply saves nothing, so its staged cards go. Once the reply commits
 * the card reads the stored revision (keeping what it draws meanwhile), so it
 * learns of a later revision as any stored card does. A step that finished
 * before either arrived stays "building" while the reply streams, so the card
 * never disappears and comes back.
 */
function artifactBlocks(message: ChatMessage, kind: ChatArtifactKind, tool: string, prefix: string): ChartBlock[] {
  if (message.role !== "assistant") return [];
  const streaming = message.status === "streaming";
  const staged = new Map<string, ChatArtifactView>();
  if (streaming) {
    for (const view of message.staged_artifacts ?? []) staged.set(`${view.artifact.id}:${view.revision.revision}`, view);
  }
  const latest = new Map<string, ChatArtifactRef>();
  const refs = [...(message.artifact_refs ?? []), ...[...staged.values()].map(stagedRef)];
  for (const artifact of refs) {
    if (artifact.kind !== kind) continue;
    const current = latest.get(artifact.artifact_id);
    if (!current || artifact.revision >= current.revision) latest.set(artifact.artifact_id, artifact);
  }
  const ready = [...latest.values()];
  const readyTitles = new Set(ready.map((artifact) => artifact.title.trim()));
  const updatingTitles = new Set<string>();
  const pending: ChartBlock[] = [];
  const publishes = isPublish(tool);
  // The first step that published each title owns its card; a later step under the same title updates it.
  const stepOfTitle = new Map<string, string>();
  const steps = (message.progress?.items ?? []).filter(publishes);
  // A failed call the model then made again (under any title, however it went) is a retry: the
  // retry's card stands for it, so at most the last attempt shows; the ledger keeps every failed step.
  const retried = (index: number) => index < steps.length - 1;
  for (const [index, item] of steps.entries()) {
    const title = titleOf(item);
    const published = title !== "" && readyTitles.has(title);
    if (published && !stepOfTitle.has(title)) stepOfTitle.set(title, item.id);
    if (item.status === "running") {
      if (published) updatingTitles.add(title);
      else pending.push({ kind: "building", key: `building:${item.id}`, title, stepId: item.id });
    } else if (item.status === "succeeded" && streaming && !published) {
      pending.push({ kind: "building", key: `building:${item.id}`, title, stepId: item.id });
    } else if (item.status === "failed" && !published && !retried(index)) {
      pending.push({ kind: "failed", key: `failed:${item.id}`, title, stepId: item.id });
    }
  }
  return [
    ...ready.map((artifact): ChartBlock => ({
      kind: "ready",
      key: `${prefix}:${artifact.artifact_id}`,
      artifact,
      updating: updatingTitles.has(artifact.title.trim()),
      stepId: stepOfTitle.get(artifact.title.trim()),
      ...stagedFor(staged, artifact),
    })),
    ...pending,
  ];
}

/** Charts an answer shows inline. */
export const chartBlocks = (message: ChatMessage): ChartBlock[] => artifactBlocks(message, "chart", "chart_publish", "chart");

/** Grid answers and conflict cards (`grid_publish`) an answer shows inline. */
export const gridBlocks = (message: ChatMessage): GridBlock[] => artifactBlocks(message, "grid", "grid_publish", "grid");
