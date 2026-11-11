import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { findEnvKeys } from "@earendil-works/pi-ai/compat";
import { builtinProviders, getBuiltinModel } from "@earendil-works/pi-ai/providers/all";
import { nativeSearchProviderIds } from "./loupist-native-search.mjs";
import { isGuardedInferenceApi } from "./provider-inference.mjs";

// Rust reads the same file (`providers/manifest.rs`); these checks hold it to
// the pinned Pi package the app bundles.
const manifest = JSON.parse(readFileSync(new URL("../provider-manifest.json", import.meta.url)));
const pi = new Map(builtinProviders().map((provider) => [provider.id, provider]));

for (const entry of manifest.providers) {
  test(`provider manifest matches pinned Pi: ${entry.id}`, () => {
    const provider = pi.get(entry.id);
    assert.ok(provider, `Pi must offer ${entry.id}`);
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

test("native search adapters are exactly the manifest's native-search providers", () => {
  assert.deepEqual(
    nativeSearchProviderIds().sort(),
    manifest.providers.filter((entry) => entry.native_search).map((entry) => entry.id).sort(),
  );
});

test("the support launch bootstrap model is in the pinned Pi catalog", () => {
  const { provider, model } = manifest.support_bootstrap;
  const entry = manifest.providers.find((candidate) => candidate.id === provider);
  assert.ok(entry?.api_key && !entry.development_only, "the bootstrap provider takes an API key");
  assert.ok(getBuiltinModel(provider, model), `Pi must supply ${provider}/${model}`);
});
