/** Preview fixtures for custom connections and a connection set up from Chat. */
import { FIXTURE_TIME, prepareFixture, PreviewBridge } from "../fixtures";

function customConnectionFixture(id = "preview-custom-ready"): import("../../contracts/customConnections").CustomConnection {
  return { id, kind: "custom", name: "Hanul investor research", transport: { kind: "http", url: "https://research.example/mcp" }, command_line: null, dont_save_to_memory: true, pending: false, enabled: true, needs_review: false, auth_state: "not_required", env_keys: [], header_names: [], last_error: null,
    tools: [
      { name: "read_company_filings", server_description: "Read public company filings and quarterly operating results. Returns dated source links and reporting periods.", input_schema: { type: "object" }, annotations: { readOnlyHint: true }, definition_hash: "a".repeat(64), definition_changed: false, eligibility: "read_only", approval: "always" },
      { name: "save_research_note", server_description: "Save a research note to the server’s shared workspace.", input_schema: { type: "object" }, annotations: null, definition_hash: "b".repeat(64), definition_changed: false, eligibility: "not_read_only", approval: null },
      { name: "place_payment", server_description: "Send a payment to the selected account.", input_schema: { type: "object" }, annotations: { readOnlyHint: true }, definition_hash: "c".repeat(64), definition_changed: false, eligibility: "financial_action", approval: null },
    ] };
}

export async function prepareCustomConnectionFixture(bridge: PreviewBridge, scenario: string) {
  await prepareFixture(bridge, scenario === "chat-needs-approval" ? "answer" : undefined);
  const ready = customConnectionFixture();
  const changed = { ...customConnectionFixture("preview-custom-changed"), name: "Morrow research tools", needs_review: true };
  changed.tools[0] = { ...changed.tools[0], approval: null, definition_changed: true, definition_hash: "d".repeat(64) };
  const signIn = { ...customConnectionFixture("preview-custom-signin"), name: "Private company research", auth_state: "sign_in_needed" as const, tools: [] };
  const records = scenario === "connections-custom-empty" ? [] : [ready, changed, signIn];
  const copy = <T,>(value: T) => structuredClone(value);
  bridge.customConnectionList = async () => copy(records);
  bridge.customConnectionAddUrl = async request => {
    const record = { ...customConnectionFixture("preview-custom-new"), name: request.name || "Company research server", transport: { kind: "http" as const, url: request.url }, pending: true, tools: [] };
    records.push(record); return copy(record);
  };
  bridge.customConnectionConfirm = async ({ connection_id }) => {
    const record = records.find(item => item.id === connection_id)!;
    Object.assign(record, { pending: scenario === "connection-add-url-signin", needs_review: true, auth_state: scenario === "connection-add-url-signin" ? "sign_in_needed" : "not_required", tools: scenario === "connection-add-url-signin" ? [] : customConnectionFixture().tools.map(tool => ({ ...tool, approval: null })) });
    return copy(record);
  };
  let cancelSignIn: (() => void) | undefined;
  bridge.customConnectionSignIn = ({ connection_id }) => new Promise(resolve => {
    const record = records.find(item => item.id === connection_id)!;
    record.auth_state = "in_progress";
    cancelSignIn = () => { record.auth_state = "sign_in_needed"; resolve(copy(record)); };
  });
  bridge.customConnectionSignInCancel = async () => { cancelSignIn?.(); };
  bridge.customConnectionAddJson = async () => {
    const remote = { ...customConnectionFixture("preview-import-remote"), name: "Company investor data", pending: true, tools: [], header_names: ["Authorization"] };
    const local = { ...customConnectionFixture("preview-import-local"), name: "Local research archive", pending: true, transport: { kind: "stdio" as const, command: "npx", args: ["@example/research-archive@1.2.3", "--read-only"] }, command_line: "npx @example/research-archive@1.2.3 --read-only", env_keys: ["RESEARCH_API_KEY"], tools: [] };
    records.push(remote, local); return copy([remote, local]);
  };
  bridge.customToolApprove = async request => {
    const record = records.find(item => item.id === request.connection_id)!;
    record.tools = record.tools.map(tool => tool.name === request.tool_name ? { ...tool, approval: request.choice, definition_changed: false } : tool);
    record.needs_review = record.tools.some(tool => tool.eligibility === "read_only" && !tool.approval);
    return copy(record);
  };
  bridge.customConnectionSetEnabled = async request => { const record = records.find(item => item.id === request.connection_id)!; record.enabled = request.enabled; return copy(record); };
  bridge.customConnectionSetMemory = async request => { const record = records.find(item => item.id === request.connection_id)!; record.dont_save_to_memory = request.dont_save_to_memory; return copy(record); };
  bridge.customConnectionRemove = async request => { const index = records.findIndex(item => item.id === request.connection_id); if (index >= 0) records.splice(index, 1); };
  if (scenario === "chat-needs-approval") {
    const select = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => { const workspace = await select(loupistId); const answer = workspace.threads[0].messages.find(item => item.id === "preview-answer"); if (answer) Object.assign(answer, { tool_refusals: [{ connection_id: changed.id, connection_name: changed.name, tool_name: changed.tools[0].name, reason: "needs_approval" }] }); return workspace; };
  }
}

/** The answer set up FRED from Chat: its `connection_setup` step leaves a card whose state the card reads live. */
export async function prepareConnectionSetupFixture(bridge: PreviewBridge, state: "needs_setup" | "ready") {
  await prepareFixture(bridge, "answer");
  if (state === "ready") {
    await bridge.marketplaceCredentialSave({ entry_id: "fred.macro", secrets: { api_key: "preview-fred-key-0123" } });
    await bridge.marketplaceCredentialVerify({ entry_id: "fred.macro" });
    await bridge.loupistCapabilityEnable({ loupist_id: "loupist-quality", entry_id: "fred.macro" });
  }
  const select = bridge.loupistSelect.bind(bridge);
  bridge.loupistSelect = async loupistId => {
    const workspace = await select(loupistId);
    const answer = workspace.threads[0].messages.find(item => item.id === "preview-answer");
    if (answer) Object.assign(answer, { progress: { startedAtMs: FIXTURE_TIME - 8000, finishedAtMs: FIXTURE_TIME - 2000, items: [
      { id: "preview-setup", kind: "tool", toolName: "connection_setup", category: "capability", operation: "generic", action: "Prepared a connection", running: "Preparing a connection", target: "fred.macro", source: { kind: "connector", name: "FRED" }, status: "succeeded", startedAtMs: FIXTURE_TIME - 8000, finishedAtMs: FIXTURE_TIME - 7000, href: null },
    ] } });
    return workspace;
  };
}
