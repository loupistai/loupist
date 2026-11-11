import {
  addSource,
  completeThroughPi,
  DEFAULT_SEARCH_INSTRUCTIONS,
  NativeSearchError,
  parseSseJsonEvents,
  PLAN_REFUSAL,
  searchQueryText,
} from "./common.mjs";

// Hosted-search parsing is derived from Oh My Pi. See ../loupist-native-search.mjs
// for the pinned upstream revision and MIT notice.
// The Responses API's hosted search, through Pi's `openai` provider (a ChatGPT
// sign-in or an OpenAI API key) or the development-only Codex client.
// Search needs source URLs, so prefer the lowest pinned input/output price.
const CODEX_SEARCH_MODELS = Object.freeze([
  "gpt-6-luna",
  "gpt-5.6-luna",
  "gpt-6.1-terra",
  "gpt-6.1-sol",
  "gpt-6-sol",
  "gpt-5.6-terra",
  "gpt-5.6-sol",
  "gpt-6-astra",
]);
const OPENAI_SEARCH_MODELS = Object.freeze([
  "gpt-6-luna",
  "gpt-5.6-luna",
  "gpt-5.4-mini",
  "gpt-6.1-terra",
  "gpt-6.1-sol",
  "gpt-6-sol",
  "gpt-5.6-terra",
  "gpt-5.4",
  "gpt-5.6-sol",
  "gpt-5.5",
  "gpt-6-astra",
]);
const HOSTED_SEARCH_TOOL = Object.freeze({ type: "web_search", search_context_size: "high" });

function completedEvent(response) {
  return { type: "response.completed", response };
}

function parseJsonObject(parsed) {
  if (Array.isArray(parsed)) return parsed;
  if (!parsed || typeof parsed !== "object") {
    throw new NativeSearchError("malformed", "Codex search response is invalid");
  }
  if (typeof parsed.type === "string") return [parsed];
  if (
    Array.isArray(parsed.output) ||
    parsed.object === "response" ||
    typeof parsed.status === "string" ||
    typeof parsed.id === "string"
  ) {
    return [completedEvent(parsed)];
  }
  throw new NativeSearchError("malformed", "Codex search response is invalid");
}

function parseNdjsonEvents(text) {
  const events = [];
  for (const line of String(text).split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith(":")) continue;
    try {
      events.push(JSON.parse(trimmed));
    } catch {
      throw new NativeSearchError("malformed", "Codex search response is invalid");
    }
  }
  if (events.length === 0) {
    throw new NativeSearchError("malformed", "Codex search response is invalid");
  }
  return events;
}

function parseCodexJsonTransport(text) {
  try {
    return parseJsonObject(JSON.parse(text));
  } catch (error) {
    if (error instanceof NativeSearchError) throw error;
    return parseNdjsonEvents(text);
  }
}

export function parseCodexSearchPayload(text) {
  const trimmed = String(text).trim();
  if (!trimmed) {
    throw new NativeSearchError("malformed", "Codex search response is empty");
  }
  const sseEvents = parseSseJsonEvents(trimmed);
  if (sseEvents.length > 0) return parseCodexSearchEvents(sseEvents);
  return parseCodexSearchEvents(parseCodexJsonTransport(trimmed));
}

function extractError(rawEvent) {
  const candidates = [rawEvent, rawEvent?.error, rawEvent?.response?.error];
  let code = "";
  let message = "";
  for (const candidate of candidates) {
    if (!candidate || typeof candidate !== "object") continue;
    if (!code && typeof candidate.code === "string" && candidate.code) code = candidate.code;
    if (!message && typeof candidate.message === "string" && candidate.message) {
      message = candidate.message;
    }
  }
  return { code, message };
}

function classifyError(code, message) {
  const detail = `${code} ${message}`.toLowerCase();
  if (PLAN_REFUSAL.test(detail)) return "unavailable";
  if (/rate[- ]?limit|too many requests|quota|\b429\b/u.test(detail)) return "rate_limited";
  if (/unauthori[sz]ed|\b401\b|forbidden|\b403\b/u.test(detail)) return "unavailable";
  if (/timeout|timed out/u.test(detail)) return "timeout";
  return "provider";
}

function isHostedSearchItem(item) {
  const type = typeof item?.type === "string" ? item.type : "";
  return type === "web_search_call" || type === "web_search_preview_call";
}

function collectOutputItem(item, sources) {
  if (!item || typeof item !== "object") return false;
  let invoked = false;
  if (isHostedSearchItem(item)) {
    invoked = true;
    for (const group of [item.action?.sources, item.sources, item.results]) {
      for (const source of group ?? []) {
        const url = source?.url ?? source?.source_website_url;
        if (!url) continue;
        addSource(sources, { title: source.title ?? source.caption ?? url, url });
      }
    }
  }
  if (item.type === "message" && Array.isArray(item.content)) {
    for (const part of item.content) {
      for (const annotation of part?.annotations ?? []) {
        if (annotation?.type === "url_citation" && annotation.url) {
          addSource(sources, {
            title: annotation.title ?? annotation.url,
            url: annotation.url,
            snippet: annotation.cited_text ?? annotation.snippet,
          });
        }
      }
    }
  }
  return invoked;
}

