// LG-003 uses the pinned Pi SDK's RPC mode with an in-memory provider.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import {
  createAgentSession,
  createAgentSessionRuntime,
  DefaultResourceLoader,
  ModelRuntime,
  runRpcMode,
  SessionManager,
  SettingsManager,
} from "@earendil-works/pi-coding-agent";
import {
  fauxAssistantMessage,
  fauxProvider,
  fauxToolCall,
  getCurrentTools,
  InMemoryCredentialStore,
} from "@earendil-works/pi-ai";

const [fixtureRoot, ...args] = process.argv.slice(2);
function argument(name) {
  const index = args.indexOf(name);
  assert(index >= 0, `Missing launch argument ${name}`);
  return args[index + 1];
}
const skillPaths = args.flatMap((arg, index) => arg === "--skill" ? [args[index + 1]] : []);
const cwd = process.cwd();
const agentDir = process.env.PI_CODING_AGENT_DIR;
const settingsManager = SettingsManager.create(cwd, agentDir);
const extension = await import(pathToFileURL(argument("--extension")).href);
const resourceLoader = new DefaultResourceLoader({
  cwd, agentDir, settingsManager,
  systemPrompt: argument("--system-prompt"),
  appendSystemPrompt: argument("--append-system-prompt"),
  additionalSkillPaths: skillPaths,
  noExtensions: true, noSkills: true, noPromptTemplates: true, noThemes: true, noContextFiles: true,
  extensionFactories: [extension.default],
});
await resourceLoader.reload();

const faux = fauxProvider({
  api: "faux-loupist-eval",
  provider: argument("--provider"),
  models: [{ id: argument("--model"), reasoning: true, input: ["text"], maxTokens: 32000 }],
});
const modelRuntime = await ModelRuntime.create({
  credentials: new InMemoryCredentialStore(), modelsPath: null, refreshOnCreate: false,
});
modelRuntime.registerNativeProvider(faux.provider);

function capture(file, context, options, model) {
  // Copy the provider transcript at callback entry. Request controls below
  // are Pi's portable inference options. The transport callbacks, cancellation
  // signal and authentication fields are local to the SDK.
  const { maxTokens, reasoning, temperature, topP, topK, thinkingBudgets, toolChoice,
    cacheRetention, sessionId, transport, samplingParams, metadata, deferred } = options ?? {};
  const snapshot = structuredClone({
    context,
    options: { maxTokens, reasoning, temperature, topP, topK, thinkingBudgets, toolChoice,
      cacheRetention, sessionId, transport, samplingParams, metadata, deferred },
    model,
  });
  for (const message of snapshot.context.messages) {
    if (message.role !== "toolResult") continue;
    // Pi carries UI details in its transcript object. Provider adapters use
    // the content blocks, so exclude the local details from exported inputs.
    delete message.details;
    delete message.nestedCalls;
  }
  writeFileSync(join(fixtureRoot, file), JSON.stringify(snapshot, null, 2) + "\n");
}

const firstBatch = ["world_search"];
function delivered(context, names) {
  const results = context.messages.filter(message => message.role === "toolResult");
  assert.deepEqual(results.map(result => result.toolName), names);
  for (const result of results) {
    assert(!result.isError, `${result.toolName} failed: ${JSON.stringify(result.content)}`);
    assert(result.content.length > 0);
  }
  return results;
}
function assertToolDeclarations(context) {
  const tools = getCurrentTools(context.messages);
  for (const name of [...firstBatch, "world_read"]) assert(tools.some(tool => tool.name === name), name);
  for (const tool of tools) {
    assert.equal(typeof tool.description, "string");
    assert.equal(typeof tool.parameters, "object");
  }
}

const text = result => result.content.find(block => block.type === "text").text;
let readArgs;

faux.setResponses([
  (context, options, _state, model) => {
    capture("first.json", context, options, model);
    assertToolDeclarations(context);
    return fauxAssistantMessage([
      fauxToolCall("world_search", { query: "Alder Rail quarterly note", scope: "source" }),
    ], { stopReason: "toolUse" });
  },
  (context) => {
    const [search] = delivered(context, firstBatch);
    const hits = JSON.parse(text(search)).hits;
    const hit = hits.find(hit => hit.readArgs?.scope === "source" && hit.readArgs.spanId);
    assert(hit, "Expected a Source span in the search result");
    readArgs = hit.readArgs;
    return fauxAssistantMessage(fauxToolCall("world_read", readArgs), { stopReason: "toolUse" });
  },
  (context, options, _state, model) => {
    capture("after-read.json", context, options, model);
    assertToolDeclarations(context);
    const results = delivered(context, [...firstBatch, "world_read"]);
    const calls = context.messages.filter(message => message.role === "assistant")
      .map(message => message.content.map(block => {
        assert.equal(block.type, "toolCall");
        return block;
      }));
    assert.deepEqual(calls.map(batch => batch.map(call => call.name)), [firstBatch, ["world_read"]]);
    assert.deepEqual(calls.flat().map(call => call.id), results.map(result => result.toolCallId));
    assert.deepEqual(calls[1][0].arguments, readArgs);
    const read = JSON.parse(text(results.at(-1)));
    assert.equal(read.source.spanId, readArgs.spanId);
    assert.equal(read.recordId, readArgs.recordId);
    assert.equal(read.revisionId, readArgs.revisionId);
    assert(read.readRef);
    return fauxAssistantMessage(`Alder Rail carried [18 million passengers](${read.citation_url}) in the third quarter.`);
  },
]);

const sessionManager = SessionManager.create(cwd, argument("--session-dir"), {
  id: argument("--session-id"),
});
const services = { cwd, agentDir, settingsManager, resourceLoader, modelRuntime, diagnostics: [] };
const runtime = await createAgentSessionRuntime(async ({ sessionManager }) => ({
  ...await createAgentSession({
    ...services, sessionManager,
    model: faux.getModel(), thinkingLevel: argument("--thinking"), noTools: "builtin",
  }),
  services,
  diagnostics: [],
}), { sessionManager });
await runRpcMode(runtime);
