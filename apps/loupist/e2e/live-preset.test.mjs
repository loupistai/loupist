import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { livePreset, selectionMatches, thinkingLabel, thinkingLabels } from "./live-preset.mjs";

test("live journey identity and effort follow the app defaults", () => {
  const { providers: [provider] } = JSON.parse(readFileSync(new URL("../chat-model-defaults.json", import.meta.url)));
  const preset = livePreset({});
  assert.equal(preset.modelId, `${provider.provider}/${provider.models[0].id}`);
  assert.equal(preset.effort, provider.models[0].thinking_level);
  assert.ok(selectionMatches(`${preset.modelLabel} · ${thinkingLabel(preset.effort)}`, preset));
});

test("live preset accepts explicit overrides and rejects empty or multiline labels", () => {
  const preset = livePreset({ LOUPIST_E2E_LIVE_MODEL_ID: "anthropic/test-model", LOUPIST_E2E_LIVE_MODEL_LABEL: "Test model", LOUPIST_E2E_LIVE_EFFORT: "high" });
  assert.deepEqual(preset, { modelId: "anthropic/test-model", modelLabel: "Test model", effort: "high" });
  assert.ok(selectionMatches("Test model · High", preset));
  assert.ok(!selectionMatches("Other model · High", preset));
  assert.ok(!selectionMatches("Test model · Low", preset));
  for (const key of ["LOUPIST_E2E_LIVE_MODEL_ID", "LOUPIST_E2E_LIVE_MODEL_LABEL", "LOUPIST_E2E_LIVE_EFFORT"]) {
    for (const value of ["", "a\nb", "x".repeat(81)]) assert.throws(() => livePreset({ [key]: value }), /visible menu label/);
  }
  assert.equal(thinkingLabel("xhigh"), "Extra high");
  assert.ok(thinkingLabels("high").includes("높음"));
});
