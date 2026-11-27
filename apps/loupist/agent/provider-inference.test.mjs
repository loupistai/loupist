import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { ModelRuntime } from "@earendil-works/pi-coding-agent";
import { getSupportedThinkingLevels } from "@earendil-works/pi-ai";
import { getBuiltinModels } from "@earendil-works/pi-ai/providers/all";
import { runProviderInference, validateInferenceRequest } from "./provider-inference.mjs";

const model = getBuiltinModels("openai-codex").find((entry) => entry.id === "gpt-6-luna");
const request = () => ({ protocol: "loupist-provider/1", type: "inference", provider: model.provider, model: model.id, messages: [{ role: "system", content: "Extract claims from the supplied original." }, { role: "user", content: "The fictional firm builds chips." }], schema: { type: "object", properties: { claims: { type: "array", items: { type: "string" } } }, required: ["claims"], additionalProperties: false }, maxOutputTokens: 2048, timeoutMs: 1000, thinkingLevel: "max", runOptions: { performance: "standard" } });
function fixture({ output = '{"claims":["The fictional firm builds chips."]}', stopReason = "stop", repeat = false, available = true, message = {} } = {}) {
  const captured = { calls: 0, dispatches: 0 };
  return { captured, options: { onDispatch: () => { captured.dispatches++; }, fetch: async () => { captured.calls++; return new Response("bounded result"); } }, ctx: { modelRegistry: {
    getAvailable: () => available ? [model] : [],
    streamSimple(selected, context, options) {
      captured.context = context; captured.options = options; captured.model = selected;
      return { result: async () => {
        captured.payload = options.onPayload({ model: selected.id });
        await options.fetch("https://provider.example.test/response", {});
        if (repeat) await options.fetch("https://provider.example.test/response", {});
        return { provider: selected.provider, model: selected.id, stopReason, content: [{ type: "text", text: output }], usage: { input: 8, output: 4, totalTokens: 12, secretHeader: "must not escape" }, ...message };
      } };
    },
  } } };
}

test("isolated inference refuses catalog models with a foreign origin or headers", async () => {
  for (const overrides of [
    { baseUrl: "https://untrusted.example" },
    { headers: { "x-catalog-fixture": "untrusted" } },
  ]) {
    const f = fixture();
    f.ctx.modelRegistry.getAvailable = () => [{ ...model, ...overrides }];
    const result = await runProviderInference(f.ctx, request(), f.options);
    assert.equal(result.disposition, "not_sent");
    assert.equal(f.captured.context, undefined);
    assert.equal(f.captured.dispatches, 0);
    assert.equal(f.captured.calls, 0);
  }
});

test("one-shot inference uses exact model controls, isolated context and validated JSON", async () => {
  const f = fixture(); const result = await runProviderInference(f.ctx, request(), f.options);
  assert.equal(result.disposition, "completed");
  assert.deepEqual(result.body, { claims: ["The fictional firm builds chips."] });
  assert.deepEqual(result.usage, { input: 8, output: 4, totalTokens: 12 });
  assert.equal(f.captured.calls, 1); assert.equal(f.captured.dispatches, 1);
  assert.deepEqual(f.captured.context.tools, []); assert.equal(f.captured.context.messages.length, 1);
  assert.equal(f.captured.options.maxRetries, 0); assert.equal(f.captured.options.transport, "sse");
  assert.equal(f.captured.options.reasoning, "max"); assert.equal(f.captured.options.maxTokens, 2048);
  assert.equal(f.captured.payload.text.format.type, "json_schema"); assert.equal(f.captured.payload.store, false);
});

test("malformed, schema-invalid, truncated and hidden retry responses are uncertain without a second request", async () => {
  for (const args of [{ output: "not json" }, { output: '{"claims":[4]}' }, { output: '{"claims":[],"extra":1}' }, { stopReason: "length" }, { repeat: true }]) {
    const f = fixture(args); const result = await runProviderInference(f.ctx, request(), f.options);
    assert.equal(result.disposition, "uncertain"); assert.equal(f.captured.calls, 1);
    assert.equal(JSON.stringify(result).includes("secretHeader"), false);
  }
});

test("isolated inference forwards only finite nonnegative cost and billing provenance", async () => {
  for (const total of [0, 0.000123, -1, Infinity, "0.1"]) {
    const f = fixture({ message: { usage: { cost: { total, totalFromProvider: true, private: "discard" } } } });
    const result = await runProviderInference(f.ctx, request(), f.options);
    assert.equal(result.disposition, "completed");
    assert.deepEqual(result.usage.cost,
      Number.isFinite(total) && total >= 0 ? { total, totalFromProvider: true } : undefined);
  }
});

