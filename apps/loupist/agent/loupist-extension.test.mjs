import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { constants, existsSync, mkdtempSync, openSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import net from "node:net";
import { createHash, randomUUID } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { fauxAssistantMessage, fauxProvider, fauxToolCall, InMemoryCredentialStore, validateToolArguments } from "@earendil-works/pi-ai";
import { createAgentSession, DefaultResourceLoader, ModelRuntime, SessionManager, SettingsManager } from "@earendil-works/pi-coding-agent";
import { convertResponsesMessages, convertResponsesTools } from "@earendil-works/pi-ai/api/openai-responses-shared";

import loupistTerminalExtension, {
  resolveCapability,
  formatRuntimeIndex,
  hostContextOpenNoFollowForPlatform,
  orderedToolNames,
} from "./loupist-extension.mjs";

const CATALOG = JSON.parse(readFileSync(new URL("./tool-catalog.json", import.meta.url), "utf8"));
const WORKSPACE_TOOLS = ["read", "write", "edit", "bash", "ls", "find", "grep"];

test("keeps host-context nofollow protection off Windows only", () => {
  assert.equal(hostContextOpenNoFollowForPlatform("win32"), 0);
  assert.equal(hostContextOpenNoFollowForPlatform("darwin"), constants.O_NOFOLLOW ?? 0);
});

const tool = (id, name, tool_names, extra = {}) => ({ id, kind: "tool", name, description: `${name} tools.`, tool_names, provider_ids: [], ...extra });
const BUNDLED_COMPONENTS = [
  tool("file-capture", "Local file sources", ["workspace_capture"]),
  tool("datasets", "Datasets and local files", ["dataset_describe", "dataset_query", "dataset_export", "dataset_import"]),
  tool("calculations", "Calculations", ["calculate"]),
  tool("documents", "Markdown artifact publishing", ["artifact_publish"]),
  tool("memory-save", "Source retention and journals", ["source_save", "note_save", "lens_file", "principle_save"]),
  tool("charts", "Chart authoring", ["chart_query", "chart_publish"]),
  tool("world-bank", "World Bank", ["worldbank_series"], { connection: "world-bank" }),
  tool("krx", "KRX", ["krx_daily_quote"], { connection: "krx" }),
  tool("koreainvestment", "Korea Investment", ["kis_operation_search", "kis_read"], { connection: "koreainvestment" }),
  {
    id: "openbb", kind: "mcp", connection: "openbb", server_id: "openbb", name: "Market data",
    description: "Read-only market, company, filings, and macro data.",
    tool_names: [], provider_ids: ["alpha_vantage", "fred", "sec", "yfinance"],
  },
  tool("opendart", "OpenDART", ["dart_company_lookup", "dart_filing_search", "dart_filing_read", "dart_financials"], { connection: "opendart" }),
  tool("web-research", "Web Research", ["web_search", "web_fetch"], { connection: "web-research" }),
];

function emptyWorkspace(directory = process.cwd()) {
  const root = realpathSync(directory);
  const digest = createHash("sha256").update("loupist-workspace/1\0").update(root).update("\0").update(root).digest("hex");
  return { root, global_root: root, instructions: [], skills: [], digest };
}

function promptEvent() {
  return { systemPromptOptions: { cwd: "/private-runtime", skills: [], contextFiles: [], sections: {} } };
}

function runtimeContext(mode, coreToolNames, components = [], extra = {}) {
  return {
    workspace: emptyWorkspace(),
    agent_runtime: {
      schema: "loupist-agent-runtime/1",
      mode,
      memory_access_enabled: coreToolNames.some((name) => ["world_search", "world_read", "lens_search", "lens_read", "world_job", "conversation_memory"].includes(name)),
      memory_update_enabled: components.some((component) => component.tool_names.some((name) => ["source_save", "note_save", "principle_save"].includes(name))),
      core_tool_names: coreToolNames,
      components,
    },
    ...extra,
  };
}

function probeNativeExtensionStartup(temporary, context, runOptions = "{}", prompt = "/loupist-verify-settings {}") {
  const contextPath = join(temporary, "host-context.json");
  writeFileSync(contextPath, typeof context === "string" ? context : JSON.stringify(context));
  const script = `
    import { createAgentSession, DefaultResourceLoader, ModelRuntime, SessionManager, SettingsManager }
      from ${JSON.stringify(import.meta.resolve("@earendil-works/pi-coding-agent"))};
    import { fauxAssistantMessage, fauxProvider, InMemoryCredentialStore }
      from ${JSON.stringify(import.meta.resolve("@earendil-works/pi-ai"))};
    const root = ${JSON.stringify(temporary)};
    const settingsManager = SettingsManager.inMemory({ compaction: { enabled: false } });
    const loader = new DefaultResourceLoader({
      cwd: root, agentDir: root, settingsManager,
      noExtensions: true, noSkills: true, noPromptTemplates: true, noThemes: true, noContextFiles: true,
      additionalExtensionPaths: [${JSON.stringify(fileURLToPath(new URL("./loupist-extension.mjs", import.meta.url)))}],
    });
    await loader.reload();
    const modelRuntime = await ModelRuntime.create({
      credentials: new InMemoryCredentialStore(), modelsPath: null, refreshOnCreate: false,
    });
    const faux = fauxProvider();
    modelRuntime.registerNativeProvider(faux.provider);
    let providerCalls = 0;
    faux.setResponses([() => { providerCalls++; return fauxAssistantMessage("Test reply."); }]);
    const { session } = await createAgentSession({
      cwd: root, agentDir: root, modelRuntime, model: faux.getModel(), settingsManager,
      resourceLoader: loader, sessionManager: SessionManager.inMemory(root), noTools: "all",
    });
    try {
      await session.bindExtensions({ mode: "rpc" });
      await session.prompt(${JSON.stringify(prompt)});
      process.stdout.write(JSON.stringify({ extensionErrors: loader.getExtensions().errors.length, providerCalls }));
    } finally {
      session.dispose();
    }
  `;
  return spawnSync(process.execPath, ["--input-type=module", "-e", script], {
    encoding: "utf8", timeout: 10_000,
    env: { PATH: process.env.PATH, LOUPIST_HOST_CONTEXT_FILE: contextPath, LOUPIST_MODEL_RUN_OPTIONS: runOptions },
  });
}

test("rejects Memory tools outside the host's explicit read and save settings before Pi can reach a provider", (t) => {
  const temporary = realpathSync(mkdtempSync(join(tmpdir(), "loupist-memory-profile-")));
  t.after(() => rmSync(temporary, { recursive: true, force: true }));
  for (const name of ["world_read", "source_save", "note_save", "principle_save"]) {
    const context = runtimeContext("chat", [name], [], { workspace: emptyWorkspace(temporary) });
    context.agent_runtime.memory_access_enabled = false;
    context.agent_runtime.memory_update_enabled = false;
    const probe = probeNativeExtensionStartup(temporary, context);
    assert.equal(probe.status, 76, probe.stderr);
    assert.equal(probe.stdout, "");
    assert.equal(existsSync(join(temporary, "host-context.json")), false);
  }
});

test("invalid host context and run options terminate native Pi instead of dispatching its settings control to a provider", (t) => {
  const temporary = realpathSync(mkdtempSync(join(tmpdir(), "loupist-invalid-bootstrap-")));
  t.after(() => rmSync(temporary, { recursive: true, force: true }));
  const context = runtimeContext("chat", ["read"], [], { workspace: emptyWorkspace(temporary) });
  for (const [invalidContext, runOptions] of [[{}, "{}"], ["{invalid", "{}"], [context, "{invalid"], [context, "[]"]]) {
    const probe = probeNativeExtensionStartup(temporary, invalidContext, runOptions);
    assert.equal(probe.status, 76, probe.stderr);
    assert.equal(probe.stdout, "");
    assert.match(probe.stderr, /Chat initialization failed/);
    assert.equal(existsSync(join(temporary, "host-context.json")), false);
  }
  const valid = probeNativeExtensionStartup(temporary, context, "{}", "Read this test prompt.");
  assert.equal(valid.status, 0, valid.stderr);
  assert.deepEqual(JSON.parse(valid.stdout), { extensionErrors: 0, providerCalls: 1 });
});

test("an invalid initial workspace snapshot terminates before Pi can continue without its extension", (t) => {
  const temporary = realpathSync(mkdtempSync(join(tmpdir(), "loupist-invalid-workspace-")));
  t.after(() => rmSync(temporary, { recursive: true, force: true }));
  const contextPath = join(temporary, "host-context.json");
  const context = runtimeContext("chat", ["read"], [], {
    workspace: { ...emptyWorkspace(temporary), digest: "0".repeat(64) },
  });
  writeFileSync(contextPath, JSON.stringify(context), { mode: 0o600 });
  const extensionUrl = new URL("./loupist-extension.mjs", import.meta.url).href;
  const probe = spawnSync(process.execPath, ["--input-type=module", "-e", `
    import extension from ${JSON.stringify(extensionUrl)};
    extension({ registerCommand() {}, on() {}, registerTool() {}, setActiveTools() {} });
    process.stdout.write("unexpected continuation");
  `], {
    encoding: "utf8", timeout: 10_000,
    env: { PATH: process.env.PATH, LOUPIST_HOST_CONTEXT_FILE: contextPath, LOUPIST_MODEL_RUN_OPTIONS: "{}" },
  });
  assert.equal(probe.status, 76, probe.stderr);
  assert.equal(probe.stdout, "");
  assert.match(probe.stderr, /Chat initialization failed/);
  assert.equal(existsSync(contextPath), false);
});

test("chat diagnostics bypass stdout takeover and emit only opt-in bounded phases and valid statuses", (t) => {
  const temporary = realpathSync(mkdtempSync(join(tmpdir(), "loupist-chat-diagnostics-")));
  t.after(() => rmSync(temporary, { recursive: true, force: true }));
  const contextPath = join(temporary, "host-context.json");
  const extensionUrl = new URL("./loupist-extension.mjs", import.meta.url).href;
  const script = `
    import assert from "node:assert/strict";
    import fs from "node:fs";
    import { syncBuiltinESMExports } from "node:module";
    import extension from ${JSON.stringify(extensionUrl)};
    process.stdout.write = (...args) => process.stderr.write(...args);
    let failedWrites = 0;
    if (process.env.PROBE_DIAGNOSTIC_WRITE_FAILURE === "1") {
      fs.writeSync = () => { failedWrites++; throw new Error("Probe output failure"); };
      syncBuiltinESMExports();
    }
    const handlers = new Map();
    extension({ registerCommand() {}, on: (name, handler) => handlers.set(name, handler), registerTool() {}, setActiveTools() {}, getCommands: () => [] });
    const event = { systemPromptOptions: { cwd: "/private-runtime", skills: [], contextFiles: [], sections: {} },
      get prompt() { throw new Error("Diagnostic read the prompt"); } };
    await handlers.get("before_agent_start")(event);
    assert.equal(event.systemPromptOptions.cwd, ${JSON.stringify(temporary)});
    assert.equal(handlers.get("before_provider_headers")({
      get headers() { throw new Error("Diagnostic read request headers"); },
    }), undefined);
    const payload = { model: "probe-model", input: "private-prompt-sentinel" };
    const transformed = handlers.get("before_provider_request")({ payload }, { model: { api: "openai-codex-responses" } });
    assert.deepEqual(transformed, { ...payload, service_tier: "priority" });
    for (const status of [200, 401, 100, 599, undefined, "401", 99, 600, 200.5, NaN, Infinity]) {
      handlers.get("after_provider_response")({ status,
        get headers() { throw new Error("Diagnostic read response headers"); },
        get error() { throw new Error("Diagnostic read error text"); } });
    }
    if (process.env.PROBE_DIAGNOSTIC_WRITE_FAILURE === "1") assert.equal(failedWrites, 7);
  `;
  for (const [enabled, failWrites] of [[undefined, false], ["0", false], ["true", false], ["1", false], ["1", true]]) {
    writeFileSync(contextPath, JSON.stringify(runtimeContext("chat", ["read"], [], {
      workspace: emptyWorkspace(temporary),
    })), { mode: 0o600 });
    const probe = spawnSync(process.execPath, ["--input-type=module", "-e", script], {
      encoding: "utf8", timeout: 10_000,
      env: {
        PATH: process.env.PATH,
        LOUPIST_HOST_CONTEXT_FILE: contextPath,
        LOUPIST_MODEL_RUN_OPTIONS: JSON.stringify({ performance: "fast" }),
        ...(enabled === undefined ? {} : { LOUPIST_E2E_CHAT_DIAGNOSTICS: enabled }),
        ...(failWrites ? { PROBE_DIAGNOSTIC_WRITE_FAILURE: "1" } : {}),
      },
    });
    assert.equal(probe.status, 0, probe.stderr);
    assert.equal(probe.stderr, "");
    if (enabled !== "1" || failWrites) {
      assert.equal(probe.stdout, "");
    } else {
      const lines = probe.stdout.trim().split("\n");
      assert(lines.every((line) => Buffer.byteLength(`${line}\n`, "utf8") <= 200));
      assert.deepEqual(lines.map((line) => JSON.parse(line)), [
        { type: "loupist_diagnostic", phase: "before_agent_start" },
        { type: "loupist_diagnostic", phase: "before_provider_headers" },
        { type: "loupist_diagnostic", phase: "before_provider_request" },
        ...[200, 401, 100, 599].map((status) => ({ type: "loupist_diagnostic", phase: "provider_response", status })),
      ]);
    }
  }
});

test("a busy provider response forwards only its timing headers to Rust", (t) => {
  const temporary = realpathSync(mkdtempSync(join(tmpdir(), "loupist-retry-hints-")));
  t.after(() => rmSync(temporary, { recursive: true, force: true }));
  const contextPath = join(temporary, "host-context.json");
  writeFileSync(contextPath, JSON.stringify(runtimeContext("chat", ["read"], [], { workspace: emptyWorkspace(temporary) })), { mode: 0o600 });
  const extensionUrl = new URL("./loupist-extension.mjs", import.meta.url).href;
  const script = `
    import extension from ${JSON.stringify(extensionUrl)};
    process.stdout.write = (...args) => process.stderr.write(...args);
    const handlers = new Map();
    extension({ registerCommand() {}, on: (name, handler) => handlers.set(name, handler), registerTool() {}, setActiveTools() {}, getCommands: () => [] });
    const respond = handlers.get("after_provider_response");
    respond({ status: 200, get headers() { throw new Error("read a successful response's headers"); } });
    respond({ status: 429, headers: { "Retry-After": "12", "x-ratelimit-reset-tokens": "6m0s", "set-cookie": "session=secret",
      "x-request-id": "req_private", "anthropic-ratelimit-tokens-reset": "x".repeat(65) } });
    respond({ status: 503, headers: {} });
  `;
  const probe = spawnSync(process.execPath, ["--input-type=module", "-e", script], {
    encoding: "utf8", timeout: 10_000,
    env: { PATH: process.env.PATH, LOUPIST_HOST_CONTEXT_FILE: contextPath, LOUPIST_MODEL_RUN_OPTIONS: "{}" },
  });
  assert.equal(probe.status, 0, probe.stderr);
  assert.deepEqual(probe.stdout.trim().split("\n").map((line) => JSON.parse(line)), [
    { type: "loupist_retry_hints", headers: { "retry-after": "12", "x-ratelimit-reset-tokens": "6m0s" } },
    { type: "loupist_retry_hints", headers: {} },
  ]);
});

/** Point the extension at a sealed host context and restore the process env afterwards. */
function useHostContext(t, context, runOptions = "{}") {
  const temporary = realpathSync(mkdtempSync(join(tmpdir(), "loupist-extension-")));
  const path = join(temporary, "host-context.json");
  writeFileSync(path, JSON.stringify(context), { mode: 0o600 });
  const keys = ["LOUPIST_HOST_CONTEXT_FILE", "LOUPIST_MODEL_RUN_OPTIONS"];
  const previous = keys.map((key) => process.env[key]);
  process.env.LOUPIST_HOST_CONTEXT_FILE = path;
  process.env.LOUPIST_MODEL_RUN_OPTIONS = runOptions;
  t.after(() => {
    keys.forEach((key, index) => {
      if (previous[index] === undefined) delete process.env[key];
      else process.env[key] = previous[index];
    });
    rmSync(temporary, { recursive: true, force: true });
  });
  return { temporary, path };
}

/** Load the extension against a recording Pi double. */
function loadExtension(commands = []) {
  const registered = new Map();
  const handlers = new Map();
  const snapshots = [];
  loupistTerminalExtension({
    registerCommand() {},
    registerTool: (definition) => registered.set(definition.name, definition),
    on: (name, handler) => handlers.set(name, handler),
    setActiveTools: (names) => snapshots.push(names),
    getCommands: () => commands,
  });
  return { registered, handlers, snapshots, active: () => snapshots.at(-1) };
}

/** A private one-request broker that acknowledges delivery like the Rust host. */
async function startToolBroker(t, handler) {
  const temporary = mkdtempSync(join(tmpdir(), "loupist-extension-broker-"));
  const socketPath = process.platform === "win32"
    ? `\\\\.\\pipe\\loupist-extension-broker-${randomUUID()}`
    : join(temporary, "broker.sock");
  const keys = ["LOUPIST_BROKER_SOCKET", "LOUPIST_BROKER_TOKEN_FD"];
  const previous = keys.map((key) => process.env[key]);
  // Rust hands the token over on an inherited descriptor; any open descriptor
  // exercises the same one-shot read at extension load.
  const tokenPath = join(temporary, "token");
  writeFileSync(tokenPath, "one-turn-capability\n", { mode: 0o600 });
  process.env.LOUPIST_BROKER_SOCKET = socketPath;
  process.env.LOUPIST_BROKER_TOKEN_FD = String(openSync(tokenPath, "r"));
  const frames = [];
  const server = net.createServer((socket) => {
    let buffered = "";
    let requestId;
    socket.setEncoding("utf8");
    socket.on("data", (chunk) => {
      buffered += chunk;
      for (let newline = buffered.indexOf("\n"); newline >= 0; newline = buffered.indexOf("\n")) {
        const frame = JSON.parse(buffered.slice(0, newline));
        buffered = buffered.slice(newline + 1);
        if (requestId) {
          assert.deepEqual(frame, { protocol: "loupist-tool/1", id: requestId, delivered: true });
          socket.end(`${JSON.stringify({ protocol: "loupist-tool/1", id: requestId, committed: true })}\n`);
          return;
        }
        requestId = frame.id;
        frames.push(frame);
        let response;
        try {
          response = { ok: true, result: handler(frame) };
        } catch (error) {
          response = { ok: false, error: { message: error.message } };
        }
        socket.write(`${JSON.stringify({ protocol: "loupist-tool/1", id: frame.id, ...response })}\n`);
      }
    });
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(socketPath, resolve);
  });
  t.after(async () => {
    await new Promise((resolve) => server.close(resolve));
    keys.forEach((key, index) => {
      if (previous[index] === undefined) delete process.env[key];
      else process.env[key] = previous[index];
    });
    rmSync(temporary, { recursive: true, force: true });
  });
  return frames;
}

const CORE_TOOLS = [
  ...WORKSPACE_TOOLS, "run_results_list", "chat_history",
  "world_search", "world_read", "lens_search", "lens_read", "world_job", "conversation_memory", "capability_load",
  "artifact_list", "artifact_read",
];

test("registers only the host allowlist, defers components, and round-trips through the private broker", async (t) => {
  const context = runtimeContext("chat", CORE_TOOLS, BUNDLED_COMPONENTS, { explicit_skill_names: ["house-style"] });
  const { temporary, path } = useHostContext(t, context, JSON.stringify({ performance: "fast" }));
  context.workspace = emptyWorkspace(temporary);
  writeFileSync(path, JSON.stringify(context), { mode: 0o600 });
  const selectedSkillPath = join(temporary, "house-style.md");
  writeFileSync(selectedSkillPath, "---\nname: house-style\ndescription: Selected formatting method\n---\n\nSELECTED_SKILL_BODY\n");
  const frames = await startToolBroker(t, () => ({ records: ["lens-quality"] }));

  const { registered, handlers, snapshots, active } = loadExtension([
    { name: "skill:house-style", source: "skill", sourceInfo: { path: selectedSkillPath } },
  ]);
  const allowed = new Set([...CORE_TOOLS, ...BUNDLED_COMPONENTS.flatMap((component) => component.tool_names)]);
  assert.deepEqual([...registered.keys()].sort(), [...allowed].sort());
  // The single-use bootstrap is consumed before any model work.
  assert.equal(existsSync(path), false);
  assert.equal(process.env.LOUPIST_HOST_CONTEXT_FILE, undefined);
  assert.equal(process.env.LOUPIST_MODEL_RUN_OPTIONS, undefined);
  // Chat-presentation metadata never reaches a model-facing definition.
  assert.equal([...registered.values()].every((definition) => !Object.hasOwn(definition, "progress")), true);

  await handlers.get("session_start")();
  const transformed = handlers.get("input")({ source: "rpc", text: "<current_user_message>Use $house-style.</current_user_message>" });
  assert.equal(transformed.action, "transform");
  assert(transformed.text.includes("SELECTED_SKILL_BODY"));
  const event = promptEvent();
  await handlers.get("before_agent_start")(event);
  const injected = event.systemPromptOptions.sections.loupist_runtime;
  assert.equal(injected, formatRuntimeIndex(context.agent_runtime));
  assert.equal(event.systemPromptOptions.cwd, temporary);
  for (const hidden of ["explicit_skill_names", "SELECTED_SKILL_BODY", "core_tool_names", "tool_names", "server_id"]) {
    assert.equal(injected.includes(hidden), false, hidden);
  }
  assert.deepEqual(
    handlers.get("before_provider_request")({ payload: { model: "gpt-test" } }, { model: { api: "openai-codex-responses" } }),
    { model: "gpt-test", service_tier: "priority" },
  );

  // Component tools are deferred: the eager prefix is the core set only.
  assert.deepEqual([...snapshots[0]].sort(), [...CORE_TOOLS].sort());
  const result = await registered.get("world_search").execute("tool-call", { query: "margin" }, new AbortController().signal);
  assert.deepEqual(result.details, { records: ["lens-quality"] });
  assert.equal(frames[0].token, "one-turn-capability");
  // The descriptor was consumed and closed at load; nothing a tool spawns can inherit it.
  assert.equal(process.env.LOUPIST_BROKER_TOKEN_FD, undefined);
  assert.equal(frames[0].method, "world.search");
  assert.deepEqual(frames[0].params, { query: "margin" });
  assert.equal(frames[0].call_id, "tool-call");

  const capabilityLoad = registered.get("capability_load");
  await capabilityLoad.execute("load-source-retention", { id: "memory-save" });
  assert(active().includes("source_save") && active().includes("note_save"));
  assert(active().includes("lens_file") && active().includes("principle_save"));
  const loadedAnalysis = await capabilityLoad.execute("load-analysis", { id: "datasets" });
  assert.equal(typeof loadedAnalysis.details.tools.find((item) => item.name === "dataset_import").parameters, "object");
  const receipt = JSON.parse(loadedAnalysis.content[0].text);
  assert.deepEqual(receipt, {
    id: "datasets", kind: "tool", name: "Datasets and local files",
    tool_names: ["dataset_describe", "dataset_query", "dataset_export", "dataset_import"], loaded: true,
  });
  const activationCount = snapshots.length;
  const repeated = await capabilityLoad.execute("load-analysis-again", { id: "datasets" });
  assert.equal(JSON.parse(repeated.content[0].text).already_loaded, true);
  assert.equal(repeated.details.tools, undefined);
  assert.equal(snapshots.length, activationCount);

  // The pinned Pi serializer sends the activated schema once, as a tool
  // definition, and never repeats it in the function-result history.
  const activeDefinitions = [...registered.values()].filter((definition) => active().includes(definition.name));
  const imported = registered.get("dataset_import");
  assert.deepEqual(convertResponsesTools(activeDefinitions).find((item) => item.name === "dataset_import").parameters, imported.parameters);
  const providerMessages = convertResponsesMessages(
    { id: "test-model", provider: "openai", api: "openai-responses", input: ["text"] },
    { messages: [{ role: "toolResult", toolCallId: "load-analysis", toolName: "capability_load", ...loadedAnalysis, isError: false, timestamp: 0 }] },
    new Set(["openai"]),
  );
  assert.equal(providerMessages[0].output, loadedAnalysis.content[0].text);
  assert.equal(JSON.stringify(providerMessages).includes('"parameters"'), false);

  const loadedWeb = await capabilityLoad.execute("load-web", { id: "connection:WEB-RESEARCH" });
  assert.deepEqual(loadedWeb.details.tool_names, ["web_search", "web_fetch"]);
  const activeAfterWeb = active();
  for (const id of ["unavailable.provider", "yfinance", "unapproved/tool"]) {
    await assert.rejects(() => capabilityLoad.execute("load-missing", { id }), /not available in this run/);
  }
  assert.deepEqual(active(), activeAfterWeb);
});

const ACTIVATE_CARD = {
  name: "mcp__openbb__activate_tools", mcp_name: "activate_tools", server_id: "openbb",
  label: "Activate tools", description: "Activate an allowlisted read-only tool.",
  parameters: {
    type: "object", additionalProperties: false, required: ["tool_names"],
    properties: { tool_names: { type: "array", items: { type: "string" } } },
  },
};
const OPENBB_COMPONENT = {
  id: "openbb", kind: "mcp", server_id: "openbb", name: "OpenBB providers",
  description: "Read-only OpenBB financial data.", tool_names: [], provider_ids: ["fmp", "yfinance"],
};

test("loads namespaced MCP tools and reconciles list changes, call errors and runtime stops", async (t) => {
  useHostContext(t, runtimeContext("chat", ["capability_load"], [OPENBB_COMPONENT]));
  const quoteCard = {
    name: "mcp__openbb__equity_price_quote", mcp_name: "equity_price_quote", server_id: "openbb",
    label: "Equity quote", description: "Read an equity quote.",
    parameters: {
      type: "object", additionalProperties: false, required: ["symbol", "provider"],
      properties: { symbol: { type: "string" }, provider: { type: "string", enum: ["fmp", "yfinance"] } },
    },
  };
  let quoteCalls = 0;
  const frames = await startToolBroker(t, (request) => {
    if (request.method === "mcp.connect") return { server_id: "openbb", tools: [ACTIVATE_CARD] };
    if (request.params.tool_name === "activate_tools") {
      return { result: { content: [{ type: "text", text: "Activated" }] }, tools: [ACTIVATE_CARD, quoteCard] };
    }
    quoteCalls += 1;
    if (quoteCalls === 2) {
      return { call_error: true, message: "Provider access rejected.", next: "Fixture recovery: verify the provider in Connections.", tools: [ACTIVATE_CARD] };
    }
    if (quoteCalls === 3) return { call_error: true, session_stopped: true };
    return { result: { structuredContent: { price: 213.4, provider: "fmp" }, result_ref: "result:delivered" } };
  });

  const { registered, snapshots, active } = loadExtension();
  const capabilityLoad = registered.get("capability_load");
  const loaded = await capabilityLoad.execute("load-openbb", { id: "openbb" });
  assert.deepEqual(JSON.parse(loaded.content[0].text), {
    id: "openbb", kind: "mcp", name: "OpenBB providers", server_id: "openbb",
    tool_names: ["mcp__openbb__activate_tools"], loaded: true,
  });
  assert.deepEqual(loaded.details.tools[0].parameters, ACTIVATE_CARD.parameters);
  const [connectedCount, activationCount] = [frames.length, snapshots.length];
  assert.equal((await capabilityLoad.execute("load-openbb-again", { id: "openbb" })).details.already_loaded, true);
  assert.equal(frames.length, connectedCount);
  assert.equal(snapshots.length, activationCount);

  const activate = registered.get("mcp__openbb__activate_tools");
  await activate.execute("activate-quote", { tool_names: ["equity_price_quote"] });
  const quote = registered.get("mcp__openbb__equity_price_quote");
  assert(active().includes(quote.name));
  assert.throws(() => validateToolArguments(quote, { type: "toolCall", id: "invalid", name: quote.name, arguments: { symbol: "AAPL", provider: "unapproved" } }));
  assert.equal((await quote.execute("read-quote", { symbol: "AAPL", provider: "fmp" })).details.result_ref, "result:delivered");
  // A call error carries the host's repair text and the reconciled inventory.
  await assert.rejects(
    () => quote.execute("stale-quote", { symbol: "AAPL", provider: "fmp" }),
    (error) => error.message.includes("Fixture recovery") && !error.message.includes("Change the request"),
  );
  assert(!active().includes(quote.name));
  assert(active().includes(ACTIVATE_CARD.name));
  await activate.execute("reactivate-quote", { tool_names: ["equity_price_quote"] });
  // A stopped runtime unloads the whole component.
  await assert.rejects(() => quote.execute("stopped", { symbol: "AAPL", provider: "fmp" }), /Bundled MCP runtime stopped/);
  assert(!active().some((name) => name.startsWith("mcp__openbb__")));
  assert.deepEqual(frames.at(-1).params, { server_id: "openbb", tool_name: "equity_price_quote", arguments: { symbol: "AAPL", provider: "fmp" } });
});

test("a rejected MCP inventory never produces an activation receipt or partially activates tools", async (t) => {
  useHostContext(t, runtimeContext("chat", ["capability_load"], [OPENBB_COMPONENT]));
  const validCard = {
    name: "mcp__openbb__quote", mcp_name: "quote", server_id: "openbb", label: "Quote", description: "Read a quote.",
    parameters: { type: "object", properties: { symbol: { type: "string" } } },
  };
  let badParameters;
  await startToolBroker(t, () => ({
    server_id: "openbb",
    tools: [validCard, { ...validCard, name: "mcp__openbb__invalid", mcp_name: "invalid", parameters: badParameters }],
  }));
  const { registered, handlers, active } = loadExtension();
  await handlers.get("session_start")();
  const originalActive = active();
  for (const parameters of [null, [], { padding: "x".repeat(64 * 1024) }]) {
    badParameters = parameters;
    await assert.rejects(() => registered.get("capability_load").execute("load-invalid", { id: "openbb" }), /MCP tool inventory is invalid/);
    assert.deepEqual(active(), originalActive);
    assert.equal([...registered.keys()].some((name) => name.startsWith("mcp__")), false);
  }
});

test("preloads mentioned MCP capabilities before the agent starts", async (t) => {
  const context = runtimeContext("chat", ["capability_load"], [OPENBB_COMPONENT], { preload_component_ids: ["openbb"] });
  useHostContext(t, context);
  const frames = await startToolBroker(t, () => ({ server_id: "openbb", tools: [ACTIVATE_CARD] }));
  const { handlers, active } = loadExtension();
  const event = promptEvent();
  await handlers.get("before_agent_start")(event);
  assert.deepEqual(frames.map((frame) => frame.method), ["mcp.connect"]);
  assert(active().includes(ACTIVATE_CARD.name));
  assert.equal(event.systemPromptOptions.sections.loupist_runtime, formatRuntimeIndex(context.agent_runtime));
  assert.deepEqual(active(), orderedToolNames(active()));
});

test("host setup notices for an MCP connection reach the model's load receipt", async (t) => {
  useHostContext(t, runtimeContext("chat", ["capability_load"], [OPENBB_COMPONENT]));
  const notice = "Fixture provider is unavailable until the user finishes its setup in Connections.";
  let notices = [{ text: notice }];
  await startToolBroker(t, () => ({ server_id: "openbb", tools: [ACTIVATE_CARD], notices }));
  const { registered } = loadExtension();
  const load = registered.get("capability_load");
  await assert.rejects(() => load.execute("load-invalid", { id: "openbb" }), /notices are invalid/);
  assert.equal([...registered.keys()].some((name) => name.startsWith("mcp__")), false);
  notices = [notice];
  const loaded = await load.execute("load-openbb", { id: "openbb" });
  assert.deepEqual(JSON.parse(loaded.content[0].text).notices, [notice]);
});

test("note_save prepares misplaced and identical repeated fields but preserves conflicts and invalid shapes", (t) => {
  useHostContext(t, runtimeContext("chat", ["capability_load"], BUNDLED_COMPONENTS));
  const definition = loadExtension().registered.get("note_save");
  const validate = (args) => validateToolArguments(definition, { id: "save", name: definition.name, arguments: args });
  const workingNote = { title: "Review", markdown: "Evidence", changeSummary: "Initial note", expectedResearchId: null };
  const misplaced = { sourceReads: ["source-read"], worldReads: ["world-read"], lensReads: ["lens-read"], previousNoteIds: ["earlier-note"] };
  const nested = { workingNote: { ...workingNote, ...structuredClone(misplaced) } };
  assert.throws(() => validate(structuredClone(nested)));
  const prepared = definition.prepareArguments(nested);
  assert.deepEqual(prepared, { workingNote, ...misplaced });
  assert.deepEqual(validate(prepared), prepared);
  assert.deepEqual(definition.prepareArguments(structuredClone(prepared)), prepared);
  for (const [key, value] of Object.entries(misplaced)) {
    const repeated = { workingNote: { ...workingNote, [key]: structuredClone(value) }, [key]: structuredClone(value) };
    assert.deepEqual(definition.prepareArguments(repeated), { workingNote, [key]: value });
    for (const topLevel of [["different-read"], null]) {
      const conflict = { workingNote: { ...workingNote, [key]: value }, [key]: topLevel };
      const before = structuredClone(conflict);
      assert.deepEqual(definition.prepareArguments(conflict), before);
      assert.throws(() => validate(conflict));
    }
  }
  for (const invalid of [null, [], {}, { workingNote: null }, { workingNote: [] },
    { workingNote: { ...workingNote, extra: "keep invalid" } }, { workingNote, sourceReads: "not an array" }]) {
    const before = structuredClone(invalid);
    assert.deepEqual(definition.prepareArguments(invalid), before);
  }
  for (const invalid of [null, [], {}, { workingNote: null }, { workingNote: [] },
    { workingNote: { ...workingNote, extra: "keep invalid" } }]) {
    assert.throws(() => validate(invalid));
  }
  // Pi 1.5's validator wraps a lone value into a one-item array; the host then
  // checks each id, so an unknown read is still refused there.
  assert.deepEqual(validate({ workingNote, sourceReads: "not an array" }),
    { workingNote, sourceReads: ["not an array"] });
});

test("lens_file prepares a bare page id and misplaced basis refs without inventing evidence", (t) => {
  useHostContext(t, runtimeContext("chat", ["capability_load"], BUNDLED_COMPONENTS));
  const definition = loadExtension().registered.get("lens_file");
  const validate = (args) => validateToolArguments(definition, { id: "file", name: definition.name, arguments: args });
  const refs = [{ world: "rdg_world", claimId: "claim-1" }];
  const put = { op: "put", key: "facts", heading: "Facts", markdown: "Ada described the engine." };
  const request = (page, op) => ({ pages: [{ page, ops: [op] }], rationale: "A dated fact with its source." });
  const canonical = request({ recordId: "rec_page" }, { ...put, basis: { refs } });
  assert.deepEqual(validate(structuredClone(canonical)), canonical);
  for (const misplaced of [
    request("rec_page", { ...put, basis: { refs } }),
    request({ recordId: "rec_page" }, { ...put, basis: refs }),
    request({ recordId: "rec_page" }, { ...put, refs }),
    request({ recordId: "rec_page" }, { ...put, basis: {}, refs }),
    request({ recordId: "rec_page" }, { ...put, basis: { refs }, refs: structuredClone(refs) }),
  ]) {
    assert.throws(() => validate(structuredClone(misplaced)));
    const prepared = definition.prepareArguments(misplaced);
    assert.deepEqual(prepared, canonical);
    assert.deepEqual(validate(prepared), prepared);
  }
  const conflict = request({ recordId: "rec_page" }, { ...put, basis: { refs }, refs: [{ said: true }] });
  const before = structuredClone(conflict);
  assert.deepEqual(definition.prepareArguments(conflict), before);
  assert.throws(() => validate(conflict));
  for (const invalid of [null, [], {}, { pages: null, rationale: "x" }, { pages: [null], rationale: "x" },
    request({ recordId: "rec_page" }, { ...put, basis: { refs: [{ world: "rdg_world" }] } }),
    request({ create: { kind: "subject", title: "Engine" } }, { ...put, basis: { refs }, expected: "short" })]) {
    const unchanged = structuredClone(invalid);
    assert.deepEqual(definition.prepareArguments(invalid), unchanged);
    assert.throws(() => validate(invalid));
  }
});

test("native Pi exposes the registered Lens schema after rejection and only dispatches the corrected call", async (t) => {
  const components = BUNDLED_COMPONENTS.filter((item) => item.id === "memory-save");
  const context = runtimeContext("chat", ["capability_load"], components);
  const { temporary: root, path } = useHostContext(t, context);
  context.workspace = emptyWorkspace(root);
  writeFileSync(path, JSON.stringify(context), { mode: 0o600 });
  const brokerCalls = await startToolBroker(t, () => ({ status: "staged", durable: false, pages: [{ title: "Product stages", kind: "method", ops: 2 }] }));
  const settingsManager = SettingsManager.inMemory({ compaction: { enabled: false } });
  const loader = new DefaultResourceLoader({ cwd: root, agentDir: root, settingsManager,
    noExtensions: true, noSkills: true, noPromptTemplates: true, noThemes: true, noContextFiles: true,
    extensionFactories: [loupistTerminalExtension] });
  await loader.reload();
  assert.deepEqual(loader.getExtensions().errors, []);
  const modelRuntime = await ModelRuntime.create({ credentials: new InMemoryCredentialStore(), modelsPath: null, refreshOnCreate: false });
  const faux = fauxProvider();
  modelRuntime.registerNativeProvider(faux.provider);
  const ops = [
    { op: "put", key: "use-when", heading: "Use when", markdown: "Comparing semiconductor product progress." },
    { op: "put", key: "steps", heading: "Steps", markdown: "Separate samples, qualification, readiness, shipment and revenue; check evidence for each stage." },
  ];
  const request = { pages: [{ page: { create: { kind: "method", title: "Product stages" } }, ops }], rationale: "User taught a reusable method." };
  const call = (name, input) => fauxAssistantMessage(fauxToolCall(name, input), { stopReason: "toolUse" });
  const inspectRejection = (modelContext) => {
    const result = modelContext.messages.findLast((item) => item.role === "toolResult");
    assert.equal(result.toolName, "lens_file");
    assert.equal(result.isError, true);
    assert.equal(brokerCalls.length, 0);
    const schema = JSON.parse(result.content.at(-1).text.split("\n").slice(1).join("\n"));
    assert.deepEqual(schema, CATALOG.tools.find((item) => item.name === "lens_file").parameters);
  };
  faux.setResponses([
    call("capability_load", { id: "memory-save" }),
    call("lens_file", { ...request, pages: [{ page: { create: { kind: "method", title: "Product stages" } }, ops: [{ op: "put", key: "Steps" }] }] }),
    (modelContext) => {
      inspectRejection(modelContext);
      return call("lens_file", { ...request, pages: [{ page: { create: { kind: "method", title: "Product stages" } }, ops, sourceReads: [] }] });
    },
    (modelContext) => {
      inspectRejection(modelContext);
      return call("lens_file", { ...request, pages: [{ page: "rec_unread", ops }] });
    },
    (modelContext) => {
      const result = modelContext.messages.findLast((item) => item.role === "toolResult");
      assert.equal(result.isError, false);
      assert.equal(JSON.parse(result.content[0].text).status, "staged");
      return fauxAssistantMessage("The method is staged.");
    },
  ]);
  const { session } = await createAgentSession({ cwd: root, agentDir: root, modelRuntime, model: faux.getModel(),
    settingsManager, resourceLoader: loader, sessionManager: SessionManager.inMemory(root), noTools: "builtin" });
  t.after(() => session.dispose());
  await session.bindExtensions({ mode: "rpc" });
  await session.prompt("Remember the product-stage method.");
  assert.equal(brokerCalls.length, 1, JSON.stringify(session.messages));
  assert.equal(brokerCalls[0].method, "lens.file");
  assert.deepEqual(brokerCalls[0].params, { ...request, pages: [{ page: { recordId: "rec_unread" }, ops }] });
  assert.equal(session.messages.filter((item) => item.role === "toolResult" && item.isError).length, 2);
});

test("scoped dataset, finance and chart calls reach the broker unchanged and cannot forge scope", async (t) => {
  useHostContext(t, runtimeContext("chat", ["capability_load"], BUNDLED_COMPONENTS));
  const received = await startToolBroker(t, (request) => ({ status: "queued", acknowledged: request.params }));
  const { registered } = loadExtension();
  const validate = (name, args) => validateToolArguments(registered.get(name), { id: name, name, arguments: args });
  const columns = [
    { id: "date", label: "Date", kind: "date", pointer: "/date" },
    { id: "close", label: "Close (USD)", kind: "number", pointer: "/close" },
  ];
  const chart = {
    mode: "create",
    title: "Pinned close prices",
    dataset: { from_dataset: { dataset_id: "dataset-owned", revision: 2, rows_pointer: "/rows", columns } },
    view: { kind: "line", x: "date", y: ["close"] },
  };
  const calls = [
    ["dataset_import", "dataset.import", { path: "output.json", input_refs: [{ name: "prices", dataset_id: "dataset-owned", revision: 2, pointer: "/rows" }] }],
    ["krx_daily_quote", "krx.daily_quote", { symbol: "005930", date: "2026-01-02" }],
    ["dataset_export", "dataset.export", { dataset_id: "dataset-owned", revision: 2, path: "data/prices.csv", format: "csv", pointer: "/rows", columns: ["date", "close"] }],
    ["chart_publish", "chart.publish", chart],
  ];
  for (const [name, method, params] of calls) {
    assert.deepEqual(validate(name, params), params);
    // Ownership is the host's: a model cannot address another Loupist's data.
    assert.throws(() => validate(name, { ...params, loupist_id: "forged" }));
    await registered.get(name).execute(name, params, new AbortController().signal);
    assert.equal(received.at(-1).method, method);
    assert.deepEqual(received.at(-1).params, params);
  }
  assert.throws(() => validate("chart_publish", { ...chart, dataset: { from_dataset: { ...chart.dataset.from_dataset, loupist_id: "forged" } } }));

  // Natural single-series and list forms are accepted; malformed views name the field.
  for (const y of ["close", ["close"]]) assert.deepEqual(validate("chart_publish", { ...chart, view: { kind: "bar", x: "date", y } }).view.y, y);
  for (const view of [{ kind: "line", x: "date", y: { column: "close" } }, { kind: "bar", x: "date", y: [] }, { kind: "line", y: "close" }]) {
    assert.throws(() => validate("chart_publish", { ...chart, view }), (error) => /view/.test(error.message) && !/schema is false/.test(error.message));
  }
  const inline = { inline: { columns: columns.map(({ pointer, ...column }) => column), rows: [["2026-10-01", 100]] } };
  assert.deepEqual(validate("chart_publish", { ...chart, dataset: inline }).dataset, inline);
  assert.throws(() => validate("chart_publish", { ...chart, dataset: { ...chart.dataset, ...inline } }));
});

test("calculate accepts numbers, dataset cells and columns, and routes them unchanged", async (t) => {
  useHostContext(t, runtimeContext("chat", ["capability_load"], BUNDLED_COMPONENTS));
  const received = await startToolBroker(t, (request) => ({ values: [], acknowledged: request.params }));
  const { registered } = loadExtension();
  const validate = (args) => validateToolArguments(registered.get("calculate"), { id: "calc", name: "calculate", arguments: args });
  const cell = (pointer) => ({ dataset_id: "dataset-owned", revision: 2, pointer });
  const accepted = [
    { operation: "free_cash_flow", unit: "USD millions", inputs: { operating_cash_flow: 1240, capex: "-310" } },
    { operation: "margin", inputs: { profit: cell("/rows/0/operating_income"), revenue: cell("/rows/0/revenue") }, labels: { profit: "Operating income" } },
    { operation: "returns_summary", series: { dataset_id: "dataset-owned", revision: 2, rows_pointer: "/rows", column: "close", date_column: "date" }, periods_per_year: 252 },
    { operation: "moving_average", window: 3, series: [1, "2.5", null, 4], dates: ["2026-01-02", "2026-01-03", "2026-01-04", "2026-01-05"], skip_missing: true },
    { operation: "reverse_dcf", inputs: { cash_flow: 100, price: 17, shares: 100, net_debt: 0 }, wacc_pct: [8, 9, 10], terminal_growth_pct: [2, 2.5], forecast_years: 5 },
  ];
  for (const params of accepted) {
    assert.deepEqual(validate(params), params);
    await registered.get("calculate").execute("calc", params, new AbortController().signal);
    assert.equal(received.at(-1).method, "calculation.run");
    assert.deepEqual(received.at(-1).params, params);
  }
  for (const rejected of [
    { ...accepted[0], loupist_id: "forged" },
    { operation: "free_cash_flow", inputs: { operating_cash_flow: "1,240", capex: 310 } },
    { operation: "ratio", inputs: { numerator: "NaN", denominator: 1 } },
    { operation: "ratio", inputs: { api_key: 1, denominator: 1 } },
    { operation: "margin", inputs: { profit: { ...cell("/x"), loupist_id: "forged" }, revenue: 1 } },
    { operation: "dcf_grid", wacc_pct: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10], inputs: { cash_flow: 1, growth_pct: 2 } },
    { operation: "average" },
  ]) {
    assert.throws(() => validate(rejected), JSON.stringify(rejected));
  }
});

