import assert from "node:assert/strict";
import { constants, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import { builtinProviders, getBuiltinModels } from "@earendil-works/pi-ai/providers/all";

test("keeps provider-result nofollow protection off Windows only", async () => {
  const moduleUrl = `${pathToFileURL(join(import.meta.dirname, "loupist-provider-extension.mjs"))}?nofollow=${Date.now()}`;
  const { resultOpenFlagsForPlatform } = await import(moduleUrl);

  assert.equal(resultOpenFlagsForPlatform("win32"), constants.O_WRONLY);
  assert.equal(
    resultOpenFlagsForPlatform("darwin"),
    constants.O_WRONLY | constants.O_TRUNC | (constants.O_NOFOLLOW ?? 0),
  );
});

test("provider result writes retry until every byte is published", async () => {
  const moduleUrl = `${pathToFileURL(join(import.meta.dirname, "loupist-provider-extension.mjs"))}?write=${Date.now()}`;
  const { writeExactSync } = await import(moduleUrl);
  const encoded = Buffer.from("bounded-result", "utf8");
  const published = [];
  writeExactSync(7, encoded, (descriptor, buffer, offset, length, position) => {
    assert.equal(descriptor, 7);
    assert.equal(position, null);
    const written = Math.min(2, length);
    published.push(buffer.subarray(offset, offset + written));
    return written;
  });
  assert.equal(Buffer.concat(published).toString("utf8"), "bounded-result");
  assert.throws(
    () => writeExactSync(7, encoded, () => 0),
    /result write made no progress/,
  );
});

test("inference command adds result diagnostics only for the explicit eval flag without changing requests", async (t) => {
  const temporary = mkdtempSync(join(tmpdir(), "loupist-inference-diagnostics-"));
  const resultPath = join(temporary, "result.json"); const requestPath = join(temporary, "request.json");
  const model = getBuiltinModels("openai-codex").find((entry) => entry.id === "gpt-6-luna");
  const request = { protocol: "loupist-provider/1", type: "inference", provider: model.provider, model: model.id, messages: [{ role: "user", content: "private fixture input" }], schema: { type: "object", properties: { claims: { type: "array", items: { type: "string" } } }, required: ["claims"], additionalProperties: false }, maxOutputTokens: 2048, timeoutMs: 1000, thinkingLevel: "max", runOptions: { performance: "standard" } };
  writeFileSync(resultPath, "", { mode: 0o600 }); writeFileSync(requestPath, JSON.stringify(request), { mode: 0o600 });
  process.env.LOUPIST_PROVIDER_RESULT_FILE = resultPath;
  process.env.LOUPIST_PROVIDER_REQUEST_FILE = requestPath;
  const originalFetch = globalThis.fetch; let fetches = 0;
  globalThis.fetch = async () => { fetches++; return new Response("fixture"); };
  t.after(() => {
    globalThis.fetch = originalFetch;
    delete process.env.LOUPIST_PROVIDER_RESULT_FILE; delete process.env.LOUPIST_PROVIDER_REQUEST_FILE;
    rmSync(temporary, { recursive: true, force: true });
  });
  const moduleUrl = `${pathToFileURL(join(import.meta.dirname, "loupist-provider-extension.mjs"))}?inference-diagnostics=${Date.now()}`;
  const { default: extension } = await import(moduleUrl); const commands = new Map();
  extension({ registerCommand: (name, command) => commands.set(name, command) });
  const payloads = []; const notices = []; let completionText = '{"claims":[]}';
  const context = { ui: { notify: (message) => notices.push(message) }, modelRegistry: {
    getAvailable: () => [model],
    streamSimple(selected, modelContext, options) {
      assert.equal(JSON.stringify(modelContext).includes("eval-diagnostics"), false);
      return { result: async () => {
        payloads.push(options.onPayload({ model: selected.id }));
        await options.fetch("https://fixture.invalid/response", {});
        return { provider: selected.provider, model: selected.id, stopReason: "stop", content: [{ type: "text", text: completionText }], usage: { input: 2, output: 1 } };
      } };
    },
  } };
  const results = []; const encodedResults = [];
  for (const args of ["", "eval-diagnostics", "unrecognized"]) {
    await commands.get("loupist-provider-inference").handler(args, context);
    const encoded = readFileSync(resultPath, "utf8");
    encodedResults.push(encoded); results.push(JSON.parse(encoded));
  }
  assert.equal(fetches, 3); assert.equal(notices.length, 0);
  const { diagnostic, ...ordinaryResult } = results[1];
  assert.deepEqual(results[0], ordinaryResult); assert.deepEqual(ordinaryResult, results[2]);
  assert.equal(encodedResults[0], encodedResults[2]);
  assert.equal(encodedResults[0], JSON.stringify({ protocol: "loupist-provider/1", type: "inference", disposition: "completed", body: { claims: [] }, usage: { input: 2, output: 1 } }));
  assert.deepEqual(payloads[0], payloads[1]); assert.deepEqual(payloads[1], payloads[2]);
  assert.equal(diagnostic.type, "inference_diagnostic"); assert.equal(diagnostic.phase, "complete");
  assert.equal(JSON.stringify(diagnostic).includes("private"), false);
  assert.deepEqual(JSON.parse(readFileSync(requestPath, "utf8")), request);
  completionText = "private invalid JSON";
  await commands.get("loupist-provider-inference").handler("", context);
  const ordinaryFailure = JSON.parse(readFileSync(resultPath, "utf8"));
  await commands.get("loupist-provider-inference").handler("eval-diagnostics", context);
  const { diagnostic: failure, ...observedFailure } = JSON.parse(readFileSync(resultPath, "utf8"));
  assert.deepEqual(observedFailure, ordinaryFailure);
  assert.equal(failure.failure_kind, "invalid_json"); assert.equal(failure.phase, "json");
  assert.equal(JSON.stringify(failure).includes("private"), false);
  assert.equal(fetches, 5); assert.equal(notices.length, 0);
});

test("xAI subscription OAuth skips native search without exposing the credential", async () => {
  const moduleUrl = `${pathToFileURL(join(import.meta.dirname, "loupist-provider-extension.mjs"))}?xai-oauth=${Date.now()}`;
  const { resolveSearchAuth } = await import(moduleUrl);
  await assert.rejects(
    () =>
      resolveSearchAuth(
        {
          modelRegistry: {
            getProvider: () => ({ id: "xai" }),
            getProviderAuth: async () => ({
              source: "OAuth",
              auth: { apiKey: "xai-oauth-redacted-test" },
            }),
          },
        },
        "xai",
      ),
    (error) => {
      assert.equal(error.kind, "unavailable");
      assert.equal(error.message.includes("xai-oauth-redacted-test"), false);
      return true;
    },
  );
});

test("persists credentials through Pi storage without returning secrets", async (t) => {
  const temporary = mkdtempSync(join(tmpdir(), "loupist-provider-extension-"));
  const resultPath = join(temporary, "result.json");
  writeFileSync(resultPath, "stale result".repeat(8_192), { mode: 0o600 });
  process.env.PI_CODING_AGENT_DIR = temporary;
  process.env.LOUPIST_PROVIDER_RESULT_FILE = resultPath;
  t.after(() => {
    delete process.env.PI_CODING_AGENT_DIR;
    delete process.env.LOUPIST_PROVIDER_RESULT_FILE;
    rmSync(temporary, { recursive: true, force: true });
  });

  const moduleUrl = `${pathToFileURL(join(import.meta.dirname, "loupist-provider-extension.mjs"))}?test=${Date.now()}`;
  const { default: extension, API_KEY_TITLE } = await import(moduleUrl);
  const commands = new Map();
  extension({
    registerCommand: (name, command) => commands.set(name, command),
  });
  assert.deepEqual([...commands.keys()].sort(), [
    "loupist-provider-api-key",
    "loupist-provider-inference",
    "loupist-provider-login",
    "loupist-provider-logout",
    "loupist-provider-models",
    "loupist-provider-search",
  ]);
  assert.equal(process.env.LOUPIST_PROVIDER_RESULT_FILE, undefined);
  assert.equal(process.env.LOUPIST_PROVIDER_REQUEST_FILE, undefined);

  const notices = [];
  const models = [
    {
      provider: "openai-codex",
      id: "gpt-test",
      name: "GPT Test",
      reasoning: true,
      contextWindow: 128_000,
      maxTokens: 32_000,
      input: ["text", "image"],
      thinkingLevelMap: { off: "off", xhigh: "xhigh", max: null },
      api: "openai-codex-responses",
    },
    {
      provider: "openai-codex",
      id: "gpt-unavailable",
      name: "Unavailable GPT",
      reasoning: true,
      contextWindow: 128_000,
      maxTokens: 32_000,
    },
    {
      provider: "openai-codex",
      id: "gpt-always-thinking",
      name: "Always Thinking GPT",
      reasoning: true,
      contextWindow: 128_000,
      maxTokens: 32_000,
      input: ["text"],
      thinkingLevelMap: {
        off: null,
        minimal: null,
        low: null,
        medium: null,
        high: "high",
        xhigh: null,
        max: "max",
      },
      api: "openai-codex-responses",
    },
  ];
  const oauthProvider = {
    id: "openai-codex",
    name: "OpenAI with ChatGPT",
    auth: {
      oauth: {
        login: async (interaction, options) => {
          assert.equal(options.agentName, "Loupist");
          assert.equal(
            await interaction.prompt({
              type: "select",
              options: [
                { id: "browser", label: "Browser" },
                { id: "device_code", label: "Device code" },
              ],
            }),
            "browser",
          );
          const manualPrompt = new AbortController();
          interaction.notify({
            type: "auth_url",
            url: "https://auth.openai.com/oauth/authorize?client_id=test",
            instructions: "Complete login in your browser.",
          });
          const callbackWait = interaction.prompt({
            type: "manual_code",
            signal: manualPrompt.signal,
          });
          manualPrompt.abort();
          await assert.rejects(callbackWait, { name: "AbortError" });
          return {
            type: "oauth",
            access: "access-secret",
            refresh: "refresh-secret",
            expires: 1_900_000_000_000,
          };
        },
        refresh: async (credential) => credential,
        toAuth: async (credential) => ({ auth: { apiKey: credential.access } }),
      },
    },
    getModels: () => models,
    stream: () => {
      throw new Error("unused in provider setup test");
    },
    streamSimple: () => {
      throw new Error("unused in provider setup test");
    },
  };
  const apiKeyProvider = {
    id: "anthropic",
    name: "Anthropic",
    auth: {
      apiKey: {
        name: "API key",
        login: async (interaction) => ({
          type: "api_key",
          key: await interaction.prompt({ type: "secret", message: "API key" }),
        }),
        resolve: async ({ credential }) =>
          credential ? { auth: { apiKey: credential.key } } : undefined,
      },
    },
    getModels: () => [],
    stream: () => {
      throw new Error("unused in provider setup test");
    },
    streamSimple: () => {
      throw new Error("unused in provider setup test");
    },
  };
  const context = {
    modelRegistry: {
      getAll: () => models,
      getAvailable: () => [models[0], models[2]],
      getProvider: (providerId) =>
        providerId === "openai-codex" ? oauthProvider : apiKeyProvider,
    },
    ui: {
      notify: (message) => notices.push(message),
      input: async (title, placeholder, options) => {
        if (title === "loupist-provider/1:manual_code") return undefined;
        assert.equal(title, API_KEY_TITLE);
        assert.equal(placeholder, "");
        assert.equal(options.timeout, 10_000);
        return "api-key-secret";
      },
    },
  };

  await commands.get("loupist-provider-login").handler("openai-codex", context);
  const result = JSON.parse(readFileSync(resultPath, "utf8"));
  assert.equal(result.protocol, "loupist-provider/1");
  assert.equal(result.type, "credential_updated");
  assert.equal("credential" in result, false);
  assert.deepEqual(result.models, []);

  await commands
    .get("loupist-provider-models")
    .handler("openai-codex", context);
  const modelResult = JSON.parse(readFileSync(resultPath, "utf8"));
  const standardModel = modelResult.models.find((model) => model.id === "gpt-test");
  assert.deepEqual(standardModel.thinking_levels, ["off", "minimal", "low", "medium", "high", "xhigh"]);
  assert.equal(standardModel.thinking_level_map.max, null);
  assert.deepEqual(
    standardModel.run_controls.map((control) => [control.id, control.choices.map((choice) => choice.id)]),
    [["performance", ["standard", "fast"]]],
  );
  assert.deepEqual(
    modelResult.models.find((model) => model.id === "gpt-always-thinking").thinking_levels,
    ["high", "max"],
  );
  assert.equal(modelResult.models.some((model) => model.id === "gpt-unavailable"), false);

  await commands
    .get("loupist-provider-api-key")
    .handler("anthropic set", context);
  const saved = JSON.parse(readFileSync(join(temporary, "auth.json"), "utf8"));
  assert.equal(saved["openai-codex"].access, "access-secret");
  assert.equal(saved.anthropic.key, "api-key-secret");
  const mutationResult = JSON.parse(readFileSync(resultPath, "utf8"));
  assert.equal(mutationResult.type, "credential_updated");
  assert.equal("credential" in mutationResult, false);

  await commands
    .get("loupist-provider-api-key")
    .handler("anthropic clear", context);
  const cleared = JSON.parse(readFileSync(join(temporary, "auth.json"), "utf8"));
  assert.equal(cleared.anthropic, undefined);
  assert.equal(cleared["openai-codex"].refresh, "refresh-secret");

  await commands.get("loupist-provider-logout").handler("openai-codex", context);
  const loggedOut = JSON.parse(readFileSync(join(temporary, "auth.json"), "utf8"));
  assert.equal(loggedOut["openai-codex"], undefined);
  const logoutResult = JSON.parse(readFileSync(resultPath, "utf8"));
  assert.equal(logoutResult.type, "credential_updated");
  assert.equal(logoutResult.provider, "openai-codex");
  assert.equal("credential" in logoutResult, false);
  assert.equal(
    notices.some((notice) => notice.includes("auth.openai.com/oauth/authorize")),
    true,
  );
  assert.equal(notices.some((notice) => notice.includes("device_code")), false);
  assert.equal(notices.some((notice) => notice.includes("access-secret")), false);
  assert.equal(notices.some((notice) => notice.includes("refresh-secret")), false);
});

test("xAI device login is the only sign-in whose posted name is rewritten", async () => {
  const { withLoupistSignInIdentity } = await import(
    `${pathToFileURL(join(import.meta.dirname, "loupist-provider-extension.mjs"))}?identity=${Date.now()}`
  );
  const calls = [];
  const fetchImpl = async (url, init) => {
    calls.push(init.body instanceof URLSearchParams ? init.body.toString() : String(init.body));
    return new Response("{}", { status: 200 });
  };
  const xai = withLoupistSignInIdentity(fetchImpl, "xai");
  await xai("https://auth.x.ai/oauth2/device/code", {
    body: Buffer.from("client_id=client&referrer=pi"),
  });
  await xai("https://auth.x.ai/oauth2/device/code?extra=1", {
    body: new URLSearchParams({ referrer: "pi" }),
  });
  await xai("https://example.com/oauth2/device/code", {
    body: new URLSearchParams({ referrer: "pi" }),
  });
  assert.match(calls[0], /(?:^|&)referrer=Loupist(?:&|$)/u);
  assert.match(calls[0], /client_id=client/u);
  assert.equal(calls[1], "referrer=pi");
  assert.equal(calls[2], "referrer=pi");
  assert.equal(withLoupistSignInIdentity(fetchImpl, "anthropic"), fetchImpl);
  assert.equal(withLoupistSignInIdentity(fetchImpl, "openai"), fetchImpl);
});

test("forwards a device sign-in's user code and verification URL", async (t) => {
  const temporary = mkdtempSync(join(tmpdir(), "loupist-provider-device-"));
  const resultPath = join(temporary, "result.json");
  writeFileSync(resultPath, "", { mode: 0o600 });
  process.env.PI_CODING_AGENT_DIR = temporary;
  process.env.LOUPIST_PROVIDER_RESULT_FILE = resultPath;
  t.after(() => {
    delete process.env.PI_CODING_AGENT_DIR;
    delete process.env.LOUPIST_PROVIDER_RESULT_FILE;
    rmSync(temporary, { recursive: true, force: true });
  });

  const moduleUrl = `${pathToFileURL(join(import.meta.dirname, "loupist-provider-extension.mjs"))}?device=${Date.now()}`;
  const { default: extension } = await import(moduleUrl);
  const commands = new Map();
  extension({
    registerCommand: (name, command) => commands.set(name, command),
  });

  const notices = [];
  const seen = [];
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    seen.push({
      url: String(url),
      body: init?.body instanceof URLSearchParams ? init.body.toString() : String(init?.body ?? ""),
    });
    return new Response("{}", { status: 200 });
  };
  t.after(() => {
    globalThis.fetch = originalFetch;
  });
  const deviceProvider = {
    id: "xai",
    name: "xAI",
    auth: {
      oauth: {
        login: async (interaction) => {
          await fetch("https://auth.x.ai/oauth2/device/code", {
            method: "POST",
            body: new URLSearchParams({ client_id: "client", referrer: "pi", scope: "openid" }),
          });
          await fetch("https://auth.x.ai/oauth2/token", {
            method: "POST",
            body: "grant_type=refresh_token&referrer=pi",
          });
          interaction.notify({
            type: "device_code",
            userCode: "WXYZ-1234",
            verificationUri: "https://accounts.x.ai/oauth2/device?user_code=WXYZ-1234",
            expiresInSeconds: 900,
          });
          return {
            type: "oauth",
            access: "xai-access",
            refresh: "xai-refresh",
            expires: 1_900_000_000_000,
          };
        },
        refresh: async (credential) => credential,
        toAuth: async (credential) => ({ auth: { apiKey: credential.access } }),
      },
    },
    getModels: () => [],
    stream: () => {
      throw new Error("unused in provider setup test");
    },
    streamSimple: () => {
      throw new Error("unused in provider setup test");
    },
  };
  const context = {
    modelRegistry: {
      getProvider: () => deviceProvider,
    },
    ui: {
      notify: (message) => notices.push(message),
    },
  };

  await commands.get("loupist-provider-login").handler("xai", context);
  await fetch("https://auth.x.ai/oauth2/device/code", {
    method: "POST",
    body: new URLSearchParams({ referrer: "pi" }),
  });
  assert.equal(seen[0].url, "https://auth.x.ai/oauth2/device/code");
  assert.match(seen[0].body, /(?:^|&)referrer=Loupist(?:&|$)/u);
  assert.doesNotMatch(seen[0].body, /(?:^|&)referrer=pi(?:&|$)/u);
  assert.match(seen[0].body, /client_id=client/u);
  assert.equal(seen[1].body, "grant_type=refresh_token&referrer=pi");
  assert.match(seen[2].body, /(?:^|&)referrer=pi(?:&|$)/u);
  const result = JSON.parse(readFileSync(resultPath, "utf8"));
  assert.equal(result.type, "credential_updated");
  assert.equal(result.provider, "xai");
  assert.equal("credential" in result, false);
  assert.equal(
    notices.some((notice) =>
      notice.includes("https://accounts.x.ai/oauth2/device?user_code=WXYZ-1234"),
    ),
    true,
  );
  const device = notices
    .map((notice) => JSON.parse(notice.slice("loupist-provider/1:".length)))
    .find((event) => event.type === "device_code");
  assert.deepEqual(device, {
    type: "device_code",
    user_code: "WXYZ-1234",
    verification_uri: "https://accounts.x.ai/oauth2/device?user_code=WXYZ-1234",
    expires_in_s: 900,
  });
  assert.equal(notices.some((notice) => notice.includes("\"type\":\"authorization_url\"")), false);
  assert.equal(notices.some((notice) => notice.includes("xai-access")), false);
});

