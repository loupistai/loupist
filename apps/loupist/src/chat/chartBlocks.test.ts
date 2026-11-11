import { describe, expect, it } from "vitest";
import type { ChatArtifactRef, ChatMessage, ChatProgressItem } from "../types";
import { chartBlocks } from "./chartBlocks";

const chart = (overrides: Partial<ChatArtifactRef> = {}): ChatArtifactRef => ({
  artifact_id: "chart-1", revision: 1, kind: "chart", title: "Margin vs utilization", digest: "a".repeat(64), ...overrides,
});
const step = (status: "running" | "succeeded" | "failed", overrides: Partial<Extract<ChatProgressItem, { kind: "tool" | "system" }>> = {}): ChatProgressItem => ({
  id: `step-${status}`, kind: "tool", toolName: "chart_publish", category: "artifact", operation: "publish",
  action: "Made a chart", running: "Making a chart", target: "Margin vs utilization", href: null, status, startedAtMs: 0, finishedAtMs: null, ...overrides,
});
const message = (overrides: Partial<ChatMessage>): ChatMessage => ({
  id: "answer", role: "assistant", content: "", created_at: "2026-10-28T00:00:00Z", status: "complete", ...overrides,
});

describe("chartBlocks", () => {
  it("reserves a card while a chart is being published and fills it in place", () => {
    const running = chartBlocks(message({ status: "streaming", progress: { startedAtMs: 0, items: [step("running")], finishedAtMs: null } }));
    expect(running).toEqual([{ kind: "building", key: "building:step-running", title: "Margin vs utilization" }]);
    const finishedBeforeEvent = chartBlocks(message({ status: "streaming", progress: { startedAtMs: 0, items: [step("succeeded")], finishedAtMs: null } }));
    expect(finishedBeforeEvent.map((block) => block.kind)).toEqual(["building"]);
    const filled = chartBlocks(message({ status: "streaming", artifact_refs: [chart()], progress: { startedAtMs: 0, items: [step("succeeded")], finishedAtMs: null } }));
    expect(filled).toEqual([{ kind: "ready", key: "chart:chart-1", artifact: chart(), updating: false }]);
  });

  it("ignores documents, keeps the latest chart revision, and marks a chart being revised", () => {
    const document = chart({ artifact_id: "doc", kind: "markdown", title: "Research note" });
    const blocks = chartBlocks(message({
      status: "streaming",
      artifact_refs: [chart(), chart({ revision: 2 }), document],
      progress: { startedAtMs: 0, items: [
        step("running"),
        step("running", { id: "doc-step", toolName: "artifact_publish", action: "Saved a Chat document", running: "Saving a Chat document", target: "Research note" }),
      ], finishedAtMs: null },
    }));
    expect(blocks).toEqual([{ kind: "ready", key: "chart:chart-1", artifact: chart({ revision: 2 }), updating: true }]);
  });

  it("shows a failed chart only when nothing was published under its title, and drops leftovers after the reply", () => {
    expect(chartBlocks(message({ progress: { startedAtMs: 0, items: [step("failed")], finishedAtMs: null } }))).toEqual([
      { kind: "failed", key: "failed:step-failed", title: "Margin vs utilization" },
    ]);
    expect(chartBlocks(message({ progress: { startedAtMs: 0, items: [step("succeeded")], finishedAtMs: null } }))).toEqual([]);
    expect(chartBlocks(message({ role: "user", artifact_refs: [chart()] }))).toEqual([]);
  });

  it("recognizes a chart step by its tool name, not by its wording", () => {
    // Another tool with the chart's wording is not a chart step, and a chart step reads as one in any wording.
    const lookalike = step("running", { id: "lookalike", toolName: "grid_publish" });
    const reworded = step("running", { id: "reworded", action: "차트 만듦", running: "차트 만드는 중" });
    const blocks = chartBlocks(message({ status: "streaming", progress: { startedAtMs: 0, items: [lookalike, reworded], finishedAtMs: null } }));
    expect(blocks.map((block) => block.key)).toEqual(["building:reworded"]);
    // A step without a tool name (recorded before the host named tools) is not guessed from its label.
    expect(chartBlocks(message({ status: "streaming", progress: { startedAtMs: 0, items: [step("running", { toolName: undefined })], finishedAtMs: null } }))).toEqual([]);
  });
});
