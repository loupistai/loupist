// One home for the live Chat preset. The composer trigger renders
// "<model name> · <thinking label>". Effort stays the stored id; the menu
// shows a localized thinking label, so accept both supported UI languages.
import { thinkingLabel } from "../src/thinking-labels.mjs";
import { readFileSync } from "node:fs";

const DEFAULT_MODEL_LABEL = "GPT-6-Luna";
const DEFAULT_EFFORT = "max";
const koreanThinking = JSON.parse(readFileSync(new URL("../src/i18n/locales/ko/settings.json", import.meta.url), "utf8")).modelSelection.thinking;

export { thinkingLabel };
export function thinkingLabels(level) {
  return [...new Set([thinkingLabel(level), koreanThinking[level], level].filter(Boolean))];
}

function labelParts(label) {
  return String(label ?? "")
    .split(" · ")
    .map((part) => part.trim())
    .filter(Boolean);
}

function requiredLabel(value, envName, fallback) {
  const label = (value ?? fallback).trim();
  if (!label || label.length > 80 || /[\u0000-\u001f]/.test(label)) {
    throw new Error(`${envName} must be a single visible menu label`);
  }
  return label;
}

export function livePreset(env = process.env) {
  return Object.freeze({
    modelLabel: requiredLabel(
      env.LOUPIST_E2E_LIVE_MODEL_LABEL,
      "LOUPIST_E2E_LIVE_MODEL_LABEL",
      DEFAULT_MODEL_LABEL,
    ),
    effort: requiredLabel(
      env.LOUPIST_E2E_LIVE_EFFORT,
      "LOUPIST_E2E_LIVE_EFFORT",
      DEFAULT_EFFORT,
    ),
  });
}

export function runLabelMatches(label, modelLabel, effort) {
  const parts = labelParts(label);
  return (
    parts.includes(modelLabel) &&
    parts.some((part) => thinkingLabels(effort).includes(part))
  );
}

export function selectionMatches(label, preset = livePreset()) {
  return runLabelMatches(label, preset.modelLabel, preset.effort);
}

export function assertPresetSelection(label, context = "Chat") {
  const preset = livePreset();
  if (!selectionMatches(label, preset)) {
    throw new Error(
      `${context}: expected ${preset.modelLabel} · ${thinkingLabels(preset.effort).join("/")}, visible label was ${JSON.stringify(String(label ?? ""))}`,
    );
  }
  return preset;
}