async function loadExtension(t, name) {
  const temporary = mkdtempSync(join(tmpdir(), `loupist-provider-${name}-`));
  const resultPath = join(temporary, "result.json");
  writeFileSync(resultPath, "", { mode: 0o600 });
  process.env.PI_CODING_AGENT_DIR = temporary;
  process.env.LOUPIST_PROVIDER_RESULT_FILE = resultPath;
  t.after(() => {
    delete process.env.PI_CODING_AGENT_DIR;
    delete process.env.LOUPIST_PROVIDER_RESULT_FILE;
    rmSync(temporary, { recursive: true, force: true });
  });
  const moduleUrl = `${pathToFileURL(join(import.meta.dirname, "loupist-provider-extension.mjs"))}?${name}=${Date.now()}`;
  const module = await import(moduleUrl);
  const commands = new Map();
  module.default({ registerCommand: (commandName, command) => commands.set(commandName, command) });
  return { module, commands, temporary, resultPath };
}

test("API-key input uses only the fixed secret prompt and sanitizes provider errors", async (t) => {
  const { module, commands, temporary, resultPath } = await loadExtension(t, "key-input");
  const notices = [];
  const prompts = [];
  const secret = "private-input-fixture";
  let promptType = "secret";
  let failAfterInput = false;
  const provider = {
    id: "anthropic", name: "Anthropic",
    auth: { apiKey: {
      login: async (interaction) => {
        const key = await interaction.prompt({ type: promptType, message: "untrusted title" });
        interaction.notify({ type: "info", message: key });
        if (failAfterInput) throw new Error(key);
        return { type: "api_key", key };
      },
      resolve: async ({ credential }) => credential ? { auth: { apiKey: credential.key } } : undefined,
    } },
    getModels: () => [],
  };
  const context = {
    modelRegistry: { getProvider: () => provider },
    ui: {
      notify: (message) => notices.push(message),
      input: async (title, placeholder, options) => {
        prompts.push(title);
        assert.equal(title, module.API_KEY_TITLE);
        assert.equal(title, "loupist-provider/1:api_key");
        assert.equal(placeholder, "");
        assert.equal(options.timeout, 10_000);
        return secret;
      },
    },
  };
  const command = commands.get("loupist-provider-api-key");
  for (const type of ["text", "select", "manual_code"]) {
    promptType = type;
    await assert.rejects(() => command.handler("anthropic set", context), { message: "Provider API key could not be saved" });
  }
  assert.deepEqual(prompts, []);
  assert.equal(readFileSync(resultPath, "utf8"), "");
  promptType = "secret";
  failAfterInput = true;
  await assert.rejects(() => command.handler("anthropic set", context), { message: "Provider API key could not be saved" });
  assert.equal(readFileSync(resultPath, "utf8"), "");
  const authAfterFailure = JSON.parse(readFileSync(join(temporary, "auth.json"), "utf8"));
  assert.equal(authAfterFailure.anthropic, undefined);
  failAfterInput = false;
  await command.handler("anthropic set", context);
  assert.equal(JSON.parse(readFileSync(join(temporary, "auth.json"), "utf8")).anthropic.key, secret);
  assert.equal(readFileSync(resultPath, "utf8").includes(secret), false);
  assert.deepEqual(notices, []);
});

