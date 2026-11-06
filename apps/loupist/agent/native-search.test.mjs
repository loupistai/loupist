import assert from "node:assert/strict";
import test from "node:test";
import { runNativeSearch } from "./loupist-native-search.mjs";
import {
  buildAnthropicBody,
  completedAnthropicTransportPrefix,
  parseAnthropicSearchPayload,
  parsePageAge,
} from "./native-search/anthropic.mjs";
import { completedCodexTransportPrefix, parseCodexSearchPayload } from "./native-search/codex.mjs";
import { classifyHttpFailure, toSearchErrorResult, toSearchResult } from "./native-search/common.mjs";
import { parseXaiSearchResponse } from "./native-search/xai.mjs";

// One SSE frame per event; the stream ends right after its terminal frame.
const sse = (events, named = false) => events
  .map((event) => `${named ? `event: ${event.type}\n` : ""}data: ${JSON.stringify(event)}\n`)
  .join("\n");
const ANSWER = "Provider synthesized answer that must be discarded.";

const CODEX_SEARCH_SSE = sse([
  { type: "response.web_search_call.completed", item_id: "ws_test" },
  {
    type: "response.output_item.done",
    item: {
      type: "web_search_call",
      action: { sources: [{ url: "https://example.com/article?utm_source=openai", title: "Example Article" }] },
    },
  },
  {
    type: "response.output_item.done",
    item: {
      type: "message",
      content: [{
        type: "output_text",
        text: ANSWER,
        annotations: [{ type: "url_citation", url: "https://example.com/article", title: "Example Article" }],
      }],
    },
  },
  {
    type: "response.completed",
    response: { id: "resp_codex_test", model: "gpt-5.5", usage: { input_tokens: 12, output_tokens: 7, total_tokens: 19 } },
  },
]);

const ANTHROPIC_SEARCH_JSON = {
  id: "msg_test",
  model: "claude-haiku-4-5",
  content: [
    { type: "server_tool_use", name: "web_search", input: { query: "latest climate report" } },
    {
      type: "web_search_tool_result",
      content: [{ type: "web_search_result", title: "UN report", url: "https://example.org/un-report", page_age: "2 days ago" }],
    },
    {
      type: "text",
      text: ANSWER,
      citations: [{ url: "https://example.org/un-report", title: "UN report", cited_text: "emissions fell" }],
    },
  ],
  usage: { input_tokens: 20, output_tokens: 8, server_tool_use: { web_search_requests: 1 } },
};

const ANTHROPIC_SEARCH_SSE = sse([
  { type: "message_start", message: { id: "msg_stream_test", model: "claude-haiku-4-5", usage: { input_tokens: 21, output_tokens: 0 } } },
  { type: "content_block_start", index: 0, content_block: { type: "server_tool_use", id: "srvtoolu_test", name: "web_search" } },
  {
    type: "content_block_start",
    index: 1,
    content_block: {
      type: "web_search_tool_result",
      tool_use_id: "srvtoolu_test",
      content: [{ type: "web_search_result", title: "Streaming report", url: "https://example.org/streaming-report", page_age: "3 hours ago" }],
    },
  },
  { type: "content_block_start", index: 2, content_block: { type: "text", text: "" } },
  {
    type: "content_block_delta",
    index: 2,
    delta: {
      type: "citations_delta",
      citation: { type: "web_search_result_location", url: "https://example.org/streaming-report", title: "Streaming report", cited_text: "Cited source excerpt" },
    },
  },
  { type: "message_delta", usage: { output_tokens: 9, server_tool_use: { web_search_requests: 1 } } },
  { type: "message_stop" },
], true);

/** A Pi model registry whose `complete` serializes through the host's onPayload
 * hook and sends one request through the per-request fetch, like Pi does. */
function piContext(provider, captured, basePayload = {}) {
  return {
    modelRegistry: {
      find: (requested, model) => (requested === provider ? { provider, id: model } : undefined),
      complete: async (_model, _context, options) => {
        captured.options = options;
        captured.body = await options.onPayload(basePayload);
        const response = await options.fetch(`https://${provider}.invalid/v1`, {
          method: "POST",
          body: JSON.stringify(captured.body),
        });
        captured.status = response.status;
        return { role: "assistant", content: [], stopReason: response.ok ? "stop" : "error" };
      },
    },
  };
}

