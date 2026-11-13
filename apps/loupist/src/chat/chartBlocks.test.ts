import { describe, expect, it } from "vitest";
import type { ChatArtifactRef, ChatArtifactView, ChatMessage, ChatProgressItem } from "../types";
import { chartBlocks, sameBlock } from "./chartBlocks";

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
  it("tells a block derived again for the next word of the answer from one that draws a different card", () => {
    const streaming = (extra: Partial<ChatMessage>) => chartBlocks(message({ status: "streaming", progress: { startedAtMs: 0, items: [step("succeeded")], finishedAtMs: null }, ...extra }))[0]!;
    const filled = streaming({ artifact_refs: [chart()], content: "Margin" });
    expect(sameBlock(filled, streaming({ artifact_refs: [chart()], content: "Margin fell" }))).toBe(true);
    expect(sameBlock(filled, streaming({ artifact_refs: [chart({ revision: 2 })] }))).toBe(false);
    expect(sameBlock(filled, streaming({}))).toBe(false);
    expect(sameBlock(streaming({}), streaming({ content: "More" }))).toBe(true);
  });

  it("reserves a card while a chart is being published and fills it in place", () => {
    const running = chartBlocks(message({ status: "streaming", progress: { startedAtMs: 0, items: [step("running")], finishedAtMs: null } }));
    expect(running).toEqual([{ kind: "building", key: "building:step-running", title: "Margin vs utilization", stepId: "step-running" }]);
    const finishedBeforeEvent = chartBlocks(message({ status: "streaming", progress: { startedAtMs: 0, items: [step("succeeded")], finishedAtMs: null } }));
    expect(finishedBeforeEvent.map((block) => block.kind)).toEqual(["building"]);
    // The filled card keeps the step it belongs to, so it stays where that step started.
    const filled = chartBlocks(message({ status: "streaming", artifact_refs: [chart()], progress: { startedAtMs: 0, items: [step("succeeded")], finishedAtMs: null } }));
    expect(filled).toEqual([{ kind: "ready", key: "chart:chart-1", artifact: chart(), updating: false, stepId: "step-succeeded" }]);
    expect(chartBlocks(message({ artifact_refs: [chart()] }))).toEqual([{ kind: "ready", key: "chart:chart-1", artifact: chart(), updating: false, stepId: undefined }]);
  });

  it("fills a card from the host's staged chart while streaming, and drops it when the reply stops or fails", () => {
    const staged: ChatArtifactView = {
      artifact: { id: "chart-1", chat_session_id: "chat", kind: "chart", title: "Margin vs utilization", current_revision: 1, created_at_ms: 0, updated_at_ms: 0 },
      revision: { artifact_id: "chart-1", revision: 1, digest: "a".repeat(64), source_message_id: "answer", created_at_ms: 0,
        payload: { kind: "chart", schema: "loupist-chart/1", chart: {} as never } },
      chart_dataset: null,
    };
    const progress = { startedAtMs: 0, items: [step("succeeded")], finishedAtMs: null };
    const filled = { kind: "ready", key: "chart:chart-1", artifact: chart(), updating: false, stepId: "step-succeeded", staged };
    expect(chartBlocks(message({ status: "streaming", staged_artifacts: [staged], progress }))).toEqual([filled]);
    // The commit adds the stored reference; the same card then reads the stored revision.
    expect(chartBlocks(message({ status: "streaming", artifact_refs: [chart()], staged_artifacts: [staged], progress }))).toEqual([filled]);
    expect(chartBlocks(message({ status: "complete", artifact_refs: [chart()], staged_artifacts: [staged], progress }))).toEqual([
      { kind: "ready", key: "chart:chart-1", artifact: chart(), updating: false, stepId: "step-succeeded" },
    ]);
    // A stopped or failed reply saved nothing, so its staged card goes.
    expect(chartBlocks(message({ status: "aborted", staged_artifacts: [staged], progress }))).toEqual([]);
    expect(chartBlocks(message({ status: "error", staged_artifacts: [staged], progress }))).toEqual([]);
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
    expect(blocks).toEqual([{ kind: "ready", key: "chart:chart-1", artifact: chart({ revision: 2 }), updating: true, stepId: "step-running" }]);
  });

  it("shows a failed chart only when nothing was published under its title, and drops leftovers after the reply", () => {
    expect(chartBlocks(message({ progress: { startedAtMs: 0, items: [step("failed")], finishedAtMs: null } }))).toEqual([
      { kind: "failed", key: "failed:step-failed", title: "Margin vs utilization", stepId: "step-failed" },
    ]);
    expect(chartBlocks(message({ progress: { startedAtMs: 0, items: [step("succeeded")], finishedAtMs: null } }))).toEqual([]);
    expect(chartBlocks(message({ role: "user", artifact_refs: [chart()] }))).toEqual([]);
  });

  it("drops a failed card once the model publishes again in the same reply, and keeps it when nothing followed", () => {
    const failed = step("failed", { target: "Margins, first try" });
    const retry = (status: "running" | "succeeded") => step(status, { id: "retry" });
    const progress = (...items: ChatProgressItem[]) => ({ startedAtMs: 0, items, finishedAtMs: null });
    expect(chartBlocks(message({ status: "streaming", progress: progress(failed, retry("running")) })).map((block) => block.key)).toEqual(["building:retry"]);
    expect(chartBlocks(message({ artifact_refs: [chart()], progress: progress(failed, retry("succeeded")) })).map((block) => block.key)).toEqual(["chart:chart-1"]);
    // A retry that failed too leaves one failed card, the last one.
    expect(chartBlocks(message({ progress: progress(failed, step("failed", { id: "again" })) })).map((block) => block.key)).toEqual(["failed:again"]);
    // A later table does not stand in for a chart.
    expect(chartBlocks(message({ progress: progress(failed, step("succeeded", { id: "table", toolName: "grid_publish" })) })).map((block) => block.key)).toEqual(["failed:step-failed"]);
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