test("isolated inference preserves the distinction between unreported and invalid cache counts", async () => {
  for (const field of ["cacheRead", "cacheWrite"]) {
    for (const count of [null, undefined, -1, Number.MAX_SAFE_INTEGER + 1]) {
      const f = fixture({ message: { usage: { input: 500, output: 120, [field]: count } } });
      const result = await runProviderInference(f.ctx, request(), f.options);
      assert.equal(result.disposition, "completed");
      assert.deepEqual(result.usage, count == null ? { input: 500, output: 120 } : { output: 120 });
    }
  }
});

test("unavailable auth/model, unsupported controls and external schema references fail before dispatch", async () => {
  const absent = fixture({ available: false }); assert.equal((await runProviderInference(absent.ctx, request(), absent.options)).disposition, "not_sent"); assert.equal(absent.captured.calls, 0);
  for (const altered of [{ ...request(), thinkingLevel: "imaginary" }, { ...request(), runOptions: { endpoint: "https://wrong.test" } }, { ...request(), schema: { $ref: "https://outside.test/schema" } }, { ...request(), maxOutputTokens: 1000000 }]) {
    const f = fixture(); assert.equal((await runProviderInference(f.ctx, altered, f.options)).disposition, "not_sent"); assert.equal(f.captured.calls, 0);
  }
});

test("schema compiler handles Graphiti-style local definitions without coercing results", () => {
  const r = request(); r.schema = { type: "object", properties: { claims: { type: "array", items: { $ref: "#/$defs/claim" } } }, required: ["claims"], $defs: { claim: { type: "object", properties: { name: { type: "string" }, index: { type: "integer" } }, required: ["name", "index"] } } };
  const validator = validateInferenceRequest(r);
  assert.equal(validator.Check({ claims: [{ name: "A", index: 2 }] }), true);
  assert.equal(validator.Check({ claims: [{ name: "A", index: "2" }] }), false);
});

test("cancel before dispatch makes no provider request", async () => {
  const f = fixture(); const signal = AbortSignal.abort();
  assert.equal((await runProviderInference(f.ctx, request(), { ...f.options, signal })).disposition, "not_sent"); assert.equal(f.captured.calls, 0);
});

test("cancel after dispatch is uncertain and never repeats the request", async () => {
  const f = fixture(); const controller = new AbortController(); let calls = 0;
  const result = await runProviderInference(f.ctx, request(), {
    ...f.options, signal: controller.signal,
    fetch: async (_url, { signal }) => { calls++; controller.abort(); signal.throwIfAborted(); },
  });
  assert.equal(result.disposition, "uncertain"); assert.equal(calls, 1);
});

test("unbounded or tool-bearing output fails closed without publishing model content", async () => {
  const f = fixture({ output: JSON.stringify({ claims: ["x".repeat(256 * 1024)] }) });
  const result = await runProviderInference(f.ctx, request(), f.options);
  assert.equal(result.disposition, "uncertain"); assert.ok(JSON.stringify(result).length < 500);
});

test("inference diagnostics classify failures without retaining error or model content", async () => {
  const cases = [
    [{ output: "private response is not JSON" }, "json", "invalid_json", "stop"],
    [{ output: '{"claims":["private value"],"extra":"private field"}' }, "schema", "schema_mismatch", "stop"],
    [{ stopReason: "length" }, "response", "incomplete_response", "length"],
    [{ stopReason: "private stop reason" }, "response", "incomplete_response", "other"],
    [{ message: { model: "private wrong model" } }, "response", "identity_mismatch", "stop"],
    [{ message: { content: [{ type: "toolCall", name: "private tool" }] } }, "response", "content_type", "stop"],
    [{ output: "x".repeat(256 * 1024 + 1) }, "response", "output_size", "stop"],
    [{ repeat: true }, "provider", "retry_refused", null],
  ];
  for (const [args, phase, failure_kind, stop_reason] of cases) {
    const f = fixture(args); const events = [];
    const result = await runProviderInference(f.ctx, request(), { ...f.options, onDiagnostic: (event) => events.push(event) });
    assert.equal(result.disposition, "uncertain");
    assert.equal(f.captured.calls, 1); assert.equal(f.captured.dispatches, 1);
    assert.deepEqual(events, [{ type: "inference_diagnostic", phase, failure_kind, dispatched: true, cancelled: false, http_status: 200, stop_reason }]);
    assert.equal(JSON.stringify(events).includes("private"), false);
  }
  const f = fixture(); const events = [];
  const result = await runProviderInference(f.ctx, request(), {
    ...f.options, onDiagnostic: (event) => events.push(event),
    fetch: async () => { throw new Error("Authorization: Bearer private-token; https://private.example/secret"); },
  });
  assert.equal(result.disposition, "uncertain");
  assert.deepEqual(events, [{ type: "inference_diagnostic", phase: "provider", failure_kind: "provider_failure", dispatched: true, cancelled: false, http_status: null, stop_reason: null }]);
});