test("cancelled or timed out API-key prompts save no credential and retain no previous answer", async (t) => {
  const { commands, temporary, resultPath } = await loadExtension(t, "cancelled-key");
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let promptCount = 0;
  let expire;
  const provider = {
    id: "anthropic", name: "Anthropic",
    auth: { apiKey: {
      login: async (interaction) => ({ type: "api_key", key: await interaction.prompt({ type: "secret" }) }),
      resolve: async () => undefined,
    } },
    getModels: () => [],
  };
  const context = {
    modelRegistry: { getProvider: () => provider },
    ui: { input: async (_title, _placeholder, options) => {
      promptCount++;
      if (promptCount === 1) return undefined; // Pi's cancelled answer.
      return new Promise((resolve) => {
        setTimeout(() => resolve(undefined), options.timeout);
        expire();
      });
    } },
  };
  const command = commands.get("loupist-provider-api-key");
  await assert.rejects(() => command.handler("anthropic set", context), { message: "Provider API key could not be saved" });
  const prompted = new Promise((resolve) => { expire = resolve; });
  const timedOut = assert.rejects(() => command.handler("anthropic set", context), { message: "Provider API key could not be saved" });
  await prompted;
  t.mock.timers.tick(10_000);
  await timedOut;
  assert.equal(promptCount, 2);
  assert.equal(JSON.parse(readFileSync(join(temporary, "auth.json"), "utf8")).anthropic, undefined);
  assert.equal(readFileSync(resultPath, "utf8"), "");
  await command.handler("anthropic clear", context);
  assert.equal(promptCount, 2, "clearing a credential never requests a key");
});

