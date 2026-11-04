import {
  closeSync,
  constants,
  fstatSync,
  ftruncateSync,
  fsyncSync,
  openSync,
  readFileSync,
  writeSync,
} from "node:fs";
import { isAbsolute, join } from "node:path";
import { ModelRuntime, SettingsManager } from "@earendil-works/pi-coding-agent";
import { getSupportedThinkingLevels } from "@earendil-works/pi-ai";
import { runControlsFor } from "./model-run-controls.mjs";
import { MAX_INFERENCE_REQUEST_BYTES, runProviderInference } from "./provider-inference.mjs";
import {
  isNativeSearchProvider,
  runNativeSearch,
  validateSearchRequest,
} from "./loupist-native-search.mjs";
import {
  assertSearchResultSafe,
  NativeSearchError,
  toSearchErrorResult,
} from "./native-search/common.mjs";

const PROTOCOL = "loupist-provider/1";
/** The name a provider shows for this app. Pi 1.0.1 hardcodes its own. */
export const LOUPIST_PROVIDER_NAME = "Loupist";
const XAI_DEVICE_CODE_URL = "https://auth.x.ai/oauth2/device/code";
/** The title of the paste prompt; Rust answers only this UI request. */
export const MANUAL_CODE_TITLE = `${PROTOCOL}:manual_code`;
const MAX_RESULT_BYTES = 512 * 1024;
const LIVE_MODELS_URL = "https://api.openai.com/v1/models";
const LIVE_MODELS_TIMEOUT_MS = 8_000;
const MAX_LIVE_MODELS_BYTES = 1024 * 1024;
const MAX_REQUEST_BYTES = 64 * 1024;
const IS_WINDOWS = process.platform === "win32";
// libuv rejects O_WRONLY | O_TRUNC on Windows, so truncate the validated
// descriptor below instead of asking Windows to truncate during open.
export function resultOpenFlagsForPlatform(platform = process.platform) {
  return platform === "win32"
    ? constants.O_WRONLY
    : constants.O_WRONLY | constants.O_TRUNC | (constants.O_NOFOLLOW ?? 0);
}

const RESULT_OPEN_FLAGS = resultOpenFlagsForPlatform();
const RESULT_FILE = process.env.LOUPIST_PROVIDER_RESULT_FILE;
const REQUEST_FILE = process.env.LOUPIST_PROVIDER_REQUEST_FILE;
const PROVIDER_API_KEY = process.env.LOUPIST_PROVIDER_API_KEY;
delete process.env.LOUPIST_PROVIDER_RESULT_FILE;
delete process.env.LOUPIST_PROVIDER_REQUEST_FILE;
delete process.env.LOUPIST_PROVIDER_API_KEY;

/**
 * The stable installation ID Pi keeps in its global settings (never a project
 * file). Pi queues the settings write, so wait for it before the sign-in starts.
 */
export async function installationDeviceId(agentDir = process.env.PI_CODING_AGENT_DIR) {
  if (!agentDir || !isAbsolute(agentDir)) throw new Error("Pi settings storage is unavailable");
  const settings = SettingsManager.create(process.cwd(), agentDir);
  const deviceId = settings.getOrCreateDeviceId();
  await settings.flush();
  return deviceId;
}

async function credentialRuntime() {
  const agentDir = process.env.PI_CODING_AGENT_DIR;
  if (!agentDir || !isAbsolute(agentDir)) {
    throw new Error("Pi credential storage is unavailable");
  }
  return ModelRuntime.create({
    authPath: join(agentDir, "auth.json"),
    modelsPath: null,
    refreshOnCreate: false,
  });
}

function writeResult(value) {
  if (!RESULT_FILE) throw new Error("Loupist provider result file is unavailable");
  const encoded = Buffer.from(JSON.stringify({ protocol: PROTOCOL, ...value }), "utf8");
  if (encoded.length === 0 || encoded.length > MAX_RESULT_BYTES) {
    throw new Error("Loupist provider result exceeded its size limit");
  }

  let descriptor;
  try {
    descriptor = openSync(RESULT_FILE, RESULT_OPEN_FLAGS);
    const metadata = fstatSync(descriptor);
    if (!metadata.isFile()) throw new Error("Loupist provider result file is invalid");
    if (IS_WINDOWS) ftruncateSync(descriptor, 0);
    writeExactSync(descriptor, encoded);
    fsyncSync(descriptor);
  } finally {
    if (descriptor !== undefined) closeSync(descriptor);
  }
}

