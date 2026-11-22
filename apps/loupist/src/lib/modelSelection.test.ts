import { modelRunSelection, resolveExplicitSelection as resolve } from "./modelSelection";
import type { ModelCatalog } from "../types";

const luna: ModelCatalog["models"][number] = {
  id: "pi/luna", name: "Luna", provider: "openai-codex", model: "gpt-5.6-luna", input: ["text"], reasoning: true,
  context_window: 272_000, max_tokens: 128_000, thinking_levels: ["deliberate", "max"], thinking_level_map: { deliberate: "d", max: "m" },
  run_controls: [], credential_source: "saved",
};
const catalog: ModelCatalog = { hidden_model_profile_ids: [], models: [luna, { ...luna, id: "pi/sol", name: "Sol" }], providers: [], custom_providers: [], model_notice: null };
const empty = { model_profile_id: "", thinking_level: "", run_options: {} };

it("uses the host identity and controls even when another model comes first in the catalog", () => {
  expect(modelRunSelection({ model: catalog.models[1]!, thinking_level: "max", thinking_levels: ["max"], run_options: {}, run_controls: [], is_default: true, unavailable_reason: null }))
    .toEqual({ model_profile_id: "pi/sol", thinking_level: "max", run_options: {} });
  expect(modelRunSelection(undefined)).toEqual(empty);
});

it("does not silently replace a stale explicit choice, but moves past a hidden one", () => {
  expect(resolve(catalog, { model_profile_id: "removed/profile", thinking_level: "max", run_options: {} })).toEqual(empty);
  expect(resolve(catalog, { model_profile_id: "pi/luna", thinking_level: "removed-level", run_options: {} })).toMatchObject({ model_profile_id: "pi/luna", thinking_level: "" });
  expect(resolve({ ...catalog, hidden_model_profile_ids: ["pi/luna"] }, { model_profile_id: "pi/luna", thinking_level: "max", run_options: {} })).toMatchObject({ model_profile_id: "pi/sol", thinking_level: "max" });
});