test("Codex parsing keeps hosted sources and usage across stream shapes, never the answer", () => {
  const completedOnly = {
    id: "resp_completed",
    object: "response",
    status: "completed",
    model: "gpt-5.6-luna",
    output: [
      { type: "web_search_preview_call", action: { sources: [{ url: "https://example.com/report", title: "Public Report" }] } },
      {
        type: "message",
        content: [{ type: "output_text", text: ANSWER, annotations: [{ type: "url_citation", url: "https://example.com/report", cited_text: "reported figure" }] }],
      },
    ],
  };
  const parsed = toSearchResult("openai-codex", parseCodexSearchPayload(CODEX_SEARCH_SSE), 5);
  assert.deepEqual(parsed.sources.map((source) => [source.url, source.title]), [["https://example.com/article", "Example Article"]]);
  assert.equal(parsed.requestId, "resp_codex_test");
  assert.deepEqual(parsed.usage, { inputTokens: 12, outputTokens: 7, totalTokens: 19 });
  // SSE completion only, a non-SSE JSON snapshot, and NDJSON events.
  for (const payload of [
    sse([{ type: "response.completed", response: completedOnly }]),
    JSON.stringify(completedOnly),
    [{ type: "response.web_search_preview_call.completed" }, { type: "response.completed", response: completedOnly }]
      .map((event) => JSON.stringify(event)).join("\n"),
  ]) {
    const result = toSearchResult("openai-codex", parseCodexSearchPayload(payload), 5);
    assert.equal(result.sources[0].url, "https://example.com/report");
    assert.equal(result.sources[0].snippet, "reported figure");
    assert.equal(JSON.stringify(result).includes(ANSWER), false);
  }
  assert.equal(JSON.stringify(parsed).includes(ANSWER), false);
});

test("completions without a hosted search call fail, and answers are never mined for URLs", () => {
  const citedOnly = {
    type: "message",
    content: [{ type: "output_text", text: "stale", annotations: [{ type: "url_citation", url: "https://example.com/cited-only" }] }],
  };
  for (const payload of [
    sse([{ type: "response.output_item.done", item: citedOnly }, { type: "response.completed", response: { id: "r" } }]),
    sse([{ type: "response.completed", response: { id: "r", output: [citedOnly] } }]),
    JSON.stringify({ id: "r", object: "response", status: "completed", output: [citedOnly] }),
  ]) {
    assert.throws(() => parseCodexSearchPayload(payload), (error) => error.kind === "no_search_tool");
  }
  assert.throws(
    () => parseXaiSearchResponse({ annotations: [{ type: "url_citation", url: "https://example.com/c" }], citations: ["https://example.com/c"] }),
    (error) => error.kind === "no_search_tool",
  );
  assert.throws(
    () => parseAnthropicSearchPayload(JSON.stringify({ content: [{ type: "text", text: "stale", citations: [{ url: "https://example.com/c" }] }] })),
    (error) => error.kind === "no_search_tool",
  );

  const emptySearch = parseCodexSearchPayload(sse([
    { type: "response.output_item.done", item: { type: "web_search_call", action: { sources: [] } } },
    {
      type: "response.output_item.done",
      item: { type: "message", content: [{ type: "output_text", text: "See https://invented.example/from-answer." }] },
    },
    { type: "response.completed", response: { id: "resp_empty" } },
  ]));
  assert.deepEqual(toSearchResult("openai-codex", emptySearch, 5).sources, []);
});

test("Codex search runs through Pi with the forced web_search body and stops at the terminal frame", async () => {
  assert.equal(completedCodexTransportPrefix(`${CODEX_SEARCH_SSE}partial`), CODEX_SEARCH_SSE);
  const captured = {};
  const result = await runNativeSearch(
    { provider: "openai-codex", query: "latest public report", limit: 3 },
    {
      ctx: piContext("openai-codex", captured),
      availableModelIds: ["gpt-5.6-luna", "gpt-5.5", "gpt-5.4-mini"],
      resolveAuth: async () => ({ apiKey: "codex-secret-token" }),
      // Codex may keep the SSE transport open after its terminal response event.
      fetchImpl: async () => new Response(
        new ReadableStream({ start: (controller) => controller.enqueue(new TextEncoder().encode(CODEX_SEARCH_SSE)) }),
        { status: 200, headers: { "Content-Type": "text/event-stream" } },
      ),
    },
  );
  assert.equal(captured.options.transport, "sse");
  assert.equal(captured.body.model, "gpt-5.5");
  assert.deepEqual(captured.body.tool_choice, { type: "web_search" });
  assert.deepEqual(captured.body.tools, [{ type: "web_search", search_context_size: "high" }]);
  assert.deepEqual(captured.body.include, ["web_search_call.action.sources"]);
  assert.equal(captured.body.reasoning.effort, "low");
  assert.equal(result.status, "ok");
  assert.equal(result.sources[0].url, "https://example.com/article");
  assert.equal(JSON.stringify(result).includes("codex-secret-token"), false);
  assert.equal(JSON.stringify(result).includes(ANSWER), false);
});

