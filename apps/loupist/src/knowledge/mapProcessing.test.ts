import type { GraphProcessingView, KnowledgeCatalog } from "../types";
import { sourceDisplayStatus, sourceProcessingLabel, worldProcessing } from "./mapProcessing";

function catalog(statuses: readonly string[]): KnowledgeCatalog {
  return {
    loupistId: "test",
    sources: [{ recordId: "s", revisionId: "r", kind: "source", title: "Sources", revisionDigest: "digest", readOnly: true, indexingState: "snapshot_only", objects: statuses.map((status, index) => ({
      id: String(index), title: String(index),
      processing: { status, jobId: String(index), error: null },
    })) }],
    worlds: [],
    lenses: [],
  };
}

type Readiness = GraphProcessingView["readiness"];
const ready = { status: "ready" } as const;
const disconnected = { status: "provider_disconnected" } as const;

it.each([
  { statuses: [], readiness: disconnected, expected: { kind: "idle" }, pending: false },
  { statuses: ["queued"], readiness: ready, expected: { kind: "building", sourceCount: 1 }, pending: true },
  { statuses: ["queued"], readiness: null, expected: { kind: "idle" }, pending: true },
  { statuses: ["queued"], readiness: disconnected, expected: { kind: "waiting", reason: "provider_disconnected" }, pending: true },
  { statuses: ["queued"], readiness: { status: "model_required", note: "awaiting_provider" }, expected: { kind: "waiting", reason: "awaiting_provider" }, pending: true },
  { statuses: ["queued"], readiness: { status: "model_required" }, expected: { kind: "waiting", reason: "model_required" }, pending: true },
  { statuses: ["queued"], readiness: { status: "disabled" }, expected: { kind: "waiting", reason: "disabled" }, pending: true },
  { statuses: ["paused"], readiness: { status: "paused" }, expected: { kind: "waiting", reason: "paused" }, pending: false },
  { statuses: ["queued"], readiness: { status: "runtime_unavailable" }, expected: { kind: "waiting", reason: "runtime_unavailable" }, pending: true },
  { statuses: ["queued"], readiness: { status: "model_unavailable" }, expected: { kind: "waiting", reason: "model_unavailable" }, pending: true },
  // With no AI account at all, the host's note decides the reason, whatever the model status.
  { statuses: ["queued"], readiness: { status: "model_unavailable", note: "no_account" }, expected: { kind: "waiting", reason: "no_account" }, pending: true },
  { statuses: ["queued"], readiness: { status: "provider_disconnected", note: "no_account" }, expected: { kind: "waiting", reason: "no_account" }, pending: true },
  { statuses: ["queued"], readiness: { status: "model_unavailable", note: "model_unavailable" }, expected: { kind: "waiting", reason: "model_unavailable" }, pending: true },
  { statuses: ["queued"], readiness: { status: "reconciliation_required" }, expected: { kind: "waiting", reason: "reconciliation_required" }, pending: true },
  { statuses: ["paused_budget"], readiness: ready, expected: { kind: "waiting", reason: "paused_budget" }, pending: false },
  { statuses: ["failed_before_mutation"], readiness: ready, expected: { kind: "waiting", reason: "failed_before_mutation" }, pending: false },
  { statuses: ["reconcile_required"], readiness: ready, expected: { kind: "waiting", reason: "reconcile_required" }, pending: false },
  { statuses: ["running", "queued"], readiness: disconnected, expected: { kind: "building", sourceCount: 1 }, pending: true },
  { statuses: ["running", "queued"], readiness: ready, expected: { kind: "building", sourceCount: 2 }, pending: true },
  { statuses: [], readiness: { status: "recovering" }, expected: { kind: "building", sourceCount: 0 }, pending: true },
] satisfies { statuses: string[]; readiness: Readiness | null; expected: object; pending: boolean }[])(
  "derives activity and polling from $statuses and $readiness",
  ({ statuses, readiness, expected, pending }) => {
    expect(worldProcessing(catalog(statuses), readiness)).toEqual({ activity: expected, readiness, pending });
  },
);

it("preserves running and stopped Sources and reflects when a paused queue is switched off", () => {
  expect(sourceDisplayStatus("queued", disconnected)).toBe("provider_disconnected");
  expect(sourceDisplayStatus("running", disconnected)).toBe("running");
  expect(sourceDisplayStatus("paused_budget", disconnected)).toBe("paused_budget");
  expect(sourceDisplayStatus("paused", { status: "disabled" })).toBe("disabled");
  expect(sourceDisplayStatus("queued", { status: "model_unavailable", note: "no_account" })).toBe("no_account");
});

it("words a budget pause and a missing account where the status is shown", () => {
  expect(sourceProcessingLabel("paused_budget")).toBe("World building paused for this source: it reached the request allowance set in World building settings.");
  expect(sourceProcessingLabel("no_account")).toBe("World is waiting for an AI account. Connect one in Settings > AI accounts.");
  expect(sourceProcessingLabel("model_unavailable")).toBe("Choose another AI model");
});