function oauthOnlyProvider(id, login) {
  return {
    id,
    name: id,
    auth: {
      oauth: {
        login,
        refresh: async (credential) => credential,
        toAuth: async (credential) => ({ auth: { apiKey: credential.access } }),
      },
    },
    getModels: () => [],
    stream: () => { throw new Error("unused"); },
    streamSimple: () => { throw new Error("unused"); },
  };
}

const credential = { type: "oauth", access: "access-fixture", refresh: "refresh-fixture", expires: 1_900_000_000_000 };

test("the host answers Pi's paste prompt, and the browser callback can still win", async (t) => {
  const { commands } = await loadExtension(t, "manual-code");
  const pasted = "http://127.0.0.1:1455/auth/callback?code=fixture&state=fixture";
  const inputs = [];
  let received;
  const login = async (interaction) => {
    received = await interaction.prompt({ type: "manual_code", signal: new AbortController().signal });
    // A second login where the browser callback arrives first: Pi aborts the prompt.
    const callbackWon = new AbortController();
    const waiting = interaction.prompt({ type: "manual_code", signal: callbackWon.signal });
    callbackWon.abort();
    await assert.rejects(waiting, { name: "AbortError" });
    return credential;
  };
  const context = {
    modelRegistry: { getProvider: () => oauthOnlyProvider("openai", login) },
    ui: {
      notify: () => {},
      input: (title, placeholder, options) => {
        inputs.push({ title, placeholder });
        if (inputs.length === 1) return Promise.resolve(pasted);
        return new Promise((resolve) => options.signal.addEventListener("abort", () => resolve(undefined), { once: true }));
      },
    },
  };
  await commands.get("loupist-provider-login").handler("openai", context);
  assert.equal(received, pasted);
  assert.deepEqual(inputs.map((input) => input.title), ["loupist-provider/1:manual_code", "loupist-provider/1:manual_code"]);
});

