import { resolveConfiguredModelRunSelection as resolve } from "./modelSelection";
import type { ModelCatalog } from "../types";

const luna: ModelCatalog["models"][number] = {
  id: "pi/luna", name: "Luna", provider: "openai-codex", model: "gpt-5.6-luna", input: ["text"], reasoning: true,
  context_window: 272_000, max_tokens: 128_000, thinking_levels: ["deliberate", "max"], thinking_level_map: { deliberate: "d", max: "m" },
  run_controls: [], credential_source: "saved",
};
const catalog: ModelCatalog = { hidden_model_profile_ids: [], models: [luna, { ...luna, id: "pi/sol", name: "Sol" }], providers: [] };
const empty = { model_profile_id: "", thinking_level: "", run_options: {} };

it("applies configured defaults before the last used model while keeping explicit composer choices", () => {
  const defaults = { model_profile_id: "pi/sol", thinking_level: "max", run_options: {} };
  expect(resolve(catalog, undefined, defaults, "pi/luna")).toMatchObject({ model_profile_id: "pi/sol", thinking_level: "max" });
  const explicit = { model_profile_id: "pi/luna", thinking_level: "deliberate", run_options: {} };
  expect(resolve(catalog, explicit, defaults, "pi/sol")).toEqual(explicit);
  expect(resolve(catalog, undefined, { ...defaults, model_profile_id: null }, "pi/luna").model_profile_id).toBe("pi/luna");
  expect(resolve(catalog, undefined, undefined)).toEqual({ model_profile_id: "pi/luna", thinking_level: "deliberate", run_options: {} });
});

it("blocks unavailable configured defaults instead of replacing them", () => {
  const defaults = { model_profile_id: "pi/luna", thinking_level: "max", run_options: {} };
  expect(resolve(catalog, undefined, { ...defaults, model_profile_id: "missing/model" })).toEqual(empty);
  expect(resolve({ ...catalog, hidden_model_profile_ids: ["pi/luna"] }, undefined, defaults)).toEqual(empty);
  expect(resolve(catalog, undefined, { ...defaults, thinking_level: "unsupported" })).toEqual(empty);
  expect(resolve(catalog, undefined, { ...defaults, run_options: { performance: "fast" } })).toEqual(empty);
});

it("does not silently replace a stale explicit choice, but moves past a hidden one", () => {
  expect(resolve(catalog, { model_profile_id: "removed/profile", thinking_level: "max", run_options: {} }, undefined)).toEqual(empty);
  expect(resolve(catalog, { model_profile_id: "pi/luna", thinking_level: "removed-level", run_options: {} }, undefined)).toMatchObject({ model_profile_id: "pi/luna", thinking_level: "" });
  expect(resolve({ ...catalog, hidden_model_profile_ids: ["pi/luna"] }, { model_profile_id: "pi/luna", thinking_level: "max", run_options: {} }, undefined)).toMatchObject({ model_profile_id: "pi/sol", thinking_level: "max" });
});
