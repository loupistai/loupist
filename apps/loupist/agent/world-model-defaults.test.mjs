import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { zstdDecompressSync } from "node:zlib";
import { getBuiltinModel, builtinProviders } from "@earendil-works/pi-ai/providers/all";
import { runProviderInference } from "./provider-inference.mjs";
import { runControlsFor } from "./model-run-controls.mjs";

const defaults = JSON.parse(readFileSync(new URL("../world-model-defaults.json", import.meta.url)));
const providers = builtinProviders();
const answer = { claims: [{ name: "Acme builds chips.", index: 0 }] };
const schema = { type: "object", properties: { claims: { type: "array", items: { $ref: "#/$defs/claim" } } }, required: ["claims"], additionalProperties: false, $defs: { claim: { type: "object", properties: { name: { type: "string" }, index: { type: "integer" } }, required: ["name", "index"] } } };
const event = (value) => `event: ${value.type}\ndata: ${JSON.stringify(value)}\n\n`;
function responseFor(model) {
  const text = JSON.stringify(answer);
  let body;
  if (model.api === "anthropic-messages") {
    body = [
      { type: "message_start", message: { id: "fixture", model: model.id, usage: { input_tokens: 10, output_tokens: 0 } } },
      { type: "content_block_start", index: 0, content_block: { type: "text", text: "" } },
      { type: "content_block_delta", index: 0, delta: { type: "text_delta", text } },
      { type: "content_block_stop", index: 0 },
      { type: "message_delta", delta: { stop_reason: "end_turn" }, usage: { output_tokens: 20 } },
      { type: "message_stop" },
    ].map(event).join("");
  } else if (model.api === "openai-completions") {
    body = `data: ${JSON.stringify({ id: "fixture", model: model.id, choices: [{ index: 0, delta: { role: "assistant", content: text }, finish_reason: "stop" }], usage: { prompt_tokens: 10, completion_tokens: 20, total_tokens: 30 } })}\n\ndata: [DONE]\n\n`;
  } else {
    const item = { type: "message", id: "msg_fixture", role: "assistant", status: "completed", content: [{ type: "output_text", text, annotations: [] }] };
    body = [
      { type: "response.created", response: { id: "fixture", model: model.id, status: "in_progress" } },
      { type: "response.output_item.added", output_index: 0, item: { ...item, content: [] } },
      { type: "response.output_text.delta", item_id: item.id, output_index: 0, content_index: 0, delta: text },
      { type: "response.output_item.done", output_index: 0, item },
      { type: "response.completed", response: { id: "fixture", model: model.id, status: "completed", output: [item], usage: { input_tokens: 10, output_tokens: 20, total_tokens: 30 } } },
    ].map(event).join("");
  }
  return new Response(body, { headers: { "content-type": "text/event-stream" } });
}

async function infer(providerId, id, effort, maxTokens = 12288) {
  const model = getBuiltinModel(providerId, id);
  assert.ok(model, `Pi must supply ${providerId}/${id} before it is recommended`);
  const provider = providers.find((entry) => entry.id === providerId);
  // No real credentials or network: run the installed Pi serializer AND parser,
  // replacing only HTTP at the same per-request fetch boundary used in production.
  const fixtureToken = `e30.${Buffer.from(JSON.stringify({ "https://api.openai.com/auth": { chatgpt_account_id: "fixture-account" } })).toString("base64url")}.fixture`;
  const ctx = { modelRegistry: {
    getAvailable: () => [model],
    streamSimple: (selected, context, options) => provider.streamSimple(selected, context, { ...options, apiKey: fixtureToken }),
  } };
  let payload, calls = 0;
  const result = await runProviderInference(ctx, {
    protocol: "loupist-provider/1", type: "inference", provider: providerId, model: id,
    thinkingLevel: effort, runOptions: Object.fromEntries(runControlsFor(model).map((control) => [control.id, control.default_choice])),
    messages: [{ role: "user", content: "Extract claims: Acme builds chips." }], schema,
    maxOutputTokens: maxTokens, timeoutMs: 5000,
  }, { fetch: async (_url, init) => {
    calls++;
    payload = JSON.parse(typeof init.body === "string" ? init.body : zstdDecompressSync(init.body).toString());
    return responseFor(model);
  } });
  assert.equal(calls, 1, `${providerId}/${id}: dispatch count; ${JSON.stringify(result)}`);
  assert.equal(result.disposition, "completed", `${providerId}/${id}: ${JSON.stringify(result)}`);
  assert.deepEqual(result.body, answer);
  assert.equal(payload.model, id);
  assert.equal(payload.service_tier, undefined);
  for (const key of ["max_tokens", "max_completion_tokens", "max_output_tokens"]) {
    if (key in payload) assert.ok(Number.isInteger(payload[key]) && payload[key] <= maxTokens, `${id}: bounded ${key}`);
  }
  return { model, payload };
}

for (const group of defaults) {
  for (const candidate of group.models) {
    test(`World recommendation uses real Pi transport: ${group.provider}/${candidate.id} (${candidate.thinking_level})`, async () => {
      const { model, payload } = await infer(group.provider, candidate.id, candidate.thinking_level);
      const effort = candidate.thinking_level;
      if (model.api === "anthropic-messages") {
        if (effort === "off") assert.equal(payload.thinking.type, "disabled");
        else if (model.compat?.forceAdaptiveThinking) {
          assert.equal(payload.thinking.type, "adaptive");
          assert.equal(payload.output_config.effort, effort);
        } else {
          assert.equal(payload.thinking.type, "enabled");
          assert.equal(payload.thinking.budget_tokens, 2048); // reviewed low/manual preset
          assert.ok(payload.max_tokens - payload.thinking.budget_tokens >= 1024);
        }
      } else if (effort !== "off") {
        if (model.compat?.thinkingFormat === "together") assert.equal(payload.reasoning.enabled, true);
        else assert.equal(payload.reasoning?.effort ?? payload.reasoning_effort, effort);
      }
      if (model.api === "openai-codex-responses") {
        assert.equal(payload.max_output_tokens, undefined, "subscription API has no output-cap field");
      }
    });
  }
}

test("disabled thinking is serialized as provider-native off, never literal off or an invalid budget", async () => {
  for (const [provider, id] of [["anthropic", "claude-haiku-4-5"], ["openai", "gpt-6-luna"], ["deepseek", "deepseek-flash"]]) {
    const { payload } = await infer(provider, id, "off", 4096);
    if (provider === "openai") assert.equal(payload.reasoning.effort, "none");
    else assert.equal(payload.thinking.type, "disabled");
  }
});

test("manual high thinking cannot increase the host's total token ceiling", async () => {
  const { payload } = await infer("anthropic", "claude-haiku-4-5", "high", 12288);
  assert.equal(payload.max_tokens, 12288);
  assert.equal(payload.thinking.budget_tokens, 11264);
});

// A user can opt into Max later; the preset must not clamp that saved choice.
test("Luna retains an explicit user-selected Max effort on both OpenAI routes", async () => {
  for (const provider of ["openai", "openai-codex"]) {
    const { payload } = await infer(provider, "gpt-6-luna", "max");
    assert.equal(payload.reasoning.effort, "max");
  }
});