test("a failed sign-in reports one fixed reason before the command fails", async (t) => {
  const { commands } = await loadExtension(t, "failed-reason");
  const notices = [];
  const login = async () => {
    throw new Error("OpenAI OAuth grant did not include chatgpt.tokens.use.direct (account acct-private)");
  };
  const context = {
    modelRegistry: { getProvider: () => oauthOnlyProvider("openai", login) },
    ui: { notify: (message) => notices.push(message) },
  };
  await assert.rejects(() => commands.get("loupist-provider-login").handler("openai", context));
  assert.deepEqual(notices.map((notice) => JSON.parse(notice.slice("loupist-provider/1:".length))), [
    { type: "failed", reason: "plan_permission_declined" },
  ]);
  assert.equal(notices.some((notice) => notice.includes("acct-private")), false);
});

test("sign-in failures map to fixed reasons", async () => {
  const { loginFailureReason } = await import(`${pathToFileURL(join(import.meta.dirname, "loupist-provider-extension.mjs"))}?reasons=${Date.now()}`);
  for (const [message, reason] of [
    ["OpenAI OAuth grant did not include chatgpt.tokens.use.direct", "plan_permission_declined"],
    ["403 subscription_sharing_user_not_eligible", "plan_not_eligible"],
    ["ChatGPT authorization failed: access_denied", "denied"],
    ["Port 1455 is in use, probably by an unfinished login", "port_in_use"],
    ["xAI device code expired", "timed_out"],
    ["fetch failed: getaddrinfo ENOTFOUND auth.openai.com", "offline"],
    ["OAuth state mismatch", "other"],
  ]) {
    assert.equal(loginFailureReason(new Error(message)), reason, message);
  }
});

