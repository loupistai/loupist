import {
  closeSync,
  constants,
  fstatSync,
  openSync,
  readFileSync,
  unlinkSync,
  writeSync,
} from "node:fs";
import { isDeepStrictEqual } from "node:util";

import { loadBrokerConnection, requestBroker } from "./broker-client.mjs";
import { registerWorkspaceTools } from "./workspace-tools.mjs";
import { applyWorkspacePrompt, loadWorkspaceResources, registerWorkspaceResources, validExplicitSkillNames } from "./workspace-resources.mjs";
import { registerPiSettingsControl } from "./pi-settings.mjs";
import { applyRunOptions } from "./model-run-controls.mjs";

const MAX_HOST_CONTEXT_BYTES = 64 * 1024;
const MAX_RUN_OPTIONS_BYTES = 4 * 1024;
const MAX_MCP_TOOLS = 512;
const MAX_MCP_SCHEMA_BYTES = 64 * 1024;
const MAX_VALIDATION_SCHEMA_BYTES = 16 * 1024;
/** Busy-response timing headers Rust reads for the retry countdown; nothing else of a response is forwarded. */
const RETRY_HEADERS = new Set([
  "retry-after", "retry-after-ms", "x-ratelimit-reset-requests", "x-ratelimit-reset-tokens",
  "anthropic-ratelimit-requests-reset", "anthropic-ratelimit-tokens-reset",
  "anthropic-ratelimit-input-tokens-reset", "anthropic-ratelimit-output-tokens-reset",
]);
export function hostContextOpenNoFollowForPlatform(platform = process.platform) {
  return platform === "win32" ? 0 : (constants.O_NOFOLLOW ?? 0);
}

const HOST_CONTEXT_OPEN_NOFOLLOW = hostContextOpenNoFollowForPlatform();
// Shared, host-verified product contracts. Pi still owns native workspace schemas.
const TOOL_CATALOG = JSON.parse(readFileSync(new URL("./tool-catalog.json", import.meta.url), "utf8"));
const TOOL_SPECS = new Map(TOOL_CATALOG.tools.map((tool) => [tool.name, tool]));
const CATALOG_ORDER = new Map(TOOL_CATALOG.tools.map((tool, index) => [tool.name, index]));

/** Catalog tools keep their declared order. Dynamically discovered tools sort by name. */
export function orderedToolNames(names) {
  return [...names].sort((left, right) => {
    const leftIndex = CATALOG_ORDER.get(left);
    const rightIndex = CATALOG_ORDER.get(right);
    if (leftIndex !== undefined || rightIndex !== undefined) {
      if (leftIndex === undefined) return 1;
      if (rightIndex === undefined) return -1;
      if (leftIndex !== rightIndex) return leftIndex - rightIndex;
    }
    return left < right ? -1 : left > right ? 1 : 0;
  });
}

function formatCapabilityLine(component) {
  const description = String(component.description ?? "").replace(/\s+/gu, " ").trim();
  const tools = (component.tool_names ?? []).filter(Boolean);
  const providers = (component.provider_ids ?? []).filter(Boolean);
  const toolNote = tools.length > 0 ? ` Tools: ${tools.join(", ")}.` : "";
  const providerNote = component.kind === "mcp"
    ? `${providers.length > 0 ? ` Providers: ${providers.join(", ")}.` : ""} Loading activates common price, fundamentals, dividend and filing tools; use available_tools then activate_tools for others.`
    : "";
  return `- \`${component.id}\` — ${component.name}: ${description}${toolNote}${providerNote}`;
}