export function writeExactSync(descriptor, encoded, write = writeSync) {
  let offset = 0;
  while (offset < encoded.length) {
    const written = write(descriptor, encoded, offset, encoded.length - offset, null);
    if (!Number.isInteger(written) || written <= 0) {
      throw new Error("Loupist provider result write made no progress");
    }
    offset += written;
  }
}

function emit(ctx, event) {
  ctx.ui.notify(`${PROTOCOL}:${JSON.stringify(event)}`, "info");
}

function modelsFor(ctx, providerId, live = null) {
  const known = ctx.modelRegistry.getAvailable().filter((model) => model.provider === providerId);
  // A live plan list keeps only what the plan offers; IDs Pi does not know
  // have no capability metadata and are left out. A list that shares no ID
  // with Pi's catalog tells nothing usable, so Pi's catalog stays.
  const offered = live && known.some((model) => live.has(model.id)) ? live : null;
  return known
    .filter((model) => !offered || offered.has(model.id))
    .map((model) => ({
      id: model.id,
      name: offered?.get(model.id) || model.name || model.id,
      reasoning: Boolean(model.reasoning),
      context_window: model.contextWindow,
      max_tokens: model.maxTokens,
      input: model.input ?? ["text"],
      thinking_levels: getSupportedThinkingLevels(model),
      thinking_level_map: model.thinkingLevelMap ?? {},
      run_controls: runControlsFor(model),
    }))
    .sort((left, right) => left.name.localeCompare(right.name));
}

/**
 * Sign in with ChatGPT lists the models a plan offers at `GET /v1/models`.
 * Reads the OpenAI list shape (`data[].id`) and, defensively, the plan form
 * (`slug`, `display_name`, `visibility`). Returns ID → display name (""
 * when the list names none), or null so the caller keeps Pi's catalog.
 */
export async function liveOpenAIModels(ctx, send = globalThis.fetch) {
  try {
    const auth = await ctx.modelRegistry.getProviderAuth("openai");
    const token = auth?.auth?.apiKey;
    if (auth?.source !== "OAuth" || typeof token !== "string" || !token) return null;
    const response = await send(LIVE_MODELS_URL, {
      headers: { authorization: `Bearer ${token}`, accept: "application/json" },
      // The plan token goes to this one address only, never to a redirect.
      redirect: "error",
      signal: AbortSignal.timeout(LIVE_MODELS_TIMEOUT_MS),
    });
    if (!response.ok) return null;
    const reader = response.body?.getReader();
    if (!reader) return null;
    const chunks = [];
    let byteLength = 0;
    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        byteLength += value.byteLength;
        if (byteLength > MAX_LIVE_MODELS_BYTES) {
          void reader.cancel().catch(() => {});
          return null;
        }
        chunks.push(value);
      }
    } finally {
      reader.releaseLock();
    }
    const text = Buffer.concat(chunks, byteLength).toString("utf8");
    const body = JSON.parse(text);
    const entries = Array.isArray(body?.data) ? body.data : Array.isArray(body?.models) ? body.models : null;
    if (!entries) return null;
    const live = new Map();
    for (const entry of entries) {
      if (!entry || typeof entry !== "object" || (entry.visibility ?? "list") !== "list") continue;
      const slug = typeof entry.slug === "string" ? entry.slug : entry.id;
      if (typeof slug !== "string" || !slug || slug.length > 128 || /[\s\u0000-\u001f]/u.test(slug)) continue;
      const name = typeof entry.display_name === "string" && entry.display_name.trim() && entry.display_name.length <= 80
        ? entry.display_name.trim()
        : "";
      live.set(slug, name);
    }
    return live.size ? live : null;
  } catch {
    return null;
  }
}

function requestUrl(input) {
  if (typeof input === "string") return input;
  if (input instanceof URL) return input.href;
  if (input && typeof input === "object" && typeof input.url === "string") return input.url;
  return "";
}

function formParams(body) {
  if (typeof body === "string") return new URLSearchParams(body);
  if (typeof URLSearchParams !== "undefined" && body instanceof URLSearchParams) {
    return new URLSearchParams(body);
  }
  if (typeof Buffer !== "undefined" && Buffer.isBuffer(body)) {
    return new URLSearchParams(body.toString("utf8"));
  }
  return null;
}

/**
 * Pi's xAI device login posts `referrer=pi`. Replace that one field with
 * Loupist. Token exchange and every other provider are left alone: Claude's
 * sign-in name belongs to Anthropic's published client, and API-key providers
 * send no sign-in name.
 */
