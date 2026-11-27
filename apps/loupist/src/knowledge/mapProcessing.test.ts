import type { GraphProcessingView, KnowledgeCatalog } from "../types";
import { sourceDisplayStatus, sourceHold, sourceHoldLine, sourceProcessingLabel, sourceRetryOffered, stoppedSourceLine, stoppedSources, waitingLabel, worldProcessing, type StoppedSource } from "./mapProcessing";

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
  { statuses: ["paused_budget"], readiness: ready, expected: { kind: "waiting", reason: "paused_budget", stopped: ["s"] }, pending: false },
  // Price data never enters the queue: World stays idle and counts nothing as building.
  { statuses: ["price_data"], readiness: ready, expected: { kind: "idle" }, pending: false },
  { statuses: ["price_data", "running"], readiness: ready, expected: { kind: "building", sourceCount: 1 }, pending: true },
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
  expect(sourceProcessingLabel("price_data")).toBe("Price data · not added to World");
});

it("counts the Sources stopped at the request limit by record and says so on World's own lines", () => {
  const records = (statuses: (string | undefined)[][]): KnowledgeCatalog => ({
    ...catalog([]),
    sources: statuses.map((objects, index) => ({ ...catalog([]).sources[0]!, recordId: `source-${index}`,
      objects: objects.map((status, object) => ({ id: `${index}-${object}`, title: "", processing: status ? { status, jobId: null, error: null } : undefined })) })),
  });
  const several = records([["paused_budget"], ["paused_budget", "completed"], ["queued"], ["paused"], [undefined], ["paused_budget"]]);
  expect(stoppedSources(several, ready)).toEqual(["source-0", "source-1", "source-5"]);
  expect(waitingLabel({ kind: "waiting", reason: "paused_budget", stopped: stoppedSources(several, ready) })).toBe("3 Sources paused at the request limit");
  expect(waitingLabel({ kind: "waiting", reason: "paused_budget", stopped: ["source-0"] })).toBe("1 Source paused at the request limit");
  // Another reason keeps its own words; the reader and each row keep the Source's sentence.
  expect(waitingLabel({ kind: "waiting", reason: "failed_before_mutation" })).toBe("World building failed");
  expect(sourceProcessingLabel("paused_budget")).toMatch(/^World building paused for this source/);
  // After partial completion only the Sources still stopped are counted.
  const after = records([["completed"], ["paused_budget", "completed"], ["running"], ["paused"], [undefined], ["queued"]]);
  expect(worldProcessing(after, ready).activity).toEqual({ kind: "building", sourceCount: 2 });
  expect(stoppedSources(after, ready)).toEqual(["source-1"]);
});

it.each([
  ["invalid_json", "The model's reply could not be used.", true],
  ["schema_mismatch", "The model's reply could not be used.", true],
  ["incomplete_response", "The model's reply could not be used.", true],
  ["output_size", "The model's reply could not be used.", true],
  ["content_type", "The model's reply could not be used.", true],
  ["provider_error", "The AI provider returned an error.", true],
  ["world_full", "World is full. This Source is kept but not read into World.", false],
  ["interrupted", "World building was interrupted", true],
] as const)("says why a Source stopped for %s and offers Try again only when it can run again", (kind, line, retry) => {
  const model = kind === "invalid_json" || kind === "schema_mismatch" || kind === "incomplete_response" || kind === "output_size" || kind === "content_type";
  const stop = { status: kind === "world_full" ? "paused_budget" : model ? "failed_extraction" : kind, failure: { kind }, retryable: true };
  expect(stoppedSourceLine(stop)).toBe(line);
  expect(sourceRetryOffered(stop)).toBe(retry);
  expect(sourceRetryOffered({ ...stop, retryable: undefined })).toBe(false);
});

it("keeps the status sentence when a stop has no recorded failure", () => {
  expect(stoppedSourceLine({ status: "paused_budget", retryable: true })).toBe("World building paused for this source: it reached the request allowance set in World building settings.");
  expect(sourceRetryOffered({ status: "paused_budget", retryable: true })).toBe(true);
  expect(sourceRetryOffered({ status: "paused_budget" })).toBe(false);
  expect(sourceRetryOffered({ status: "interrupted" })).toBe(false);
  expect(stoppedSourceLine({ status: "failed_extraction" })).toBe("The model's reply could not be used.");
});

it("names a Source waiting on an allowance or a plan limit", () => {
  expect(sourceHold({ status: "queued", hold: { cause: "allowance" } } as StoppedSource)).toBe("allowance");
  expect(sourceHold({ status: "queued", hold: { cause: "plan_limit", provider: "openai" } } as StoppedSource)).toBe("plan_limit");
  expect(sourceHold({ status: "queued", hold: { cause: "out_of_credit", provider: "openai" } } as StoppedSource)).toBe("out_of_credit");
  expect(sourceHold({ status: "paused", hold: "daily_allowance" } as StoppedSource)).toBe("daily_allowance");
  expect(sourceHold({ status: "out_of_credit" })).toBe("out_of_credit");
  expect(sourceHold({ status: "paused_budget", retryable: true })).toBeUndefined();
  expect(sourceHoldLine("allowance")).toBe("Waiting for tomorrow's allowance.");
  expect(sourceHoldLine("plan_limit")).toBe("Your AI plan's limit was reached. Reading resumes tomorrow.");
  expect(sourceHoldLine("out_of_credit")).toBe("Your AI plan's limit was reached. Reading resumes tomorrow.");
});
