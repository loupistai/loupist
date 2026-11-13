import type { ChatArtifactKind, ChatArtifactRef, ChatMessage, ChatProgressItem } from "../types";

type ToolItem = Extract<ChatProgressItem, { kind: "tool" | "system" }>;

/** `stepId` is the publish step the card belongs to, so the card stands where that step started. */
export type ChartBlock =
  | { kind: "ready"; key: string; artifact: ChatArtifactRef; updating: boolean; stepId?: string }
  | { kind: "building"; key: string; title: string; stepId: string }
  | { kind: "failed"; key: string; title: string; stepId: string };

/** A grid's reserved, filled or failed block takes the same states as a chart's. */
export type GridBlock = ChartBlock;

/** The host names each step's tool, so a chart step is told from a grid or document step exactly. */
const isPublish = (tool: string) => (item: ChatProgressItem): item is ToolItem =>
  item.kind === "tool" && item.toolName === tool;

const titleOf = (item: ToolItem) => item.target?.trim() ?? "";

/**
 * Cards of one artifact kind an answer shows inline, in a stable order:
 * published ones first (latest revision of each), then ones still being built
 * or that failed. A step that finished before its artifact event arrived stays
 * "building" while the reply streams, so the card never disappears and comes back.
 */
function artifactBlocks(message: ChatMessage, kind: ChatArtifactKind, tool: string, prefix: string): ChartBlock[] {
  if (message.role !== "assistant") return [];
  const latest = new Map<string, ChatArtifactRef>();
  for (const artifact of message.artifact_refs ?? []) {
    if (artifact.kind !== kind) continue;
    const current = latest.get(artifact.artifact_id);
    if (!current || artifact.revision >= current.revision) latest.set(artifact.artifact_id, artifact);
  }
  const ready = [...latest.values()];
  const readyTitles = new Set(ready.map((artifact) => artifact.title.trim()));
  const streaming = message.status === "streaming";
  const updatingTitles = new Set<string>();
  const pending: ChartBlock[] = [];
  const publishes = isPublish(tool);
  // The first step that published each title owns its card; a later step under the same title updates it.
  const stepOfTitle = new Map<string, string>();
  for (const item of message.progress?.items ?? []) {
    if (!publishes(item)) continue;
    const title = titleOf(item);
    const published = title !== "" && readyTitles.has(title);
    if (published && !stepOfTitle.has(title)) stepOfTitle.set(title, item.id);
    if (item.status === "running") {
      if (published) updatingTitles.add(title);
      else pending.push({ kind: "building", key: `building:${item.id}`, title, stepId: item.id });
    } else if (item.status === "succeeded" && streaming && !published) {
      pending.push({ kind: "building", key: `building:${item.id}`, title, stepId: item.id });
    } else if (item.status === "failed" && !published) {
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
    })),
    ...pending,
  ];
}

/** Charts an answer shows inline. */
export const chartBlocks = (message: ChatMessage): ChartBlock[] => artifactBlocks(message, "chart", "chart_publish", "chart");

/** Grid answers and conflict cards (`grid_publish`) an answer shows inline. */
export const gridBlocks = (message: ChatMessage): GridBlock[] => artifactBlocks(message, "grid", "grid_publish", "grid");
