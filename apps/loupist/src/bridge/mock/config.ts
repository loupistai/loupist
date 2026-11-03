import { DEFAULT_AGENT_SKILL_IDS } from "../../lib/agentSkills";
import type { ConfigDocument, ConfigScope, ConfigSnapshot, ConfigSource, ConfigUpdateRequest, EffectiveConfig } from "../../contracts/config";

type ObjectValue = Record<string, unknown>;
const object = (value: unknown): value is ObjectValue => value !== null && typeof value === "object" && !Array.isArray(value);

function merge(target: ObjectValue, patch: ObjectValue, source?: ConfigSource, sources: Record<string, ConfigSource> = {}, prefix = "") {
  for (const [key, value] of Object.entries(patch)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (object(value)) {
      if (!object(target[key])) target[key] = {};
      merge(target[key] as ObjectValue, value, source, sources, path);
    } else {
      target[key] = structuredClone(value);
      if (source) sources[path] = source;
    }
  }
}

const defaults = (): EffectiveConfig => ({
  appearance: { theme: "system", language: "system" }, models: { hidden_model_profile_ids: [] }, connectors: {},
  chat: { model_profile_id: null, thinking_level: null, run_options: {}, use_memory: true, update_memory: true },
  skills: Object.fromEntries(DEFAULT_AGENT_SKILL_IDS.map((id) => [id, { enabled: id !== "skill-creator" && id !== "skill-installer" }])),
  pi: { compaction: { reserve_tokens: 16384, keep_recent_tokens: 20000 }, retry: { enabled: true, max_retries: 3, base_delay_ms: 2000, max_agent_delay_ms: 60000 } },
});

export class MockConfig {
  private documents = new Map<string, ConfigDocument>();
  private revisions = new Map<string, number>();

  read(scope: ConfigScope, workspace?: string): ConfigSnapshot {
    const key = scope.kind === "global" ? "global" : scope.loupistId;
    const effective = {} as EffectiveConfig;
    const sources: Record<string, ConfigSource> = {};
    merge(effective, defaults(), "default", sources);
    merge(effective, this.documents.get("global") ?? {}, "global", sources);
    if (scope.kind === "project") merge(effective, this.documents.get(key) ?? {}, "project", sources);
    for (const [id, skill] of Object.entries(effective.skills)) {
      if (skill.enabled === undefined) { skill.enabled = true; sources[`skills.${id}.enabled`] = "default"; }
    }
    return { scope, path: scope.kind === "global" ? "/Users/preview/.loupist/config.toml" : `${workspace}/.loupist/config.toml`, revision: String(this.revisions.get(key) ?? 0), explicit: structuredClone(this.documents.get(key) ?? {}), effective, sources, diagnostics: [] };
  }

  update(request: ConfigUpdateRequest, workspace?: string) {
    const snapshot = this.read(request.scope, workspace);
    if (snapshot.revision !== request.expectedRevision) throw new Error("Configuration changed on disk. Reload before saving again.");
    if (request.scope.kind === "project" && ["appearance", "models", "connectors"].some((key) => key in request.set || request.unset.some((path) => path[0] === key))) throw new Error("This setting is only available globally.");
    for (const [id, skill] of Object.entries(request.set.skills ?? {})) {
      if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(id) || ["world", "lens", "journal", "visualize"].includes(id) || skill.enabled !== undefined && typeof skill.enabled !== "boolean") throw new Error("Unknown or fixed app skill.");
    }
    const document = snapshot.explicit ?? {};
    for (const path of request.unset) {
      let parent: ObjectValue = document;
      for (const key of path.slice(0, -1)) {
        if (!object(parent[key])) break;
        parent = parent[key] as ObjectValue;
      }
      delete parent[path.at(-1) ?? ""];
    }
    merge(document, request.set);
    for (const [key, value] of Object.entries(document.pi?.compaction ?? {})) {
      if (!Number.isSafeInteger(value) || value < 0) throw new Error(`${key} must be a nonnegative safe integer.`);
    }
    for (const [key, value] of Object.entries(document.pi?.retry ?? {})) {
      if (key !== "enabled" && (!Number.isSafeInteger(value) || Number(value) < 0 || Number(value) > (key === "max_retries" ? 10 : 300000))) throw new Error(`${key} is outside the supported range.`);
    }
    const key = request.scope.kind === "global" ? "global" : request.scope.loupistId;
    this.documents.set(key, document);
    this.revisions.set(key, Number(snapshot.revision) + 1);
    return this.read(request.scope, workspace);
  }
}