/** Model-facing capability index. Volatile turn data stays in the user envelope. */
export function formatRuntimeIndex(runtime) {
  const lines = [
    `memory_access_enabled: ${runtime.memory_access_enabled === true}`,
    `memory_update_enabled: ${runtime.memory_update_enabled === true}`,
    "",
    "# Capabilities",
    "Load an id with capability_load before calling its tools. @mentioned connections are already loaded.",
  ];
  const components = [...(runtime.components ?? [])].sort((left, right) =>
    left.id < right.id ? -1 : left.id > right.id ? 1 : 0);
  const connections = components.filter((component) => component.connection);
  const builtin = components.filter((component) => !component.connection);
  lines.push("Connections:");
  if (connections.length === 0) {
    lines.push("No connections are enabled for this turn.");
  } else {
    for (const component of connections) lines.push(formatCapabilityLine(component));
  }
  const unavailable = (runtime.unavailable_connections ?? []).filter(Boolean);
  if (unavailable.length > 0) {
    lines.push(`Not available in this Loupist: ${unavailable.map((connection) => `${connection.name}: ${connection.description}`).join("; ")}. If needed, tell the user to enable it in Connections.`);
  }
  if (builtin.length > 0) {
    lines.push("Built-in:");
    for (const component of builtin) lines.push(formatCapabilityLine(component));
  }
  return lines.join("\n");
}

// Resolution never adds authority: only the sealed run's component map can load.
export function resolveCapability(id, components, unavailable = [], skillNames = []) {
  const normalize = (value) => typeof value === "string"
    ? value.toLowerCase().replace(/^connection:/, "") : "";
  const requested = normalize(id);
  const matches = [...components.values()].filter((item) => normalize(item.id) === requested);
  if (requested && matches.length === 1) return matches[0];
  const available = [...components.keys()].slice(0, 64).map((value) => value.slice(0, 193)).join(", ");
  const prefix = "Capability is not available in this run. ";
  if (requested && skillNames.some((name) => normalize(name.replace(/^skill:/, "")) === requested)) {
    const closest = { journal: "memory-save", visualize: "charts" }[requested];
    // A Skill name stands for the tools that Skill uses; load them only when this run already has them.
    if (closest && components.has(closest)) return components.get(closest);
    throw new Error(prefix + "This is a Skill already listed in the session; no capability load is needed. " +
      `Available capability ids: ${available || "none"}.`);
  }
  // Display-name aliases are diagnostic only; they never resolve a load.
  if (requested && unavailable.some((item) => normalize(item.id) === requested ||
      normalize(item.name).replace(/\s+/g, "-") === requested)) {
    throw new Error(prefix + "This bundled connection is not enabled for this Loupist. The user can enable it in Connections.");
  }
  throw new Error(prefix + "Pass {id: capability_id}. Available capability ids: " + (available || "none") + ".");
}