test("diagnostic observers do not change provider payload, controls, result, or dispatch count", async () => {
  const plain = fixture(); const observed = fixture(); const events = [];
  const expected = await runProviderInference(plain.ctx, request(), plain.options);
  const actual = await runProviderInference(observed.ctx, request(), { ...observed.options, onDiagnostic: (event) => { events.push(event); throw new Error("private observer failure"); } });
  assert.deepEqual(actual, expected);
  assert.deepEqual(observed.captured.payload, plain.captured.payload);
  const context = (value) => ({ ...value, messages: value.messages.map(({ timestamp, ...message }) => message) });
  assert.deepEqual(context(observed.captured.context), context(plain.captured.context));
  for (const key of ["reasoning", "maxTokens", "timeoutMs", "maxRetries", "transport", "cacheRetention"]) {
    assert.equal(observed.captured.options[key], plain.captured.options[key]);
  }
  assert.equal(observed.captured.calls, 1); assert.equal(observed.captured.dispatches, 1);
  assert.deepEqual(events, [{ type: "inference_diagnostic", phase: "complete", failure_kind: null, dispatched: true, cancelled: false, http_status: 200, stop_reason: "stop" }]);
  const asynchronous = fixture();
  assert.deepEqual(await runProviderInference(asynchronous.ctx, request(), { ...asynchronous.options, onDiagnostic: async () => { throw new Error("private observer rejection"); } }), expected);
});

test("inference diagnostics retain only HTTP status and cancellation state", async () => {
  const unavailable = fixture({ stopReason: "error", message: { errorMessage: "private provider response" } }); const statuses = [];
  const failed = await runProviderInference(unavailable.ctx, request(), {
    ...unavailable.options, onDiagnostic: (event) => statuses.push(event),
    fetch: async () => new Response("private body", { status: 503, headers: { authorization: "private header" } }),
  });
  assert.equal(failed.disposition, "uncertain");
  assert.equal(statuses[0].http_status, 503); assert.equal(statuses[0].stop_reason, "error");
  assert.equal(JSON.stringify(statuses).includes("private"), false);
  // The provider's text goes to Rust only, for its plan-limit/reconnect classification.
  assert.equal(failed.provider_error, "private provider response");
  assert.equal(JSON.stringify(failed.body).includes("private"), false);
  const limited = fixture({ stopReason: "error", message: { errorMessage: "x".repeat(20_000) } });
  assert.equal((await runProviderInference(limited.ctx, request(), limited.options)).provider_error.length, 8192);
  const cancelled = fixture(); const controller = new AbortController(); const cancellations = [];
  const result = await runProviderInference(cancelled.ctx, request(), {
    ...cancelled.options, signal: controller.signal, onDiagnostic: (event) => cancellations.push(event),
    fetch: async (_url, { signal }) => { controller.abort(); signal.throwIfAborted(); },
  });
  assert.equal(result.disposition, "uncertain");
  assert.equal(cancellations[0].failure_kind, "cancelled");
  assert.equal(cancellations[0].cancelled, true); assert.equal(cancellations[0].dispatched, true);
});