test("OpenAI search runs through Pi's openai provider for a ChatGPT sign-in or a key", async () => {
  for (const auth of [{ apiKey: "plan-access-token" }, { apiKey: "sk-fixture-key" }]) {
    const captured = {};
    const result = await runNativeSearch(
      { provider: "openai", query: "latest public report", limit: 3 },
      {
        ctx: piContext("openai", captured),
        availableModelIds: ["gpt-6.1-sol", "gpt-5.4-mini"],
        resolveAuth: async () => auth,
        fetchImpl: async () => new Response(CODEX_SEARCH_SSE, { status: 200, headers: { "Content-Type": "text/event-stream" } }),
      },
    );
    assert.equal(captured.body.model, "gpt-5.4-mini");
    assert.deepEqual(captured.body.tools, [{ type: "web_search", search_context_size: "high" }]);
    assert.equal(captured.body.store, false);
    // A ChatGPT sign-in rejects these fields; the search body never sends them.
    for (const field of ["temperature", "max_output_tokens", "prompt_cache_retention"]) {
      assert.equal(field in captured.body, false, field);
    }
    assert.equal(result.provider, "openai");
    assert.equal(result.sources[0].url, "https://example.com/article");
    assert.equal(JSON.stringify(result).includes(auth.apiKey), false);
  }
  // A plan at its limit is never retried: it falls back instead.
  const limited = await runNativeSearch(
    { provider: "openai", query: "q", limit: 3 },
    {
      ctx: piContext("openai", {}),
      resolveAuth: async () => ({ apiKey: "plan-access-token" }),
      fetchImpl: async () => new Response('{"error":{"code":"subscription_sharing_usage_limit_exceeded"}}', { status: 429 }),
    },
  ).then(() => assert.fail("search should fail"), (failure) => failure);
  assert.equal(toSearchErrorResult("openai", limited).error_kind, "unavailable");
});

test("Pi search HTTP failures are classified without exposing credentials or provider text", async () => {
  for (const [status, kind, headers] of [
    [401, "unavailable", {}],
    [429, "rate_limited", { "retry-after": "2" }],
    [503, "transport", {}],
  ]) {
    const error = await runNativeSearch(
      { provider: "openai-codex", query: "q", limit: 3 },
      {
        ctx: piContext("openai-codex", {}),
        resolveAuth: async () => ({ apiKey: "codex-secret-token", headers: { Authorization: "Bearer codex-secret-token" } }),
        fetchImpl: async () => new Response(`provider said codex-secret-token ${status}`, { status, headers }),
      },
    ).then(() => assert.fail("search should fail"), (failure) => failure);
    const result = toSearchErrorResult("openai-codex", error);
    assert.equal(result.status, "error");
    assert.equal(result.error_kind, kind);
    if (status === 429) assert.equal(result.retry_after_ms, 2000);
    assert.equal(JSON.stringify(result).includes("codex-secret-token"), false);
    assert.equal(JSON.stringify(result).includes("provider said"), false);
  }
  assert.equal(classifyHttpFailure(400, "Tool choice 'web_search_preview' not found in 'tools' parameter."), "provider");
  assert.equal(classifyHttpFailure(402, "payment required"), "unavailable");
  assert.equal(classifyHttpFailure(400, "insufficient_quota"), "unavailable");
});

