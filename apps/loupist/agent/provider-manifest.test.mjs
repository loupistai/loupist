import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { findEnvKeys } from "@earendil-works/pi-ai/compat";
import { getSupportedThinkingLevels, InMemoryCredentialStore } from "@earendil-works/pi-ai";
import { builtinProviders, getBuiltinModel } from "@earendil-works/pi-ai/providers/all";
import { ModelRuntime } from "@earendil-works/pi-coding-agent";
import { nativeSearchProviderIds } from "./loupist-native-search.mjs";
import { codexSearchAdapter, openaiSearchAdapter } from "./native-search/codex.mjs";
import { anthropicSearchAdapter } from "./native-search/anthropic.mjs";
import { xaiSearchAdapter } from "./native-search/xai.mjs";
import { isGuardedInferenceApi } from "./provider-inference.mjs";
import { hasTrustedModelEndpoint } from "./model-run-controls.mjs";

// Rust reads the same file (`providers/manifest.rs`); these checks hold it to
// the pinned Pi package the app bundles.
const manifest = JSON.parse(readFileSync(new URL("../provider-manifest.json", import.meta.url)));
const pi = new Map(builtinProviders().map((provider) => [provider.id, provider]));

for (const entry of manifest.providers) {
  test(`provider manifest matches pinned Pi: ${entry.id}`, () => {
    const provider = pi.get(entry.id);
    assert.ok(provider, `Pi must offer ${entry.id}`);
    assert.ok(provider.getModels().every(hasTrustedModelEndpoint), `${entry.id}: bundled routes pass discovery`);
    if (entry.api_key) {
      assert.ok(provider.auth?.apiKey, `${entry.id}: Pi takes an API key`);
      // Pi reads exactly this host variable for the provider, so development
      // forwarding and the launch scrub name the right one.
      assert.deepEqual(findEnvKeys(entry.id, { [entry.api_key.environment]: "fixture" }), [entry.api_key.environment]);
    }
    if (entry.sign_in) assert.ok(provider.auth?.oauth, `${entry.id}: Pi offers a sign-in`);
    if (entry.isolated_inference) {
      assert.ok(
        provider.getModels().some((model) => isGuardedInferenceApi(model.api)),
        `${entry.id}: some model uses an API isolated inference guards`,
      );
    }
  });
}

test("OpenAI, Codex and xAI catalog routes must match the bundled base URL", () => {
  for (const provider of ["openai", "openai-codex", "xai"]) {
    const [bundled] = pi.get(provider).getModels();
    assert.ok(hasTrustedModelEndpoint(bundled), provider);
    const baseUrl = new URL("/unbundled-path", bundled.baseUrl).href;
    assert.equal(hasTrustedModelEndpoint({ ...bundled, baseUrl }), false, provider);
  }
});

test("native search adapters are exactly the manifest's native-search providers", () => {
  assert.deepEqual(
    nativeSearchProviderIds().sort(),
    manifest.providers.filter((entry) => entry.native_search).map((entry) => entry.id).sort(),
  );
});

test("the support launch bootstrap model is in the pinned Pi catalog", () => {
  const { provider, model } = manifest.support_bootstrap;
  assert.deepEqual(manifest.support_bootstrap, { provider: "google", model: "gemini-3.5-flash-lite" });
  const entry = manifest.providers.find((candidate) => candidate.id === provider);
  assert.ok(entry?.api_key && !entry.development_only, "the bootstrap provider takes an API key");
  assert.ok(getBuiltinModel(provider, model), `Pi must supply ${provider}/${model}`);
});

test("Chat defaults and eval models resolve through Pi's bundled and public catalogs", async () => {
  const defaults = JSON.parse(readFileSync(new URL("../chat-model-defaults.json", import.meta.url)));
  // Selected rows and Last-Modified values from pi.dev/api/models/providers/{openai,openai-codex}
  // on 2026-11-18. Exercise the pinned SDK's offline restore of public metadata.
  // The snapshot fixes the test input. Auto skips defaults absent from discovery.
  const cached = JSON.parse(readFileSync(new URL("../scripts/fixtures/pi-public-openai-models.json", import.meta.url)));
  const runtime = await ModelRuntime.create({
    credentials: new InMemoryCredentialStore(),
    modelsPath: null,
    modelsStore: { read: async (provider) => cached[provider] },
    allowModelNetwork: false,
  });
  for (const group of defaults.providers) {
    for (const { id, thinking_level } of group.models) {
      const model = runtime.getModel(group.provider, id);
      assert.ok(model, `${group.provider}/${id}`);
      assert.ok(hasTrustedModelEndpoint(model), `${group.provider}/${id}: trusted request route`);
      assert.ok(getSupportedThinkingLevels(model).includes(thinking_level), `${group.provider}/${id}: ${thinking_level}`);
    }
  }
  for (const id of ["gpt-6.2-sol", "gpt-6.2-astra"]) {
    const model = runtime.getModel("openai-codex", id);
    assert.equal(model?.api, "openai-codex-responses");
    assert.ok(getSupportedThinkingLevels(model).includes("medium"), `openai-codex/${id}`);
  }
});

test("native-search allowlists exist in the bundled Pi catalog", () => {
  for (const adapter of [openaiSearchAdapter, codexSearchAdapter, anthropicSearchAdapter, xaiSearchAdapter]) {
    assert(adapter.modelIds.length > 0);
    const models = adapter.modelIds.map((id) => {
      const model = getBuiltinModel(adapter.id, id);
      assert.ok(model, `${adapter.id}/${id}`);
      return model;
    });
    assert.doesNotMatch(models[0].id, /(?:^|[-.])(?:sol|astra|opus|sonnet)(?:[-.]|$)/u,
      `${adapter.id}: search must start with a low-cost model`);
    // Pin ordinary uncached input/output rates; equal-cost entries prefer newer models.
    const prices = models.map((model) => model.cost.input + model.cost.output);
    assert.deepEqual(prices, [...prices].sort((left, right) => left - right),
      `${adapter.id}: native-search models must remain cheapest-first`);
  }
});
