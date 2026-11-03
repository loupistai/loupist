import type {
  CustomConnection, CustomConnectionsBridge, CustomConnectionTransport, CustomToolApproval,
} from "../../contracts/customConnections";

/** UI fixtures only. Rust owns the real transport, credentials, and approval policy. */
export class MockCustomConnections implements CustomConnectionsBridge {
  private records = new Map<string, CustomConnection>();
  private enabled = new Map<string, Set<string>>();
  private approvals = new Map<string, CustomToolApproval>();
  private nextId = 1;

  private record(id: string) {
    const record = this.records.get(id);
    if (!record) throw new Error("Custom connection was not found");
    return record;
  }

  private view(id: string, loupist?: string): CustomConnection {
    const record = structuredClone(this.record(id));
    record.enabled = !!loupist && !!this.enabled.get(id)?.has(loupist);
    record.tools = record.tools.map(tool => ({ ...tool, approval: loupist ? this.approvals.get(`${loupist}:${id}:${tool.name}`) ?? null : null }));
    record.needs_review = record.tools.some(tool => tool.eligibility === "read_only" && !tool.approval);
    return record;
  }

  private add(name: string, transport: CustomConnectionTransport, pending: boolean, env: string[] = [], headers: string[] = []) {
    const id = `custom-${(this.nextId++).toString(16).padStart(32, "0")}`;
    const commandLine = transport.kind === "stdio" ? [transport.command, ...transport.args].map(arg => /^[\w./@=:+,-]+$/.test(arg) ? arg : `'${arg.replaceAll("'", "'\\''")}'`).join(" ") : null;
    this.records.set(id, { id, kind: "custom", name, transport, command_line: commandLine,
      dont_save_to_memory: true, pending, enabled: false, needs_review: true, auth_state: "not_required", tools: [],
      env_keys: env, header_names: headers, last_error: null });
    if (!pending) this.populate(id);
    return this.view(id);
  }

  private populate(id: string) {
    const record = this.record(id);
    record.pending = false;
    record.tools = [{ name: "read_prices", server_description: "Read market prices.", input_schema: { type: "object", properties: { symbol: { type: "string" } }, required: ["symbol"] }, annotations: { readOnlyHint: true }, definition_hash: "a".repeat(64), definition_changed: false, eligibility: "read_only", approval: null },
      { name: "place_order", server_description: "Place an order.", input_schema: { type: "object" }, annotations: { readOnlyHint: true }, definition_hash: "b".repeat(64), definition_changed: false, eligibility: "financial_action", approval: null }];
  }

  async customConnectionAddUrl(request: Parameters<CustomConnectionsBridge["customConnectionAddUrl"]>[0]) {
    const url = new URL(request.url);
    if (url.username || url.password || url.hash || url.search || !(url.protocol === "https:" || (url.protocol === "http:" && ["localhost", "127.0.0.1"].includes(url.hostname)))) throw new Error("Use HTTPS or localhost HTTP");
    return this.add(request.name ?? url.hostname, { kind: "http", url: url.pathname === "/" ? url.origin : url.href }, true, [], Object.keys(request.headers ?? {}));
  }

  async customConnectionAddCommand(request: Parameters<CustomConnectionsBridge["customConnectionAddCommand"]>[0]) {
    if (["npx", "uvx", "pipx"].includes(request.command.split(/[\\/]/).at(-1) ?? "") && !request.args.some(arg => /(?:@|==)\d+\.\d+\.\d+(?:[-+][\w.-]+)?$/.test(arg))) throw new Error("An exact package version is required");
    return this.add(request.name ?? request.command, { kind: "stdio", command: request.command, args: [...request.args] }, true, Object.keys(request.env));
  }

  async customConnectionAddJson(request: { json: string }) {
    const input: unknown = JSON.parse(request.json);
    if (!input || typeof input !== "object" || !("mcpServers" in input) || !input.mcpServers || typeof input.mcpServers !== "object") throw new Error("Expected mcpServers object");
    const result: CustomConnection[] = [];
    for (const [name, value] of Object.entries(input.mcpServers)) {
      if (!value || typeof value !== "object") throw new Error("Invalid server entry");
      const entry = value as { url?: string; command?: string; args?: string[]; env?: Record<string, string>; headers?: Record<string, string> };
      if (entry.url) {
        const created = await this.customConnectionAddUrl({ url: entry.url, name, headers: entry.headers });
        this.record(created.id).pending = true;
        this.record(created.id).tools = [];
        result.push(this.view(created.id));
      } else if (entry.command) result.push(await this.customConnectionAddCommand({ command: entry.command, args: entry.args ?? [], env: entry.env ?? {}, name }));
      else throw new Error("Expected URL or command");
    }
    return result;
  }

  async customConnectionConfirm(request: Parameters<CustomConnectionsBridge["customConnectionConfirm"]>[0]) {
    this.populate(request.connection_id);
    if (request.loupist_id) {
      const enabled = this.enabled.get(request.connection_id) ?? new Set<string>();
      enabled.add(request.loupist_id);
      this.enabled.set(request.connection_id, enabled);
    }
    return this.view(request.connection_id, request.loupist_id);
  }
  async customConnectionList(request: { loupist_id?: string } = {}) { return [...this.records.keys()].map(id => this.view(id, request.loupist_id)); }
  async customConnectionRemove({ connection_id }: Parameters<CustomConnectionsBridge["customConnectionRemove"]>[0]) { this.records.delete(connection_id); this.enabled.delete(connection_id); for (const key of this.approvals.keys()) if (key.includes(`:${connection_id}:`)) this.approvals.delete(key); }
  async customConnectionSetMemory(request: Parameters<CustomConnectionsBridge["customConnectionSetMemory"]>[0]) { this.record(request.connection_id).dont_save_to_memory = request.dont_save_to_memory; return this.view(request.connection_id); }
  async customConnectionSetEnabled(request: Parameters<CustomConnectionsBridge["customConnectionSetEnabled"]>[0]) {
    const enabled = this.enabled.get(request.connection_id) ?? new Set<string>();
    if (request.enabled) enabled.add(request.loupist_id); else enabled.delete(request.loupist_id);
    this.enabled.set(request.connection_id, enabled);
    return this.view(request.connection_id, request.loupist_id);
  }
  async customConnectionAuthorize(request: Parameters<CustomConnectionsBridge["customConnectionAuthorize"]>[0]) { return this.customConnectionConfirm(request); }
  async customConnectionSignIn(request: Parameters<CustomConnectionsBridge["customConnectionSignIn"]>[0]) { this.record(request.connection_id).auth_state = "signed_in"; return this.customConnectionConfirm(request); }
  async customConnectionSignInCancel(request: Parameters<CustomConnectionsBridge["customConnectionSignInCancel"]>[0]) { this.record(request.connection_id).auth_state = "sign_in_needed"; }
  async customToolApprove(request: Parameters<CustomConnectionsBridge["customToolApprove"]>[0]) {
    const tool = this.record(request.connection_id).tools.find(tool => tool.name === request.tool_name);
    if (!tool || tool.eligibility !== "read_only" || tool.definition_hash !== request.definition_hash) throw new Error("Tool needs review");
    this.approvals.set(`${request.loupist_id}:${request.connection_id}:${request.tool_name}`, request.choice);
    return this.view(request.connection_id, request.loupist_id);
  }
}