function loadHostContext() {
  const path = process.env.LOUPIST_HOST_CONTEXT_FILE;
  delete process.env.LOUPIST_HOST_CONTEXT_FILE;
  if (!path) throw new Error("Loupist host context is unavailable");

  let descriptor;
  try {
    descriptor = openSync(path, constants.O_RDONLY | HOST_CONTEXT_OPEN_NOFOLLOW);
    const metadata = fstatSync(descriptor);
    if (!metadata.isFile() || metadata.size <= 0 || metadata.size > MAX_HOST_CONTEXT_BYTES) {
      throw new Error("Loupist host context is invalid");
    }
    const context = readFileSync(descriptor, "utf8");
    if (Buffer.byteLength(context, "utf8") !== metadata.size || context.includes("\0")) {
      throw new Error("Loupist host context is invalid");
    }
    const parsed = JSON.parse(context);
    if (
      !parsed ||
      typeof parsed !== "object" ||
      !parsed.agent_runtime ||
      typeof parsed.agent_runtime !== "object"
    ) {
      throw new Error("Loupist host context is invalid");
    }
    const runtime = parsed.agent_runtime;
    if (
      !runtime ||
      runtime.schema !== "loupist-agent-runtime/1" ||
      runtime.mode !== "chat" ||
      typeof runtime.memory_access_enabled !== "boolean" ||
      typeof runtime.memory_update_enabled !== "boolean" ||
      !validToolNames(runtime.core_tool_names) ||
      !Array.isArray(runtime.components) ||
      runtime.components.length > 64
    ) {
      throw new Error("Loupist agent runtime profile is invalid");
    }
    const components = new Map();
    const allowedTools = new Set(runtime.core_tool_names);
    for (const component of runtime.components) {
      if (
        !component ||
        typeof component !== "object" ||
        typeof component.id !== "string" ||
        !component.id ||
        components.has(component.id) ||
        typeof component.name !== "string" ||
        !component.name ||
        typeof component.description !== "string" ||
        !component.description ||
        !validRuntimeComponent(component) ||
        (component.provider_ids !== undefined && !validProviderIds(component.provider_ids))
      ) {
        throw new Error("Loupist agent runtime profile is invalid");
      }
      components.set(component.id, component);
      for (const name of component.tool_names) {
        if (allowedTools.has(name)) {
          throw new Error("Loupist agent runtime profile is invalid");
        }
        allowedTools.add(name);
      }
    }
    if (
      (runtime.components.length > 0) !== runtime.core_tool_names.includes("capability_load")
    ) {
      throw new Error("Loupist agent runtime profile is invalid");
    }
    if ([...allowedTools].some((name) =>
      (!runtime.memory_access_enabled && TOOL_SPECS.get(name).permission === "memory_read") ||
      (!runtime.memory_update_enabled && TOOL_SPECS.get(name).permission === "memory_write"),
    )) {
      throw new Error("Loupist Memory tool profile exceeds this turn's settings");
    }
    const preloadComponentIds = parsed.preload_component_ids ?? [];
    if (!validPreloadComponentIds(preloadComponentIds, components)) {
      throw new Error("Loupist preload component ids are invalid");
    }
    const explicitSkillNames = parsed.explicit_skill_names ?? [];
    if (!validExplicitSkillNames(explicitSkillNames)) {
      throw new Error("Loupist explicit Skill names are invalid");
    }
    return {
      promptText: formatRuntimeIndex(runtime),
      allowedTools,
      initialTools: new Set(runtime.core_tool_names),
      components,
      unavailableConnections: runtime.unavailable_connections ?? [],
      preloadComponentIds,
      explicitSkillNames,
      workspace: parsed.workspace,
    };
  } finally {
    if (descriptor !== undefined) closeSync(descriptor);
    try {
      unlinkSync(path);
    } catch {
      // Rust owns final cleanup if Pi exits before or during extension load.
    }
  }
}

function loadRunOptions() {
  const encoded = process.env.LOUPIST_MODEL_RUN_OPTIONS;
  delete process.env.LOUPIST_MODEL_RUN_OPTIONS;
  if (!encoded || Buffer.byteLength(encoded, "utf8") > MAX_RUN_OPTIONS_BYTES) {
    throw new Error("Loupist model run options are unavailable");
  }
  const parsed = JSON.parse(encoded);
  if (
    !parsed ||
    typeof parsed !== "object" ||
    Array.isArray(parsed) ||
    Object.keys(parsed).length > 16 ||
    Object.entries(parsed).some(
      ([key, value]) =>
        !/^[a-z][a-z0-9_-]{0,63}$/u.test(key) ||
        typeof value !== "string" ||
        !/^[a-z][a-z0-9_-]{0,63}$/u.test(value),
    )
  ) {
    throw new Error("Loupist model run options are invalid");
  }
  return parsed;
}

function validToolNames(names) {
  return (
    Array.isArray(names) &&
    names.length > 0 &&
    new Set(names).size === names.length &&
    names.every((name) => typeof name === "string" && TOOL_SPECS.has(name))
  );
}

function validRuntimeComponent(component) {
  if (component.kind === "tool") {
    return component.server_id === undefined && validToolNames(component.tool_names);
  }
  return (
    component.kind === "mcp" &&
    typeof component.server_id === "string" &&
    /^[a-z][a-z0-9._-]{0,63}$/u.test(component.server_id) &&
    Array.isArray(component.tool_names) &&
    component.tool_names.length === 0
  );
}

/** Host-written setup notes for an MCP connection, such as a provider that needs setup. */
function validMcpNotices(notices) {
  if (notices === undefined) return [];
  if (!Array.isArray(notices) || notices.length > 8 ||
      notices.some((notice) => typeof notice !== "string" || notice.length === 0 || notice.length > 400)) {
    throw new Error("MCP connection notices are invalid");
  }
  return notices;
}

function validProviderIds(ids) {
  return (
    Array.isArray(ids) &&
    new Set(ids).size === ids.length &&
    ids.every((id) => typeof id === "string" && id.length > 0 && id.length <= 96)
  );
}