function parseCodexSearchEvents(events) {
  const sources = [];
  let model;
  let requestId;
  let usage;
  let webSearchInvoked = false;

  for (const rawEvent of events) {
    const eventType = typeof rawEvent?.type === "string" ? rawEvent.type : "";
    if (!eventType) continue;
    if (eventType.startsWith("response.web_search")) webSearchInvoked = true;
    if (eventType === "error") {
      const { code, message } = extractError(rawEvent);
      throw new NativeSearchError(
        classifyError(code, message),
        `Codex error (${code}): ${message || "Unknown error"}`,
      );
    }
    if (eventType === "response.failed") {
      const { code, message } = extractError(rawEvent);
      throw new NativeSearchError(
        classifyError(code, message),
        code
          ? `Codex request failed (${code}): ${message || "Request failed"}`
          : `Codex request failed: ${message || "Request failed"}`,
      );
    }
    if (eventType === "response.created") {
      requestId = rawEvent.response?.id ?? requestId;
      model = rawEvent.response?.model ?? model;
      continue;
    }
    if (eventType === "response.output_item.done" || eventType === "response.output_item.added") {
      if (collectOutputItem(rawEvent.item, sources)) webSearchInvoked = true;
      continue;
    }
    if (eventType === "response.completed" || eventType === "response.done") {
      const response = rawEvent.response;
      model = response?.model ?? model;
      requestId = response?.id ?? requestId;
      if (response?.usage) {
        usage = {
          inputTokens: response.usage.input_tokens,
          outputTokens: response.usage.output_tokens,
          totalTokens: response.usage.total_tokens,
        };
      }
      for (const item of Array.isArray(response?.output) ? response.output : []) {
        if (collectOutputItem(item, sources)) webSearchInvoked = true;
      }
    }
  }
  if (!webSearchInvoked) {
    throw new NativeSearchError(
      "no_search_tool",
      "Codex returned a completion without running web search",
    );
  }
  return { sources, model, requestId, usage, searchRequestCount: 1 };
}

function isTerminalEvent(event) {
  const type = typeof event?.type === "string" ? event.type : "";
  if (["error", "response.failed", "response.completed", "response.done", "response.incomplete"].includes(type)) {
    return true;
  }
  return (
    event?.object === "response" &&
    ["completed", "failed", "incomplete", "cancelled"].includes(event.status)
  );
}

export function completedCodexTransportPrefix(text) {
  const raw = String(text);
  for (const match of raw.matchAll(/(?:^|\r?\n)data:\s*(.*?)\r?\n/g)) {
    const data = match[1].trim();
    if (!data || data === "[DONE]") continue;
    try {
      if (isTerminalEvent(JSON.parse(data))) return raw.slice(0, match.index + match[0].length);
    } catch {
      // Keep buffering until a complete JSON data line arrives.
    }
  }
  let completeSseEnd = 0;
  for (const match of raw.matchAll(/\r?\n\r?\n/g)) completeSseEnd = match.index + match[0].length;
  if (completeSseEnd > 0) {
    const prefix = raw.slice(0, completeSseEnd);
    try {
      if (parseSseJsonEvents(prefix).some(isTerminalEvent)) return prefix;
    } catch {
      // The final parser reports complete malformed frames after transport close.
    }
  }
  try {
    const parsed = JSON.parse(raw);
    const events = Array.isArray(parsed) ? parsed : [parsed];
    if (events.some(isTerminalEvent)) return raw;
  } catch {
    // A streaming JSON object or NDJSON line may still be incomplete.
  }
  let ndjsonEnd = 0;
  for (const match of raw.matchAll(/.*(?:\r?\n|$)/g)) {
    const line = match[0];
    if (!line) continue;
    const trimmed = line.trim();
    ndjsonEnd = match.index + line.length;
    if (!trimmed || trimmed.startsWith(":") || !line.endsWith("\n")) continue;
    try {
      if (isTerminalEvent(JSON.parse(trimmed))) return raw.slice(0, ndjsonEnd);
    } catch {
      return undefined;
    }
  }
  return undefined;
}

export function buildCodexBody(request, model) {
  return {
    model,
    stream: true,
    store: false,
    include: ["web_search_call.action.sources"],
    parallel_tool_calls: true,
    input: [{
      type: "message",
      role: "user",
      content: [{ type: "input_text", text: searchQueryText(request) }],
    }],
    tools: [HOSTED_SEARCH_TOOL],
    tool_choice: { type: "web_search" },
    instructions: DEFAULT_SEARCH_INSTRUCTIONS,
    reasoning: { effort: "low" },
  };
}

/** One hosted-search request through the given Pi provider. */
function searchThrough(provider) {
  return async ({ request, model, deps, signal }) => {
    const body = buildCodexBody(request, model);
    const text = await completeThroughPi(deps.ctx, {
      provider,
      model,
      signal,
      fetchImpl: deps.fetchImpl,
      transport: "sse",
      context: {
        systemPrompt: "Run the supplied bounded hosted-search request.",
        messages: [{
          role: "user",
          content: [{ type: "text", text: "Run the supplied search request." }],
          timestamp: Date.now(),
        }],
        tools: [],
      },
      onPayload: () => body,
      completedPrefix: completedCodexTransportPrefix,
    });
    return parseCodexSearchPayload(text);
  };
}

/** A ChatGPT sign-in or an OpenAI API key: Pi's `openai` provider. */
export const openaiSearchAdapter = Object.freeze({
  id: "openai",
  modelIds: OPENAI_SEARCH_MODELS,
  search: searchThrough("openai"),
});

export const codexSearchAdapter = Object.freeze({
  id: "openai-codex",
  modelIds: CODEX_SEARCH_MODELS,
  search: searchThrough("openai-codex"),
});