test("Anthropic parsing accepts JSON and streaming results without the provider answer", () => {
  assert.equal(completedAnthropicTransportPrefix(`${ANTHROPIC_SEARCH_SSE}never-closes`), ANTHROPIC_SEARCH_SSE);
  const json = toSearchResult("anthropic", parseAnthropicSearchPayload(JSON.stringify(ANTHROPIC_SEARCH_JSON)), 5);
  assert.equal(json.sources[0].url, "https://example.org/un-report");
  assert.equal(json.searchRequestCount, 1);
  const streamed = toSearchResult("anthropic", parseAnthropicSearchPayload(ANTHROPIC_SEARCH_SSE), 5);
  assert.equal(streamed.requestId, "msg_stream_test");
  assert.equal(streamed.sources[0].url, "https://example.org/streaming-report");
  assert.equal(streamed.sources[0].snippet, "Cited source excerpt");
  assert.deepEqual(streamed.usage, { inputTokens: 21, outputTokens: 9, searchRequests: 1 });
  for (const result of [json, streamed]) assert.equal(JSON.stringify(result).includes(ANSWER), false);

  assert.throws(
    () => parseAnthropicSearchPayload(JSON.stringify({
      content: [
        { type: "server_tool_use", name: "web_search" },
        { type: "web_search_tool_result", content: { type: "web_search_tool_result_error", error_code: "unavailable" } },
      ],
    })),
    (error) => error.kind === "unavailable",
  );
});

test("Anthropic search through Pi keeps Pi's system blocks and replaces only the search payload", async () => {
  const captured = {};
  const system = [{ type: "text", text: "Pi OAuth system identity", cache_control: { type: "ephemeral" } }];
  const result = await runNativeSearch(
    { provider: "anthropic", query: "latest primary report", limit: 3, include_domains: ["example.org"] },
    {
      ctx: piContext("anthropic", captured, { system, messages: [{ role: "user", content: "placeholder" }] }),
      availableModelIds: ["claude-haiku-4-5"],
      resolveAuth: async () => ({ apiKey: "sk-ant-secret" }),
      fetchImpl: async () => new Response(ANTHROPIC_SEARCH_SSE, { status: 200, headers: { "Content-Type": "text/event-stream" } }),
    },
  );
  assert.deepEqual(captured.body.system, system);
  assert.equal(captured.body.model, "claude-haiku-4-5");
  assert.equal(captured.body.max_tokens, 4096);
  assert.deepEqual(captured.body.tools, [{ type: "web_search_20250305", name: "web_search", max_uses: 3, allowed_domains: ["example.org"] }]);
  // Haiku 4.5 rejects effort, so none is sent.
  assert.equal(captured.body.output_config, undefined);
  assert.match(captured.body.messages[0].content, /^latest primary report/);
  assert.equal(result.sources[0].url, "https://example.org/streaming-report");
  assert.equal(JSON.stringify(result).includes("sk-ant-secret"), false);
});

test("Anthropic search prefers the cheapest search model and asks it for low effort", () => {
  const request = { query: "quarterly results", limit: 5 };
  assert.equal(
    buildAnthropicBody(request, "claude-haiku-5-5").output_config.effort,
    "low",
  );
  assert.equal(buildAnthropicBody(request, "claude-sonnet-4-5").output_config, undefined);
});

test("Anthropic's search cap keeps earlier results, and page ages become dates when absolute", () => {
  const capped = parseAnthropicSearchPayload(JSON.stringify({
    ...ANTHROPIC_SEARCH_JSON,
    content: [
      ...ANTHROPIC_SEARCH_JSON.content,
      { type: "server_tool_use", name: "web_search", input: { query: "more" } },
      { type: "web_search_tool_result", content: { type: "web_search_tool_result_error", error_code: "max_uses_exceeded" } },
    ],
  }));
  assert.deepEqual(capped.sources.map((source) => source.url), ["https://example.org/un-report"]);
  assert.throws(() => parseAnthropicSearchPayload(JSON.stringify({
    ...ANTHROPIC_SEARCH_JSON,
    content: [{ type: "web_search_tool_result", content: { type: "web_search_tool_result_error", error_code: "too_many_requests" } }],
  })), (error) => error.kind === "rate_limited");

  assert.equal(parsePageAge("April 30, 2025"), "2025-04-30");
  assert.equal(parsePageAge("2025-04-30T08:00:00Z"), "2025-04-30T08:00:00Z");
  assert.equal(parsePageAge("2 days ago"), "2 days ago");
  assert.equal(parsePageAge(""), undefined);
});

test("xAI sources and citations count as either URL strings or objects", () => {
  const parsed = parseXaiSearchResponse({
    model: "grok-4.5",
    output: [{ type: "web_search_call", action: { sources: ["https://example.org/a", { url: "https://example.org/b", title: "B" }] } }],
    citations: [{ url: "https://example.org/c", title: "C" }, "https://example.org/a"],
  });
  assert.deepEqual(parsed.sources.map((source) => [source.url, source.title]), [
    ["https://example.org/a", "https://example.org/a"],
    ["https://example.org/b", "B"],
    ["https://example.org/c", "C"],
  ]);
});