function validPreloadComponentIds(ids, components) {
  return (
    Array.isArray(ids) &&
    ids.length <= components.size &&
    new Set(ids).size === ids.length &&
    ids.every(
      (id) =>
        typeof id === "string" &&
        id.length > 0 &&
        id.length <= 193 &&
        components.has(id),
    )
  );
}

function textResult(value, modelValue = value) {
  return {
    // The structured value remains available in details for the UI. Keep the
    // model-facing representation compact so every tool round does not pay
    // repeated whitespace tokens for the same JSON payload.
    content: [{ type: "text", text: JSON.stringify(modelValue) }],
    details: value,
  };
}

function prepareSiblingArguments(input, container, fields) {
  if (!input || typeof input !== "object" || Array.isArray(input)) return input;
  const note = input[container];
  if (!note || typeof note !== "object" || Array.isArray(note)) return input;
  for (const key of fields) {
    if (!Object.hasOwn(note, key)) continue;
    if (!Object.hasOwn(input, key)) input[key] = note[key];
    // Repeated identical values are unambiguous. Leave differing values in
    // place for schema rejection; never choose or merge evidence for the model.
    if (isDeepStrictEqual(input[key], note[key])) delete note[key];
  }
  return input;
}

function prepareLensArguments(input, parameters) {
  const references = ["sourceReads", "worldReads", "journalReads", "previousNoteIds"];
  const prepared = prepareSiblingArguments(input, "document", references);
  const document = prepared?.document;
  if (!document || typeof document !== "object" || Array.isArray(document)) return prepared;
  // An empty unsupported section contains no text or evidence to preserve.
  // Keep substantive extras and misplaced/conflicting references for validation.
  for (const [key, value] of Object.entries(document)) {
    if (value === "" && !Object.hasOwn(parameters.properties.document.properties, key)
      && !references.includes(key)) delete document[key];
  }
  return prepared;
}

function register(pi, name, label, description, parameters, method) {
  pi.registerTool({
    name,
    label,
    description,
    parameters,
    ...(name === "note_save" ? {
      prepareArguments: (input) => prepareSiblingArguments(input, "workingNote", ["sourceReads", "worldReads", "lensReads", "previousNoteIds"]),
    } : name === "lens_update" ? {
      prepareArguments: (input) => prepareLensArguments(input, parameters),
    } : {}),
    async execute(toolCallId, input, signal) {
      return textResult(await requestBroker(method, input, signal, toolCallId));
    },
  });
}

export default function loupistTerminalExtension(originalPi) {
  try {
    registerLoupistExtension(originalPi);
  } catch {
    // Pi discards failed extensions and continues startup. Without this
    // command registration the host's settings check becomes a model prompt.
    // End the process before any incomplete extension can reach a provider.
    process.stderr.write("Loupist Chat initialization failed; start a new turn.\n");
    process.exit(76);
  }
}