test("Pi's own OpenAI sign-in pages name Loupist from agentName", async () => {
  // The real Pi flows, stopped at the page they would open: no network request.
  const signInPage = async (providerId, method) => {
    const controller = new AbortController();
    let page;
    const interaction = {
      signal: controller.signal,
      notify: (event) => {
        if (event.type === "auth_url") {
          page = event.url;
          controller.abort();
        }
      },
      prompt: (request) => request.type === "select" ? Promise.resolve(method) : new Promise((_, reject) => {
        const cancel = () => reject(new Error("Login cancelled"));
        if (controller.signal.aborted) cancel();
        else controller.signal.addEventListener("abort", cancel, { once: true });
      }),
    };
    const oauth = builtinProviders().find((provider) => provider.id === providerId).auth.oauth;
    await assert.rejects(oauth.login(interaction, {
      agentName: "Loupist",
      getDeviceId: () => "11111111-2222-3333-4444-555555555555",
    }), /Login cancelled/u);
    return new URL(page);
  };
  const chatgpt = await signInPage("openai");
  assert.equal(chatgpt.host, "auth.openai.com");
  assert.equal(chatgpt.searchParams.get("agent_name_hint"), "Loupist");
  const codex = await signInPage("openai-codex", "browser");
  assert.equal(codex.host, "auth.openai.com");
  assert.equal(codex.searchParams.get("originator"), "Loupist");
});

test("Sign in with ChatGPT receives a stable installation ID from Pi's settings", async (t) => {
  const { commands, temporary } = await loadExtension(t, "device-id");
  const ids = [];
  const login = async (_interaction, options) => {
    ids.push(options.getDeviceId());
    assert.equal(options.agentName, "Loupist");
    return credential;
  };
  const context = {
    modelRegistry: { getProvider: () => oauthOnlyProvider("openai", login) },
    ui: { notify: () => {} },
  };
  await commands.get("loupist-provider-login").handler("openai", context);
  await commands.get("loupist-provider-login").handler("openai", context);
  assert.match(ids[0], /^[0-9a-f-]{36}$/u);
  assert.equal(ids[1], ids[0]);
  const settings = JSON.parse(readFileSync(join(temporary, "settings.json"), "utf8"));
  assert.equal(settings.deviceId, ids[0]);
});

