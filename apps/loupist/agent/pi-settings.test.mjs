import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { createAgentSession, DefaultResourceLoader, ModelRuntime, SessionManager, SettingsManager } from "@earendil-works/pi-coding-agent";
import { fauxAssistantMessage, fauxProvider, InMemoryCredentialStore } from "@earendil-works/pi-ai";
import { registerPiSettingsControl, verifyPiSettings } from "./pi-settings.mjs";

function settings(enabled) {
  return { transport: "auto", httpIdleTimeoutMs: 300000,
    compaction: { enabled: true, reserveTokens: 1024, keepRecentTokens: 2048, modelOverrides: null },
    retry: { enabled, maxRetries: 2, baseDelayMs: 0, maxAgentDelayMs: 5000,
      provider: { maxRetries: 0, timeoutMs: 300000, maxRetryDelayMs: 60000 } } };
}

test("native execution verifies project retry settings without modifying shared settings or calling the provider", async (t) => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "loupist-scoped-pi-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const agentDir = join(root, "shared-agent");
  mkdirSync(agentDir);
  const globalBytes = JSON.stringify({ retry: { enabled: true, maxRetries: 7, provider: { maxRetries: 5, timeoutMs: 1, maxRetryDelayMs: 1 } }, compaction: { enabled: false } });
  writeFileSync(join(agentDir, "settings.json"), globalBytes);
  const modelRuntime = await ModelRuntime.create({ credentials: new InMemoryCredentialStore(), modelsPath: null, refreshOnCreate: false });
  const faux = fauxProvider();
  modelRuntime.registerNativeProvider(faux.provider);
  let providerCalls = 0;
  faux.setResponses([() => { providerCalls += 1; return fauxAssistantMessage("unexpected provider call"); }]);
  const sessions = [];
  for (const enabled of [false, true]) {
    const cwd = join(root, `project-${enabled}`);
    mkdirSync(join(cwd, ".pi"), { recursive: true });
    const expected = settings(enabled);
    writeFileSync(join(cwd, ".pi/settings.json"), JSON.stringify(expected));
    const settingsManager = SettingsManager.create(cwd, agentDir);
    const loader = new DefaultResourceLoader({ cwd, agentDir, settingsManager, noExtensions: true,
      noSkills: true, noPromptTemplates: true, noThemes: true, noContextFiles: true,
      extensionFactories: [registerPiSettingsControl] });
    await loader.reload();
    const { session } = await createAgentSession({ cwd, agentDir, modelRuntime, model: faux.getModel(),
      settingsManager, resourceLoader: loader, sessionManager: SessionManager.inMemory(cwd), noTools: "all" });
    sessions.push(session);
    t.after(() => session.dispose());
    await session.bindExtensions({ mode: "rpc" });
    let disposition;
    await session.prompt(`/loupist-verify-settings ${JSON.stringify(expected)}`, { source: "rpc", preflightResult: (value) => { disposition = value; } });
    assert.equal(disposition, "handled");
    assert.equal(session.messages.length, 0);
    assert.equal(session.autoRetryEnabled, enabled);
    assert.equal(session.autoCompactionEnabled, true);
    assert.deepEqual(settingsManager.getProviderRetrySettings(), { maxRetries: 0, timeoutMs: 300000, maxRetryDelayMs: 60000 });
    assert.deepEqual(settingsManager.getCompactionSettings(faux.getModel()), { enabled: true, reserveTokens: 1024, keepRecentTokens: 2048 });
    assert.equal(readFileSync(join(agentDir, "settings.json"), "utf8"), globalBytes);
  }
  assert.equal(sessions[0].autoRetryEnabled, false);
  assert.equal(providerCalls, 0);
});

test("Pi's cache warmer stops a refresh after a Chat turn settles", async (t) => {
  const cwd = realpathSync(mkdtempSync(join(tmpdir(), "loupist-cache-mode-")));
  t.after(() => rmSync(cwd, { recursive: true, force: true }));
  const settingsManager = SettingsManager.inMemory({ cacheWarming: "idle" });
  const loader = new DefaultResourceLoader({ cwd, agentDir: cwd, settingsManager,
    noExtensions: true, noSkills: true, noPromptTemplates: true, noThemes: true, noContextFiles: true,
    extensionFactories: [registerPiSettingsControl] });
  await loader.reload();
  const modelRuntime = await ModelRuntime.create({ credentials: new InMemoryCredentialStore(), modelsPath: null, refreshOnCreate: false });
  const faux = fauxProvider();
  const model = faux.getModel();
  model.promptCache = { short: 11 };
  model.cost.input = 1000;
  model.cost.cacheWrite = 1000;
  modelRuntime.registerNativeProvider(faux.provider);
  faux.setResponses([fauxAssistantMessage("Test response."), fauxAssistantMessage("Unexpected cache warm.")]);
  const { session } = await createAgentSession({ cwd, agentDir: cwd, modelRuntime, model,
    settingsManager, resourceLoader: loader, sessionManager: SessionManager.inMemory(cwd), noTools: "all" });
  t.after(() => session.dispose());
  await session.bindExtensions({ mode: "rpc" });
  t.mock.timers.enable({ apis: ["setTimeout", "Date"] });
  await session.prompt("A long prompt makes a cache refresh worthwhile. ".repeat(100), { source: "rpc" });
  assert.equal(session.cacheWarmingStatus.state, "scheduled");
  assert.equal(session.cacheWarmingStatus.decision.action, "warm");
  t.mock.timers.tick(1000);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(session.cacheWarmingStatus.reason, "stopped by extension");
  assert.equal(faux.state.callCount, 1);
});