export function withLoupistSignInIdentity(fetchImpl, providerId) {
  if (providerId !== "xai" || typeof fetchImpl !== "function") return fetchImpl;
  return async (input, init) => {
    if (requestUrl(input) === XAI_DEVICE_CODE_URL && init?.body != null) {
      const params = formParams(init.body);
      if (params?.get("referrer") === "pi") {
        params.set("referrer", LOUPIST_PROVIDER_NAME);
        init = { ...init, body: params };
      }
    }
    return fetchImpl(input, init);
  };
}

/** Fixed sign-in failure reasons; provider text never leaves this process. */
export function loginFailureReason(error) {
  const message = String(error?.message ?? error ?? "").toLowerCase();
  if (message.includes("chatgpt.tokens.use.direct")) return "plan_permission_declined";
  if (message.includes("not_eligible") || message.includes("not eligible")) return "plan_not_eligible";
  if (message.includes("access_denied")) return "denied";
  if (/port \d+ is in use/u.test(message) || message.includes("eaddrinuse")) return "port_in_use";
  if (message.includes("expired") || message.includes("timed out") || message.includes("timeout")) return "timed_out";
  if (/fetch failed|enotfound|eai_again|getaddrinfo|econnrefused|econnreset|network/u.test(message)) return "offline";
  return "other";
}

function validateProviderId(providerId) {
  if (!providerId || !/^[a-z0-9-]{1,64}$/.test(providerId)) {
    throw new Error("Provider ID is invalid");
  }
  return providerId;
}

function waitForBrowserCallback(signal) {
  if (!signal || typeof signal.addEventListener !== "function") {
    throw new Error("Pi browser login cancellation is unavailable");
  }
  const reason = () =>
    signal.reason instanceof Error
      ? signal.reason
      : new Error("Browser authorization completed");
  if (signal.aborted) return Promise.reject(reason());
  return new Promise((_, reject) => {
    signal.addEventListener("abort", () => reject(reason()), { once: true });
  });
}

function readSearchRequest() {
  if (!REQUEST_FILE) throw new Error("Loupist provider request file is unavailable");
  const encoded = readFileSync(REQUEST_FILE);
  if (encoded.length === 0 || encoded.length > MAX_REQUEST_BYTES) {
    throw new Error("Loupist provider request exceeded its size limit");
  }
  const request = JSON.parse(encoded.toString("utf8"));
  const allowedKeys = new Set([
    "protocol",
    "type",
    "provider",
    "query",
    "limit",
    "recency",
    "include_domains",
    "exclude_domains",
  ]);
  if (
    !request ||
    typeof request !== "object" ||
    Array.isArray(request) ||
    Object.keys(request).some((key) => !allowedKeys.has(key))
  ) {
    throw new Error("Loupist provider search request is invalid");
  }
  if (request.protocol !== PROTOCOL || request.type !== "search") {
    throw new Error("Loupist provider search request is invalid");
  }
  if (!isNativeSearchProvider(request.provider)) {
    throw new Error("Search provider is not allowlisted");
  }
  // Optional filters may be absent or JSON null at the Rust/JS boundary.
  const search = {
    provider: request.provider,
    query: request.query,
    limit: request.limit ?? 5,
    recency: request.recency ?? undefined,
    include_domains: request.include_domains ?? undefined,
    exclude_domains: request.exclude_domains ?? undefined,
  };
  validateSearchRequest(search);
  return search;
}

export async function resolveSearchAuth(ctx, providerId) {
  const provider = ctx.modelRegistry.getProvider(providerId);
  if (!provider) throw new NativeSearchError("unavailable", "Pi provider is unavailable");
  let auth;
  try {
    auth = await ctx.modelRegistry.getProviderAuth(providerId);
  } catch {
    throw new NativeSearchError("unavailable", "provider credentials are unavailable");
  }
  if (providerId === "xai" && auth?.source === "OAuth") {
    throw new NativeSearchError(
      "unavailable",
      "xAI native web search requires an xAI API key",
    );
  }
  if (!auth?.auth) throw new NativeSearchError("unavailable", "provider credentials are unavailable");
  return {
    apiKey: auth.auth.apiKey,
    headers: auth.auth.headers ?? {},
    baseUrl: auth.auth.baseUrl,
  };
}

function credentialCommand(args) {
  const parts = args.trim().split(/\s+/u);
  if (parts.length !== 2 || !["set", "clear"].includes(parts[1])) {
    throw new Error("Credential command is invalid");
  }
  return {
    providerId: validateProviderId(parts[0]),
    operation: parts[1],
  };
}