test("xAI search uses the Responses API with bounded filters and keeps only cited sources", async () => {
  const parsed = toSearchResult("xai", parseXaiSearchResponse({
    output_text: "Top-level xAI answer",
    annotations: [{ type: "url_citation", url: "https://example.com/top-annotation", text: "Top annotation text" }],
    output: [
      {
        type: "message",
        content: [{ type: "output_text", text: ANSWER, annotations: [{ type: "url_citation", url: "https://example.com/cited", text: "Cited source excerpt" }] }],
      },
      { type: "web_search_call", action: { sources: [{ url: "https://example.com/raw", title: "Raw result" }] } },
    ],
    citations: ["https://example.com/top-level-citation"],
    usage: { input_tokens: 12, output_tokens: 8, total_tokens: 20 },
  }), 10);
  assert.deepEqual(parsed.sources.map((source) => source.url), [
    "https://example.com/top-annotation",
    "https://example.com/cited",
    "https://example.com/raw",
    "https://example.com/top-level-citation",
  ]);
  assert.equal(parsed.sources[1].snippet, "Cited source excerpt");
  assert.equal(JSON.stringify(parsed).includes("Top-level xAI answer"), false);

  let captured;
  const result = await runNativeSearch(
    {
      provider: "xai",
      query: "latest xAI API release",
      limit: 3,
      include_domains: ["a.example", "b.example", "c.example", "d.example", "e.example", "f.example"],
      exclude_domains: ["ignored.example"],
    },
    {
      availableModelIds: ["grok-4.6", "grok-4.5"],
      resolveAuth: async () => ({ apiKey: "xai-redacted-test" }),
      fetchImpl: async (url, init) => {
        captured = { url: String(url), headers: init.headers, body: JSON.parse(init.body) };
        return Response.json({
          id: "resp_xai_mock",
          model: "grok-4.5",
          output: [{ type: "web_search_call", action: { sources: [{ url: "https://a.example/web-search", title: "Web Search" }] } }],
        });
      },
    },
  );
  assert.match(captured.url, /api\.x\.ai\/v1\/responses$/);
  assert.equal(captured.headers.authorization, "Bearer xai-redacted-test");
  assert.equal(captured.body.model, "grok-4.5");
  assert.deepEqual(captured.body.reasoning, { effort: "low" });
  assert.deepEqual(captured.body.tools, [
    { type: "web_search", filters: { allowed_domains: ["a.example", "b.example", "c.example", "d.example", "e.example"] } },
  ]);
  assert.equal(result.sources[0].url, "https://a.example/web-search");
  assert.equal(JSON.stringify(result).includes("xai-redacted-test"), false);
});

test("invalid input fails before credentials are resolved, and stalled requests hard-time out", async () => {
  let resolvedAuth = false;
  const deps = { resolveAuth: async () => { resolvedAuth = true; return { apiKey: "unused" }; } };
  for (const [request, message] of [
    [{ provider: "xai", query: "q".repeat(4_097), limit: 5 }, "search query is invalid"],
    [{ provider: "xai", query: "q", limit: 11 }, "search limit is invalid"],
    [{ provider: "xai", query: "q", limit: 5, recency: "yesterday" }, "search recency is invalid"],
    [{ provider: "xai", query: "q", limit: 5, include_domains: ["not a hostname!"] }, "search domains are invalid"],
  ]) {
    await assert.rejects(() => runNativeSearch(request, deps), (error) => error.kind === "malformed" && error.message === message);
  }
  await assert.rejects(() => runNativeSearch({ provider: "unknown", query: "q", limit: 5 }, deps), (error) => error.kind === "unavailable");
  assert.equal(resolvedAuth, false);

  const keepAlive = setTimeout(() => undefined, 100);
  try {
    await assert.rejects(
      () => runNativeSearch(
        { provider: "xai", query: "bounded request", limit: 3 },
        {
          timeoutMs: 10,
          resolveAuth: async () => ({ apiKey: "xai-redacted-timeout" }),
          fetchImpl: async (_url, init) => new Promise((_resolve, reject) => {
            init.signal.addEventListener("abort", () => reject(init.signal.reason), { once: true });
          }),
        },
      ),
      (error) => error.kind === "timeout",
    );
  } finally {
    clearTimeout(keepAlive);
  }
});