test("warm resume restores allowed components in catalog order and drops revoked tools", async (t) => {
  const datasets = BUNDLED_COMPONENTS.find((component) => component.id === "datasets");
  useHostContext(t, runtimeContext("chat", ["capability_load", "read"], [datasets]));
  const { handlers, active } = loadExtension();
  await handlers.get("session_start")(undefined, {
    sessionManager: {
      buildSessionContext: () => ({
        messages: [
          { role: "system", toolsAdded: [{ name: "dataset_query" }, { name: "dataset_import" }, { name: "source_save" }] },
          { role: "system", toolsRemoved: [{ name: "dataset_import" }] },
        ],
      }),
    },
  });
  assert(active().includes("dataset_query"));
  assert(active().includes("read"));
  assert(!active().includes("dataset_import"));
  assert(!active().includes("source_save"), "a tool outside this run's grant is never restored");
  assert.deepEqual(active(), orderedToolNames(active()));
});

test("catalog schemas survive provider adapters and never ask the model for credentials", () => {
  // Pi's Anthropic adapter copies only parameters.properties and .required, so a
  // top-level oneOf/anyOf would arrive as an empty object.
  for (const { name, parameters } of CATALOG.tools) {
    if (!parameters) continue;
    assert.equal(parameters.oneOf, undefined, name);
    assert.equal(parameters.anyOf, undefined, name);
    const properties = Object.keys(parameters.properties ?? {});
    if ((parameters.required ?? []).length > 0) assert.ok(properties.length > 0, name);
    assert.equal(properties.some((key) => /api.?key|credential|secret|password|app.?key|account/i.test(key)), false, name);
  }
  const index = formatRuntimeIndex({
    memory_access_enabled: true,
    memory_update_enabled: false,
    unavailable_connections: [{ name: "World Bank", description: "Country indicators and macroeconomic time series." }],
    components: [
      { id: "opendart", connection: "opendart", kind: "tool", name: "OpenDART", description: "Korean filings.", tool_names: ["dart_filing_search"], provider_ids: [] },
      { id: "openbb", connection: "openbb", kind: "mcp", name: "Market data", description: "Read-only market data.", tool_names: [], provider_ids: ["fred"] },
      { id: "charts", kind: "tool", name: "Chart authoring", description: "Publish charts.", tool_names: ["chart_publish"], provider_ids: [] },
    ],
  });
  assert.match(index, /Connections:\n- `openbb`/);
  assert.match(index, /Tools: dart_filing_search/);
  assert.match(index, /Providers: fred\./);
  assert(index.includes("World Bank: Country indicators and macroeconomic time series."));
  assert.match(index, /Built-in:\n- `charts`/);
});