function writeCredentialUpdated(providerId) {
  writeResult({
    type: "credential_updated",
    provider: providerId,
    models: [],
  });
}

/**
 * Pi's paste prompt: the host answers it with a value Rust validated (the
 * ChatGPT loopback address or a Claude code), or the browser callback wins and
 * Pi aborts the prompt. Never reject before that abort: Pi's callback helper
 * cancels the callback when the manual prompt fails.
 */
async function manualCode(ctx, prompt) {
  const signal = prompt.signal;
  if (!signal || typeof signal.addEventListener !== "function") {
    throw new Error("Pi browser login cancellation is unavailable");
  }
  if (!signal.aborted && typeof ctx.ui.input === "function") {
    const value = await ctx.ui.input(MANUAL_CODE_TITLE, "", { signal });
    if (typeof value === "string" && value.length > 0) return value;
  }
  return waitForBrowserCallback(signal);
}

function oauthPrompt(ctx, prompt) {
  if (prompt.type === "select") {
    const browser = prompt.options.find((option) => option.id === "browser");
    if (!browser) throw new Error("Pi browser login is unavailable");
    return browser.id;
  }
  if (prompt.type === "manual_code") return manualCode(ctx, prompt);
  throw new Error("Unexpected OAuth prompt");
}

function oauthNotify(ctx, event) {
  if (event.type === "auth_url") {
    emit(ctx, {
      type: "authorization_url",
      url: event.url,
      instructions: event.instructions,
    });
    return;
  }
  if (event.type === "device_code") {
    if (typeof event.verificationUri !== "string" || event.verificationUri.length === 0) {
      throw new Error("Pi device login is missing a verification URL");
    }
    if (typeof event.userCode !== "string" || !/^[A-Z0-9-]{1,32}$/u.test(event.userCode)) {
      throw new Error("Pi device login returned an invalid code");
    }
    // The user types this code on the provider's page; Rust shows it and opens the page.
    emit(ctx, {
      type: "device_code",
      user_code: event.userCode,
      verification_uri: event.verificationUri,
      expires_in_s: Number.isSafeInteger(event.expiresInSeconds) && event.expiresInSeconds > 0
        ? event.expiresInSeconds
        : null,
    });
    return;
  }
  if (event.type === "progress" || event.type === "info") {
    emit(ctx, { type: "progress", message: event.message });
  }
}