test("a failed inference keeps reported usage and names every failure kind", async () => {
  const reported = { input: 11, output: 7, cacheRead: 3, cacheWrite: 1, totalTokens: 22, cost: { total: 0.000002, totalFromProvider: true } };
  const kept = { input: 11, output: 7, cacheRead: 3, cacheWrite: 1, totalTokens: 22, cost: { total: 0.000002, totalFromProvider: true } };
  const afterMessage = [
    ["invalid_json", { output: "not json" }],
    ["schema_mismatch", { output: '{"claims":[4]}' }],
    ["incomplete_response", { stopReason: "length" }],
    ["content_type", { message: { content: [{ type: "toolCall", name: "private" }] } }],
    ["output_size", { output: "x".repeat(256 * 1024 + 1) }],
    ["identity_mismatch", { message: { model: "other-model" } }],
  ];
  for (const [failure_kind, args] of afterMessage) {
    for (const usage of [reported, null]) {
      const f = fixture({ ...args, message: { ...(args.message ?? {}), usage } });
      const result = await runProviderInference(f.ctx, request(), f.options);
      assert.equal(result.disposition, "uncertain", failure_kind);
      assert.equal(result.failure_kind, failure_kind);
      assert.equal(result.body.failure_kind, failure_kind);
      assert.equal(result.body.status, "uncertain");
      assert.deepEqual(result.usage, usage ? kept : null, failure_kind);
      assert.equal(JSON.stringify(result).includes("private"), false);
    }
  }
  const beforeMessage = [
    ["retry_refused", (f) => fixture({ ...f, repeat: true })],
    ["provider_failure", (f) => ({ ...f, options: { ...f.options, fetch: async () => { throw new Error("Authorization: Bearer private"); } } })],
    ["cancelled", (f) => {
      const controller = new AbortController();
      return { ...f, options: { ...f.options, signal: controller.signal, fetch: async (_url, { signal }) => { controller.abort(); signal.throwIfAborted(); } } };
    }],
    ["dispatch_marker_failed", (f) => ({ ...f, options: { ...f.options, onDispatch: () => { throw new Error("private marker"); } } })],
    ["payload_invalid", (f) => ({
      ...f,
      ctx: {
        modelRegistry: {
          ...f.ctx.modelRegistry,
          streamSimple(selected, _context, options) {
            return { result: async () => { options.onPayload(null); return { provider: selected.provider, model: selected.id, stopReason: "stop", content: [{ type: "text", text: "{}" }] }; } };
          },
        },
      },
    })],
  ];
  for (const [failure_kind, prepare] of beforeMessage) {
    const f = prepare(fixture());
    const result = await runProviderInference(f.ctx, request(), f.options);
    const dispatched = failure_kind === "retry_refused" || failure_kind === "provider_failure" || failure_kind === "cancelled";
    assert.equal(result.disposition, dispatched ? "uncertain" : "not_sent", failure_kind);
    assert.equal(result.failure_kind, failure_kind);
    assert.equal(result.body.failure_kind, failure_kind);
    assert.equal(result.usage, null, failure_kind);
    assert.equal(JSON.stringify(result).includes("private"), false);
  }
});

test("one Markdown fence is removed only when the API is not given a JSON schema", async () => {
  const fenced = "```json\n{\"claims\":[\"The fictional firm builds chips.\"]}\n```";
  const schemaApi = fixture({ output: fenced });
  const refused = await runProviderInference(schemaApi.ctx, request(), schemaApi.options);
  assert.equal(refused.disposition, "uncertain");
  assert.equal(refused.failure_kind, "invalid_json");
  const plain = getBuiltinModels("anthropic").find((entry) => entry.api === "anthropic-messages");
  assert.ok(plain);
  const level = getSupportedThinkingLevels(plain)[0];
  const open = fixture({ output: fenced });
  open.ctx.modelRegistry.getAvailable = () => [plain];
  const asked = request();
  asked.provider = plain.provider;
  asked.model = plain.id;
  asked.thinkingLevel = level;
  asked.runOptions = {};
  const result = await runProviderInference(open.ctx, asked, open.options);
  assert.equal(result.disposition, "completed");
  assert.deepEqual(result.body, { claims: ["The fictional firm builds chips."] });
});

test("separate Pi runtimes share one locked OAuth refresh and preserve other credentials", async () => {
  const directory = mkdtempSync(join(tmpdir(), "loupist-inference-refresh-")); const path = join(directory, "auth.json");
  const providerId = "inference-refresh-fixture"; let refreshes = 0;
  writeFileSync(path, JSON.stringify({ [providerId]: { type: "oauth", access: "expired-fixture", refresh: "fixture-refresh", expires: 1 }, unrelated: { type: "api_key", key: "other-fixture" } }), { mode: 0o600 });
  try {
    const provider = { id: providerId, name: "Refresh fixture", auth: { oauth: { refresh: async (credential) => { refreshes++; await new Promise((resolve) => setTimeout(resolve, 30)); return { ...credential, access: "renewed-fixture", expires: Date.now() + 3600000 }; }, toAuth: async (credential) => ({ apiKey: credential.access }) } }, getModels: () => [] };
    const a = await ModelRuntime.create({ authPath: path, modelsPath: null, refreshOnCreate: false });
    const b = await ModelRuntime.create({ authPath: path, modelsPath: null, refreshOnCreate: false });
    a.registerNativeProvider(provider); b.registerNativeProvider(provider);
    const values = await Promise.all([a.getAuth(providerId), b.getAuth(providerId)]);
    assert.equal(refreshes, 1); assert.equal(values[0].auth.apiKey, "renewed-fixture"); assert.equal(values[1].auth.apiKey, "renewed-fixture");
    assert.equal(JSON.parse(readFileSync(path, "utf8")).unrelated.key, "other-fixture");
  } finally { rmSync(directory, { recursive: true, force: true }); }
});
