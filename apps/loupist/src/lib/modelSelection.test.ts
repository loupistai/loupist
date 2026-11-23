import { modelRunSelection, resolveExplicitSelection as resolve, usableCatalogModel } from "./modelSelection";
import type { ModelCatalog } from "../types";

const luna: ModelCatalog["models"][number] = {
  id: "pi/luna", name: "Luna", provider: "openai-codex", model: "gpt-5.6-luna", input: ["text"], reasoning: true,
  context_window: 272_000, max_tokens: 128_000, thinking_levels: ["deliberate", "max"], thinking_level_map: { deliberate: "d", max: "m" },
  run_controls: [], credential_source: "saved",
};
const catalog: ModelCatalog = { hidden_model_profile_ids: [], models: [luna, { ...luna, id: "pi/sol", name: "Sol" }], providers: [], custom_providers: [], model_notice: null };
const empty = { model_profile_id: "", thinking_level: "", run_options: {} };

it("uses the host identity and controls even when another model comes first in the catalog", () => {
  expect(modelRunSelection({ model: catalog.models[1]!, thinking_level: "max", thinking_levels: ["max"], run_options: {}, run_controls: [], is_default: true, unavailable_reason: null, stored_unavailable: null, stored_model: null }))
    .toEqual({ model_profile_id: "pi/sol", thinking_level: "max", run_options: {} });
  expect(modelRunSelection(undefined)).toEqual(empty);
});

it("fills a model's omitted controls and keeps supplied choices", () => {
  const resolved = {
    model: luna,
    thinking_level: "max",
    thinking_levels: luna.thinking_levels,
    run_controls: [{
      id: "performance", label: "Performance", default_choice: "standard",
      choices: [
        { id: "standard", label: "Standard", description: "Use the standard tier." },
        { id: "fast", label: "Fast", description: "Use the priority tier." },
      ],
    }],
    is_default: false,
    unavailable_reason: null,
    run_options: {},
    stored_unavailable: null,
    stored_model: null,
  };
  expect(modelRunSelection(resolved)).toMatchObject({ model_profile_id: luna.id, run_options: { performance: "standard" } });
  expect(modelRunSelection({ ...resolved, run_options: { performance: "fast" } }).run_options).toEqual({ performance: "fast" });
  expect(modelRunSelection({ ...resolved, model: null, unavailable_reason: "account_disconnected" })).toEqual(empty);
});

it("resolves an explicit choice only to that model while Chat can answer with it", () => {
  expect(resolve(catalog, { model_profile_id: "pi/luna", thinking_level: "removed-level", run_options: {} })).toMatchObject({ model_profile_id: "pi/luna", thinking_level: "" });
  // Removed, hidden or signed out: never another catalog model; the caller falls back to the host default.
  expect(resolve(catalog, { model_profile_id: "removed/profile", thinking_level: "max", run_options: {} })).toEqual(empty);
  expect(resolve({ ...catalog, hidden_model_profile_ids: ["pi/luna"] }, { model_profile_id: "pi/luna", thinking_level: "max", run_options: {} })).toEqual(empty);
  const signedOut = { ...catalog, models: [{ ...luna, credential_source: "missing" as const }, catalog.models[1]!] };
  expect(resolve(signedOut, { model_profile_id: "pi/luna", thinking_level: "max", run_options: {} })).toEqual(empty);
  expect(usableCatalogModel(signedOut, "pi/luna")).toBeUndefined();
  expect(usableCatalogModel(signedOut, "pi/sol")?.id).toBe("pi/sol");
  expect(usableCatalogModel(null, "pi/sol")).toBeUndefined();
});
