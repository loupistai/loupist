import { createMockBridgeState } from "./state";
import { knowledgeGraphJob, knowledgeGraphRetrySource, knowledgeGraphSettingsGet } from "./knowledge";
import type { GraphJobView, GraphProcessingSettings, SourceDetail } from "../../contracts/knowledge";

it("reads the user's pause separately and retries a stopped Source only on explicit intent", async () => {
  const state = createMockBridgeState(0);
  const loupistId = "loupist-quality";
  await knowledgeGraphSettingsGet(state, { loupistId });
  const store = state.knowledgeStores[loupistId] as { settings: GraphProcessingSettings; jobs: GraphJobView[]; sources: SourceDetail[] };
  const source = store.sources[0]!;
  const job: GraphJobView = { jobId: "stopped", loupistId, worldRecordId: "world", worldRevisionId: "revision", status: "paused_budget", payloadDigest: "digest", createdAtMs: 1, hostMayRedrive: false };
  store.jobs.push(job);
  source.processing = { jobId: job.jobId, status: "paused_budget", error: null };
  store.settings.enrichmentEnabled = true;
  store.settings.helperModelProfileId = "fixture";
  store.settings.paused = false;
  const read = () => knowledgeGraphJob(state, { loupistId, jobId: job.jobId });
  const retry = () => knowledgeGraphRetrySource(state, { loupistId, recordId: source.recordId });
  expect((await read()).status).toBe("paused_budget");
  store.settings.paused = true;
  expect((await read()).status).toBe("paused");
  await expect(retry()).rejects.toThrow();
  store.settings.paused = false;
  expect((await retry()).status).toBe("queued");
  expect(source.processing.status).toBe("queued");
  await expect(retry()).rejects.toThrow();
  job.status = "reconcile_required";
  await expect(retry()).rejects.toThrow();
});