export default function loupistTerminalProviderExtension(pi) {
  pi.registerCommand("loupist-provider-inference", {
    description: "Run one isolated, schema-checked model request through Pi",
    handler: async (args, ctx) => {
      let result;
      let diagnostic;
      try {
        if (!REQUEST_FILE) throw new Error("missing request");
        const encoded = readFileSync(REQUEST_FILE);
        if (!encoded.length || encoded.length > MAX_INFERENCE_REQUEST_BYTES) throw new Error("request size");
        result = await runProviderInference(ctx, JSON.parse(encoded.toString("utf8")), {
          onDispatch: () => writeResult({ type: "inference", disposition: "uncertain", body: { status: "in_flight" }, usage: null }),
          onDiagnostic: args.trim() === "eval-diagnostics" ? (event) => { diagnostic = event; } : undefined,
        });
      } catch {
        result = { disposition: "not_sent", body: { status: "not_sent", message: "The isolated model request is invalid." }, usage: null };
      }
      writeResult({ type: "inference", ...result, ...(diagnostic ? { diagnostic } : {}) });
    },
  });

  pi.registerCommand("loupist-provider-models", {
    description: "Return Pi's bundled model catalog for one provider",
    handler: async (args, ctx) => {
      const parts = args.trim().split(/\s+/u);
      if (parts.length > 2 || (parts.length === 2 && parts[1] !== "--live")) {
        throw new Error("Model catalog command is invalid");
      }
      const providerId = validateProviderId(parts[0]);
      const provider = ctx.modelRegistry.getProvider(providerId);
      if (!provider) throw new Error("Pi provider is unavailable");
      const live = parts[1] === "--live" && providerId === "openai" ? await liveOpenAIModels(ctx) : null;
      writeResult({ type: "models", provider: providerId, models: modelsFor(ctx, providerId, live) });
    },
  });

  pi.registerCommand("loupist-provider-api-key", {
    description: "Persist or clear one API-key credential through Pi's credential store",
    handler: async (args, ctx) => {
      const { providerId, operation } = credentialCommand(args);
      const provider = ctx.modelRegistry.getProvider(providerId);
      if (!provider?.auth?.apiKey) {
        throw new Error("Pi provider does not support API-key authentication");
      }
      const runtime = await credentialRuntime();
      runtime.registerNativeProvider(provider);
      if (operation === "clear") {
        await runtime.logout(providerId);
      } else {
        if (
          !PROVIDER_API_KEY ||
          Buffer.byteLength(PROVIDER_API_KEY, "utf8") > 8 * 1024 ||
          /[\u0000-\u001f\u007f]/u.test(PROVIDER_API_KEY)
        ) {
          throw new Error("Provider API key is invalid");
        }
        await runtime.login(providerId, "api_key", {
          prompt: async (prompt) => {
            if (prompt.type !== "secret" && prompt.type !== "text") {
              throw new Error("Pi provider requested unsupported API-key input");
            }
            return PROVIDER_API_KEY;
          },
          notify: () => undefined,
        });
      }
      writeCredentialUpdated(providerId);
    },
  });

  pi.registerCommand("loupist-provider-login", {
    description: "Connect one OAuth provider through Pi",
    handler: async (args, ctx) => {
      const providerId = validateProviderId(args.trim());
      const provider = ctx.modelRegistry.getProvider(providerId);
      if (!provider?.auth?.oauth) throw new Error("Pi provider does not support OAuth");

      const controller = new AbortController();
      const runtime = await credentialRuntime();
      runtime.registerNativeProvider(provider);
      const previousFetch = globalThis.fetch;
      globalThis.fetch = withLoupistSignInIdentity(previousFetch, providerId);
      try {
        // Sign in with ChatGPT names this installation by a stable ID kept in
        // Pi's private settings directory (`ext_agent_host_id`). Pi 1.0.1
        // ignores `agentName` and still sends its own name; Rust rewrites the
        // page it opens. A newer Pi uses this name directly.
        const loginOptions = {};
        if (providerId === "openai") {
          const deviceId = await installationDeviceId();
          loginOptions.getDeviceId = () => deviceId;
        }
        if (providerId === "openai" || providerId === "openai-codex") {
          loginOptions.agentName = LOUPIST_PROVIDER_NAME;
        }
        await runtime.login(providerId, "oauth", {
          signal: controller.signal,
          prompt: (prompt) => oauthPrompt(ctx, prompt),
          notify: (event) => oauthNotify(ctx, event),
        }, Object.keys(loginOptions).length > 0 ? loginOptions : undefined);
      } catch (error) {
        if (!/login cancelled/iu.test(String(error?.message ?? ""))) {
          emit(ctx, { type: "failed", reason: loginFailureReason(error) });
        }
        throw error;
      } finally {
        globalThis.fetch = previousFetch;
      }

      writeCredentialUpdated(providerId);
      emit(ctx, { type: "connected", message: `${provider.name} is connected.` });
    },
  });

  pi.registerCommand("loupist-provider-search", {
    description: "Run one bounded model-native web search through the current Pi credential store",
    handler: async (_args, ctx) => {
      const request = readSearchRequest();
      const secrets = [];
      try {
        const availableModelIds = ctx.modelRegistry.getAvailable()
          .filter((model) => model.provider === request.provider)
          .map((model) => model.id);
        const result = await runNativeSearch(request, {
          ctx,
          availableModelIds,
          resolveAuth: async (providerId) => {
            const auth = await resolveSearchAuth(ctx, providerId);
            if (auth.apiKey) secrets.push(auth.apiKey);
            for (const value of Object.values(auth.headers ?? {})) {
              if (typeof value === "string" && value.length > 8) secrets.push(value);
            }
            return auth;
          },
        });
        assertSearchResultSafe(result, secrets);
        writeResult(result);
      } catch (error) {
        const failure = toSearchErrorResult(request.provider, error);
        assertSearchResultSafe(failure, secrets);
        writeResult(failure);
      }
    },
  });

  pi.registerCommand("loupist-provider-logout", {
    description: "Clear one saved Pi credential for any cataloged provider",
    handler: async (args, ctx) => {
      const providerId = validateProviderId(args.trim());
      const provider = ctx.modelRegistry.getProvider(providerId);
      if (!provider) throw new Error("Pi provider is unavailable");
      const runtime = await credentialRuntime();
      runtime.registerNativeProvider(provider);
      await runtime.logout(providerId);
      writeCredentialUpdated(providerId);
    },
  });
}