test("a ChatGPT sign-in lists only the models its plan offers", async (t) => {
  const { module, commands, resultPath } = await loadExtension(t, "live-models");
  const known = (id, name) => ({ provider: "openai", id, name, reasoning: true, contextWindow: 272_000, maxTokens: 128_000, input: ["text"], api: "openai-responses" });
  const available = [known("gpt-6.1-sol", "GPT-6.1 Sol"), known("gpt-5.6-luna", "GPT-5.6 Luna"), known("gpt-4.1-mini", "GPT-4.1 mini")];
  const requests = [];
  const originalFetch = globalThis.fetch;
  // The OpenAI list shape: IDs only, no display names.
  let body = { object: "list", data: [
    { id: "gpt-6.1-sol", object: "model", created: 1, owned_by: "openai" },
    { id: "gpt-unknown", object: "model", created: 1, owned_by: "openai" },
  ] };
  globalThis.fetch = async (url, init) => {
    requests.push({ url, authorization: init.headers.authorization, redirect: init.redirect });
    return new Response(JSON.stringify(body), { status: 200 });
  };
  t.after(() => { globalThis.fetch = originalFetch; });
  const context = {
    modelRegistry: {
      getAvailable: () => available,
      getProvider: () => ({ id: "openai" }),
      getProviderAuth: async () => ({ source: "OAuth", auth: { apiKey: "plan-token" } }),
    },
    ui: { notify: () => {} },
  };
  const listed = async () => {
    await commands.get("loupist-provider-models").handler("openai --live", context);
    return JSON.parse(readFileSync(resultPath, "utf8")).models.map((model) => [model.id, model.name]);
  };
  assert.deepEqual(await listed(), [["gpt-6.1-sol", "GPT-6.1 Sol"]]);
  assert.deepEqual(requests, [{ url: "https://api.openai.com/v1/models", authorization: "Bearer plan-token", redirect: "error" }]);
  assert.equal(readFileSync(resultPath, "utf8").includes("plan-token"), false);
  // The plan form names models and hides some.
  body = { models: [
    { slug: "gpt-6.1-sol", display_name: "GPT-6.1 Sol (plan)", visibility: "list" },
    { slug: "gpt-5.6-luna", display_name: "GPT-5.6 Luna", visibility: "hide" },
  ] };
  assert.deepEqual(await listed(), [["gpt-6.1-sol", "GPT-6.1 Sol (plan)"]]);
  // A list that shares no ID with Pi's catalog keeps Pi's catalog.
  body = { data: [{ id: "gpt-unknown" }, { id: "gpt-other" }] };
  assert.equal((await listed()).length, 3);
  // A failed live call keeps Pi's catalog.
  body = { error: "nope" };
  assert.equal((await listed()).length, 3);
  // An API key never makes the live call.
  context.modelRegistry.getProviderAuth = async () => ({ source: "stored", auth: { apiKey: "sk-fixture" } });
  assert.equal(await module.liveOpenAIModels(context), null);
  assert.equal(requests.length, 4);
  await assert.rejects(() => commands.get("loupist-provider-models").handler("openai --other", context), /invalid/);
});

test("live ChatGPT model lists enforce the response byte limit while streaming", async (t) => {
  const { liveOpenAIModels } = await import("./loupist-provider-extension.mjs");
  const context = {
    modelRegistry: {
      getProviderAuth: async () => ({ source: "OAuth", auth: { apiKey: "plan-token" } }),
    },
  };
  const maxBytes = 1024 * 1024;

  await t.test("cancels at the first chunk over the limit", async () => {
    let pulls = 0;
    let cancellations = 0;
    const stream = new ReadableStream({
      pull(controller) {
        pulls++;
        controller.enqueue(new Uint8Array(maxBytes));
        if (pulls === 4) controller.close();
      },
      cancel() { cancellations++; },
    }, { highWaterMark: 0 });

    assert.equal(await liveOpenAIModels(context, async () => new Response(stream)), null);
    assert.equal(pulls, 2);
    assert.equal(cancellations, 1);
  });

  await t.test("falls back without waiting for cancellation to settle", async () => {
    let cancelled = false;
    const stream = new ReadableStream({
      pull(controller) { controller.enqueue(new Uint8Array(maxBytes + 1)); },
      cancel() { cancelled = true; return new Promise(() => {}); },
    }, { highWaterMark: 0 });
    let timer;
    try {
      const result = await Promise.race([
        liveOpenAIModels(context, async () => new Response(stream)),
        new Promise(resolve => { timer = setTimeout(() => resolve("pending"), 1000); }),
      ]);
      assert.equal(result, null);
      assert.equal(cancelled, true);
    } finally {
      clearTimeout(timer);
    }
  });

  await t.test("accepts exactly one MiB including split multibyte text", async () => {
    const body = JSON.stringify({ data: [{ id: "gpt-boundary", display_name: "한국" }] });
    const encoded = Buffer.from(body + " ".repeat(maxBytes - Buffer.byteLength(body)));
    const split = encoded.indexOf(Buffer.from("한국")) + 1;
    let pulls = 0;
    const chunks = [encoded.subarray(0, split), encoded.subarray(split)];
    const stream = new ReadableStream({
      pull(controller) {
        controller.enqueue(chunks[pulls++]);
        if (pulls === chunks.length) controller.close();
      },
    }, { highWaterMark: 0 });

    assert.deepEqual(
      await liveOpenAIModels(context, async () => new Response(stream)),
      new Map([["gpt-boundary", "한국"]]),
    );
    assert.equal(
      await liveOpenAIModels(context, async () => new Response(Buffer.concat([encoded, Buffer.from(" ")]))),
      null,
    );
  });

  await t.test("keeps unnamed models and falls back for unusable responses", async () => {
    assert.deepEqual(
      await liveOpenAIModels(context, async () => new Response('{"data":[{"id":"gpt-valid"}]}')),
      new Map([["gpt-valid", ""]]),
    );
    for (const response of [
      new Response("malformed JSON"),
      new Response('{"data":[]}'),
      new Response(null),
      new Response("unavailable", { status: 503 }),
    ]) {
      assert.equal(await liveOpenAIModels(context, async () => response), null);
    }
    assert.equal(await liveOpenAIModels(context, async () => { throw new Error("network failed"); }), null);
  });

  await t.test("falls back when a stream fails after delivering valid JSON", async () => {
    let pulls = 0;
    const stream = new ReadableStream({
      pull(controller) {
        if (pulls++ === 0) controller.enqueue(Buffer.from('{"data":[{"id":"gpt-valid"}]}'));
        else controller.error(new Error("stream failed"));
      },
    }, { highWaterMark: 0 });

    assert.equal(await liveOpenAIModels(context, async () => new Response(stream)), null);
    assert.equal(pulls, 2);
  });
});