test("Pi's cache warmer stops when the session cannot report whether a turn is active", () => {
  const handlers = new Map();
  registerPiSettingsControl({ on: (name, handler) => handlers.set(name, handler), registerCommand() {} });
  const decision = handlers.get("cache_warming_decision");
  assert.equal(decision({}, { isIdle: () => false }), undefined);
  assert.deepEqual(decision({}, { isIdle() { throw new Error("session released"); } }), { action: "stop" });
});

test("settings confirmation rejects unsupported values and effective-state drift", () => {
  const expected = settings(false);
  verifyPiSettings(expected, expected);
  for (const patch of [
    { retry: { ...expected.retry, enabled: true } },
    { compaction: { ...expected.compaction, enabled: false } },
    { httpIdleTimeoutMs: 1 },
    { retry: { ...expected.retry, provider: { ...expected.retry.provider, maxRetries: 1 } } },
    { compaction: { ...expected.compaction, modelOverrides: { "provider/model": { reserveTokens: 1 } } } },
  ]) assert.throws(() => verifyPiSettings({ ...expected, ...patch }, expected), /changed/);
  for (const patch of [
    { retry: { ...expected.retry, maxRetries: 11 } },
    { retry: { ...expected.retry, baseDelayMs: -1 } },
    { compaction: { ...expected.compaction, reserveTokens: Number.MAX_SAFE_INTEGER + 1 } },
  ]) assert.throws(() => verifyPiSettings(expected, { ...expected, ...patch }), /invalid/);
});


test("a settings mismatch exits before Pi can swallow command failure or acknowledge it", () => {
  const expected = settings(false);
  const moduleUrl = new URL("./pi-settings.mjs", import.meta.url).href;
  const result = spawnSync(process.execPath, ["--input-type=module", "-e", `
    import { registerPiSettingsControl } from ${JSON.stringify(moduleUrl)};
    let command;
    registerPiSettingsControl({ on() {}, registerCommand: (_, value) => { command = value; }, getSettings: () => ({}) });
    await command.handler(${JSON.stringify(JSON.stringify(expected))});
    process.stdout.write("unexpected acknowledgement");
  `], { encoding: "utf8", timeout: 10000 });
  assert.equal(result.status, 76);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /settings changed/);
});

test("a native warm resume reads the next snapshot from its stable private session CWD", async (t) => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "loupist-warm-settings-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const cwd = join(root, "chat-runtime");
  const sessionDir = join(root, "session");
  const agentDir = join(root, "shared-agent");
  mkdirSync(join(cwd, ".pi"), { recursive: true });
  mkdirSync(sessionDir);
  mkdirSync(agentDir);
  writeFileSync(join(agentDir, "settings.json"), "{}");
  const expected = settings(false);
  writeFileSync(join(cwd, ".pi/settings.json"), JSON.stringify(expected));
  const modelRuntime = await ModelRuntime.create({ credentials: new InMemoryCredentialStore(), modelsPath: null, refreshOnCreate: false });
  const faux = fauxProvider();
  modelRuntime.registerNativeProvider(faux.provider);
  let resumedContext;
  faux.setResponses([fauxAssistantMessage("Retained research context."), (context) => {
    resumedContext = context;
    return fauxAssistantMessage("Continued the research.");
  }]);
  const openSession = async (sessionManager) => {
    // The native CLI takes its runtime CWD from this header on warm resume.
    const runtimeCwd = sessionManager.getCwd();
    assert.equal(runtimeCwd, cwd);
    const settingsManager = SettingsManager.create(runtimeCwd, agentDir);
    const loader = new DefaultResourceLoader({ cwd: runtimeCwd, agentDir, settingsManager, noExtensions: true,
      noSkills: true, noPromptTemplates: true, noThemes: true, noContextFiles: true,
      extensionFactories: [registerPiSettingsControl] });
    await loader.reload();
    const { session } = await createAgentSession({ cwd: runtimeCwd, agentDir, modelRuntime, model: faux.getModel(),
      settingsManager, resourceLoader: loader, sessionManager, noTools: "all" });
    t.after(() => session.dispose());
    await session.bindExtensions({ mode: "rpc" });
    await session.prompt(`/loupist-verify-settings ${JSON.stringify(expected)}`, { source: "rpc" });
    assert.equal(session.autoRetryEnabled, false);
    return session;
  };
  const first = await openSession(SessionManager.create(cwd, sessionDir));
  await first.prompt("Remember this research context.", { source: "rpc" });
  const sessionFile = first.sessionManager.getSessionFile();
  const sessionId = first.sessionManager.getSessionId();
  first.dispose();
  // A fresh admitted turn rematerializes its frozen settings in the same CWD.
  writeFileSync(join(cwd, ".pi/settings.json"), JSON.stringify(expected));
  const second = await openSession(SessionManager.open(sessionFile, sessionDir));
  assert.equal(second.sessionManager.getSessionId(), sessionId);
  assert.equal(second.sessionManager.getHeader().cwd, cwd);
  assert(second.messages.some((message) => message.role === "assistant"));
  await second.prompt("Continue from that context.", { source: "rpc" });
  const text = resumedContext.messages.flatMap((message) => typeof message.content === "string"
    ? [message.content] : message.content.filter((part) => part.type === "text").map((part) => part.text)).join("\n");
  assert.match(text, /Remember this research context/);
  assert.match(text, /Retained research context/);
  assert.match(text, /Continue from that context/);
  assert.equal(readFileSync(join(agentDir, "settings.json"), "utf8"), "{}");
});