function registerLoupistExtension(originalPi) {
  // First, so the token descriptor is closed even if a later bootstrap fails.
  loadBrokerConnection();
  const hostContext = loadHostContext();
  hostContext.workspace = loadWorkspaceResources(hostContext.workspace);
  registerPiSettingsControl(originalPi);
  const runOptions = loadRunOptions();
  const e2eChatDiagnostics = process.env.LOUPIST_E2E_CHAT_DIAGNOSTICS === "1";
  const emitE2eDiagnostic = (phase, status) => {
    if (!e2eChatDiagnostics) return;
    const line = `${JSON.stringify({
      type: "loupist_diagnostic",
      phase,
      ...(status === undefined ? {} : { status }),
    })}\n`;
    if (Buffer.byteLength(line, "utf8") > 200) return;
    try {
      // Pi redirects process.stdout.write to stderr in RPC mode. These small
      // E2E frames belong on the actual RPC pipe, below its atomic write limit.
      writeSync(1, line);
    } catch {
      // Diagnostic delivery must never change the run's behavior.
    }
  };
  const activeTools = new Set(hostContext.initialTools);
  const registeredToolCards = new Map();
  const mcpCards = new Map();
  const ensureWorkspaceResourcesUnchanged = registerWorkspaceResources(originalPi, hostContext.workspace, hostContext.explicitSkillNames);

  // Pi reports grouped missing properties at the first child's path. Include
  // the registered schema on rejected calls so a repair can recover the object
  // structure. message_end also sees validation failures, unlike tool_result.
  originalPi.on("message_end", ({ message }) => {
    if (message.role !== "toolResult" || message.isError !== true ||
        !activeTools.has(message.toolName) || !Array.isArray(message.content)) return;
    const prefix = `Validation failed for tool "${message.toolName}":\n`;
    if (!message.content.some((item) => item.type === "text" && item.text.startsWith(prefix))) return;
    const card = registeredToolCards.get(message.toolName);
    if (!card) return;
    const schema = JSON.stringify(card.parameters);
    if (Buffer.byteLength(schema, "utf8") > MAX_VALIDATION_SCHEMA_BYTES) return;
    const text = `Correct the arguments using this registered input schema. Keep sibling fields at the shown object level.\n${schema}`;
    if (message.content.some((item) => item.type === "text" && item.text === text)) return;
    return { message: { ...message, content: [...message.content, { type: "text", text }] } };
  });

  function validateMcpCards(serverId, cards) {
    if (!Array.isArray(cards) || cards.length === 0 || cards.length > MAX_MCP_TOOLS) {
      throw new Error("MCP tool inventory is invalid");
    }
    const names = new Set();
    const namespace = serverId.replace(/[.-]/gu, "_");
    for (const card of cards) {
      if (
        !card ||
        typeof card !== "object" ||
        card.server_id !== serverId ||
        typeof card.name !== "string" ||
        !/^mcp__[A-Za-z0-9_]+__[A-Za-z0-9_-]+$/u.test(card.name) ||
        !card.name.startsWith(`mcp__${namespace}__`) ||
        names.has(card.name) ||
        typeof card.mcp_name !== "string" ||
        !/^[A-Za-z0-9_.\/-]{1,128}$/u.test(card.mcp_name) ||
        typeof card.label !== "string" ||
        !card.label ||
        card.label.length > 160 ||
        typeof card.description !== "string" ||
        card.description.length > 4096 ||
        !card.parameters ||
        typeof card.parameters !== "object" ||
        Array.isArray(card.parameters) ||
        Buffer.byteLength(JSON.stringify(card.parameters), "utf8") > MAX_MCP_SCHEMA_BYTES
      ) {
        throw new Error("MCP tool inventory is invalid");
      }
      names.add(card.name);
    }
  }

  function publishActiveTools() {
    originalPi.setActiveTools(orderedToolNames(activeTools));
  }

  function transcriptActiveToolNames(ctx) {
    const messages = ctx?.sessionManager?.buildSessionContext?.().messages;
    if (!Array.isArray(messages)) return [];
    const tools = new Map();
    for (const message of messages) {
      if (!message || message.role !== "system") continue;
      for (const tool of message.toolsRemoved ?? []) {
        if (tool && typeof tool.name === "string") tools.delete(tool.name);
      }
      for (const tool of message.toolsAdded ?? []) {
        if (tool && typeof tool.name === "string") tools.set(tool.name, true);
      }
    }
    return [...tools.keys()];
  }

  // Pi's deferred tool_search was compared structurally and not adopted.
  // Activating a deferred tool still rewrites the declared tool prefix, and it
  // adds a discovery round. Restoring the previous turn's allowed components
  // keeps one tool prefix for the rest of a warm Chat.
  async function restoreStickyComponents(ctx) {
    const restored = new Set(transcriptActiveToolNames(ctx));
    if (restored.size === 0) return;
    for (const component of hostContext.components.values()) {
      if (component.kind === "mcp") {
        const namespace = String(component.server_id).replace(/[.-]/gu, "_");
        const prefix = `mcp__${namespace}__`;
        if ([...restored].some((name) => name.startsWith(prefix))) {
          try {
            await loadCapabilityComponent(component);
          } catch {
            // Reconnect can fail without discarding the rest of the warm session.
          }
        }
        continue;
      }
      for (const name of component.tool_names) {
        if (restored.has(name) && hostContext.allowedTools.has(name)) activeTools.add(name);
      }
    }
  }

  function reconcileMcpTools(component, cards) {
    validateMcpCards(component.server_id, cards);
    const orderedCards = [...cards].sort((left, right) =>
      left.name < right.name ? -1 : left.name > right.name ? 1 : 0);
    for (const name of component.tool_names) activeTools.delete(name);
    for (const card of orderedCards) {
      const signature = JSON.stringify(card);
      if (
        mcpCards.has(card.name) &&
        mcpCards.get(card.name).serverId !== card.server_id
      ) {
        throw new Error("MCP tool namespace collision");
      }
      if (mcpCards.get(card.name)?.signature !== signature) {
        registeredToolCards.set(card.name, {
          name: card.name,
          label: card.label,
          description: card.description,
          parameters: card.parameters,
        });
        originalPi.registerTool({
          name: card.name,
          label: card.label,
          description: card.description,
          parameters: card.parameters,
          async execute(toolCallId, input, signal) {
            const response = await requestBroker(
              "mcp.call",
              {
                server_id: card.server_id,
                tool_name: card.mcp_name,
                arguments: input,
              },
              signal,
              toolCallId,
            );
            return textResult(acceptMcpCallResponse(component, response));
          },
        });
        mcpCards.set(card.name, { signature, serverId: card.server_id });
      }
      activeTools.add(card.name);
    }
    component.tool_names = orderedCards.map((card) => card.name);
    component.loaded = true;
    publishActiveTools();
  }

  function unloadMcpTools(component) {
    for (const name of component.tool_names) activeTools.delete(name);
    component.tool_names = [];
    component.loaded = false;
    publishActiveTools();
  }

  function acceptMcpCallResponse(component, response) {
    if (!response || typeof response !== "object" || Array.isArray(response)) {
      throw new Error("MCP call response is invalid");
    }
    if (response.session_stopped !== undefined) {
      if (
        response.session_stopped !== true ||
        response.call_error !== true ||
        response.tools !== undefined ||
        response.result !== undefined
      ) {
        throw new Error("MCP call response is invalid");
      }
      unloadMcpTools(component);
      const detail = typeof response.message === "string" && response.message
        ? response.message
        : "Bundled MCP runtime stopped.";
      const next = typeof response.next === "string" && response.next
        ? response.next
        : "Load this capability again to reconnect. If it stops again, report that the connection is unavailable.";
      throw new Error(`${detail} ${next}`);
    }
    if (response.tools !== undefined) {
      reconcileMcpTools(component, response.tools);
    }
    if (response.call_error !== undefined) {
      if (response.call_error !== true || response.result !== undefined) {
        throw new Error("MCP call response is invalid");
      }
      const detail = typeof response.message === "string" && response.message
        ? response.message
        : "The tool call failed.";
      const next = typeof response.next === "string" && response.next
        ? response.next
        : "Change the request before trying the same call again.";
      throw new Error(`Tool call failed. ${detail} ${next}`);
    }
    if (!Object.hasOwn(response, "result")) {
      throw new Error("MCP call response is invalid");
    }
    return response.result;
  }

  const pi = {
    registerTool(tool) {
      if (!hostContext.allowedTools.has(tool.name)) return;
      registeredToolCards.set(tool.name, {
        name: tool.name,
        label: tool.label,
        description: tool.description,
        parameters: tool.parameters,
      });
      originalPi.registerTool(tool);
    },
  };
  // Rust owns both unattended policy and turn taint. Every native execution
  // asks the same host gate, so a script cannot bypass a tainted-turn refusal.
  // Write and edit also name their resolved target, so Rust can keep a
  // tainted turn from rewriting instruction and Skill files.
  registerWorkspaceTools(pi, hostContext.workspace.root,
    (name, path, signal, id) => requestBroker("native.authorize",
      path === undefined ? { name } : { name, path }, signal, id));
  async function loadCapabilityComponent(component, signal) {
    if ((component.kind === "mcp" && component.loaded === true) ||
        (component.kind !== "mcp" && component.tool_names.every((name) => activeTools.has(name)))) {
      return { id: component.id, loaded: true, already_loaded: true,
        message: "Already loaded; its tools are callable. Call the tool directly." };
    }
    if (component.kind === "mcp") {
      const connected = await requestBroker(
        "mcp.connect",
        { server_id: component.server_id },
        signal,
      );
      if (connected.server_id !== component.server_id) {
        throw new Error("MCP runtime identity mismatch");
      }
      const notices = validMcpNotices(connected.notices);
      reconcileMcpTools(component, connected.tools);
      return {
        id: component.id,
        kind: component.kind,
        name: component.name,
        server_id: component.server_id,
        tool_names: component.tool_names,
        ...(notices.length > 0 ? { notices } : {}),
        tools: component.tool_names.map((name) => registeredToolCards.get(name)).filter(Boolean),
      };
    }
    for (const name of component.tool_names) activeTools.add(name);
    publishActiveTools();
    return {
      id: component.id,
      kind: component.kind,
      name: component.name,
      tool_names: component.tool_names,
      tools: component.tool_names.map((name) => registeredToolCards.get(name)).filter(Boolean),
    };
  }

  originalPi.on("session_start", async (_event, ctx) => {
    await restoreStickyComponents(ctx);
    publishActiveTools();
  });
  originalPi.on("before_agent_start", async (event) => {
    emitE2eDiagnostic("before_agent_start");
    ensureWorkspaceResourcesUnchanged();
    // Ids were validated against the sealed component map at startup.
    for (const id of hostContext.preloadComponentIds) {
      await loadCapabilityComponent(hostContext.components.get(id));
    }
    ensureWorkspaceResourcesUnchanged();
    applyWorkspacePrompt(event, hostContext.workspace, hostContext.promptText);
  });
  originalPi.on("before_provider_request", (event, ctx) => {
    emitE2eDiagnostic("before_provider_request");
    return applyRunOptions(ctx.model, event.payload, runOptions);
  });
  originalPi.on("before_provider_headers", () => {
    emitE2eDiagnostic("before_provider_headers");
  });
  originalPi.on("after_provider_response", (event) => {
    if (Number.isInteger(event.status) && event.status >= 100 && event.status <= 599) {
      emitE2eDiagnostic("provider_response", event.status);
      if (event.status !== 429 && event.status !== 503) return;
      // A busy response's timing headers, and only those, reach Rust, which
      // reads and bounds the wait (`pi/provider_failure.rs`, RETRY_HEADERS).
      const headers = {};
      for (const [name, value] of Object.entries(event.headers ?? {})) {
        const key = name.toLowerCase();
        if (RETRY_HEADERS.has(key) && typeof value === "string" && value.length <= 64) headers[key] = value;
      }
      const line = `${JSON.stringify({ type: "loupist_retry_hints", headers })}\n`;
      // Below the pipe's atomic write size, like the diagnostics above.
      if (Buffer.byteLength(line, "utf8") > 500) return;
      try {
        // Pi redirects process.stdout in RPC mode; this frame belongs on the RPC pipe.
        writeSync(1, line);
      } catch {
        // The RPC stream owns transport failures; a lost hint falls back to the fixed waits.
      }
    }
  });

  pi.registerTool({
    name: "capability_load",
    label: "Load a capability",
    description: TOOL_SPECS.get("capability_load").description,
    parameters: {
      type: "object",
      additionalProperties: false,
      required: ["id"],
      properties: {
        id: { type: "string", minLength: 1, maxLength: 193 },
      },
    },
    async execute(_toolCallId, input, signal) {
      const component = resolveCapability(input?.id, hostContext.components,
        hostContext.unavailableConnections,
        (originalPi.getCommands?.() ?? []).filter((command) => command.source === "skill").map((command) => command.name));
      const loaded = await loadCapabilityComponent(component, signal);
      // Pi sends active definitions on the next provider request. Repeating the
      // same schemas in the transcript charges every following tool round for
      // a second copy; retain the complete cards only in local UI details.
      const { tools: _tools, ...receipt } = loaded;
      return textResult(loaded, { ...receipt, loaded: true });
    },
  });

  for (const tool of TOOL_CATALOG.tools) {
    if (tool.method) {
      register(pi, tool.name, tool.label, tool.description,
        structuredClone(tool.parameters), tool.method);
    }
  }
}