test("search command runs through Pi and publishes only sanitized sources", async (t) => {
  const temporary = mkdtempSync(join(tmpdir(), "loupist-provider-search-"));
  const resultPath = join(temporary, "result.json");
  const requestPath = join(temporary, "request.json");
  const request = {
    protocol: "loupist-provider/1",
    type: "search",
    provider: "anthropic",
    query: "latest public report",
    limit: 3,
    recency: null,
    include_domains: null,
    exclude_domains: null,
  };
  writeFileSync(resultPath, "", { mode: 0o600 });
  writeFileSync(requestPath, JSON.stringify(request), { mode: 0o600 });
  process.env.LOUPIST_PROVIDER_RESULT_FILE = resultPath;
  process.env.LOUPIST_PROVIDER_REQUEST_FILE = requestPath;
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async () => Response.json({
    id: "msg_test",
    model: "claude-haiku-4-5",
    content: [
      { type: "server_tool_use", name: "web_search" },
      { type: "web_search_tool_result", content: [{ type: "web_search_result", title: "UN report", url: "https://example.org/un-report" }] },
      { type: "text", text: "Anthropic synthesized answer", citations: [{ url: "https://example.org/un-report", cited_text: "emissions fell" }] },
    ],
  });
  t.after(() => {
    globalThis.fetch = originalFetch;
    delete process.env.LOUPIST_PROVIDER_RESULT_FILE;
    delete process.env.LOUPIST_PROVIDER_REQUEST_FILE;
    rmSync(temporary, { recursive: true, force: true });
  });

  const moduleUrl = `${pathToFileURL(join(import.meta.dirname, "loupist-provider-extension.mjs"))}?search=${Date.now()}`;
  const { default: extension } = await import(moduleUrl);
  const commands = new Map();
  extension({ registerCommand: (name, command) => commands.set(name, command) });
  let payload;
  const context = {
    modelRegistry: {
      getProvider: () => ({ id: "anthropic", name: "Anthropic" }),
      getAvailable: () => [{ provider: "anthropic", id: "claude-haiku-4-5" }],
      getProviderAuth: async () => ({ auth: { apiKey: "sk-ant-secret", headers: { "x-api-key": "sk-ant-secret" } } }),
      find: (provider, id) => ({ provider, id }),
      complete: async (_model, _context, options) => {
        payload = options.onPayload({});
        const response = await options.fetch("https://api.anthropic.com/v1/messages", { method: "POST" });
        await response.text();
        return { role: "assistant", content: [], stopReason: "stop" };
      },
    },
  };
  await commands.get("loupist-provider-search").handler("", context);
  const encoded = readFileSync(resultPath, "utf8");
  const result = JSON.parse(encoded);
  assert.equal(result.protocol, "loupist-provider/1");
  assert.equal(result.status, "ok");
  assert.equal(result.provider, "anthropic");
  assert.equal(payload.model, "claude-haiku-4-5");
  assert.deepEqual(result.sources.map((source) => source.url), ["https://example.org/un-report"]);
  for (const forbidden of ["sk-ant-secret", "Anthropic synthesized answer", "credential", "answer"]) {
    assert.equal(encoded.includes(forbidden), false, forbidden);
  }

  // Invalid non-null values fail before any credential or provider request.
  writeFileSync(requestPath, JSON.stringify({ ...request, recency: "yesterday" }));
  await assert.rejects(
    () => commands.get("loupist-provider-search").handler("", {}),
    /search recency is invalid/,
  );
  writeFileSync(requestPath, JSON.stringify({ ...request, credential: "x" }));
  await assert.rejects(
    () => commands.get("loupist-provider-search").handler("", {}),
    /search request is invalid/,
  );
});