test("capability resolution uses only unambiguous run ids and gives bounded repairs", () => {
  const components = new Map(["krx", "opendart", "web-research", "memory-save", "charts"].map((id) => [id, { id }]));
  for (const [input, expected] of [["KRX", "krx"], ["connection:opendart", "opendart"], ["CONNECTION:WEB-RESEARCH", "web-research"]]) {
    assert.equal(resolveCapability(input, components).id, expected);
  }
  for (const [skill, pack] of [["journal", "memory-save"], ["visualize", "charts"]]) {
    assert.equal(resolveCapability(skill, components, [], [`skill:${skill}`]).id, pack);
    const withoutPack = new Map([...components].filter(([id]) => id !== pack));
    assert.throws(() => resolveCapability(skill, withoutPack, [], [`skill:${skill}`]), (error) =>
      error instanceof Error && /Skill/.test(error.message) && /no capability load/.test(error.message) && !error.message.includes(pack));
  }
  const disabled = [{ id: "openbb", name: "Market data" }, { id: "web-research", name: "Web Research" }];
  const restricted = new Map([...components].filter(([id]) => id !== "web-research"));
  for (const id of ["connection:MARKET-DATA", "openbb", "web-research"]) {
    assert.throws(() => resolveCapability(id, restricted, disabled), /not enabled.*Connections/);
    assert(!restricted.has(id));
  }
  for (const id of ["unknown", undefined, "sk-secret-should-not-echo"]) {
    assert.throws(() => resolveCapability(id, components, disabled), (error) =>
      /Available capability ids:/.test(error.message) && error.message.includes("krx") && !error.message.includes("sk-secret-should-not-echo"));
  }
  const ambiguous = new Map([["krx", { id: "krx" }], ["KRX", { id: "KRX" }]]);
  assert.throws(() => resolveCapability("krx", ambiguous), /Available capability ids/);
  assert.throws(() => resolveCapability("KRX", new Map(), disabled), /Available capability ids: none/);
});
