import type {
  ConfigSnapshot,
  ModelCatalog,
  ModelRunSelection,
} from "../types";
import i18n from "../i18n/index";

import type { ModelRunControl, ModelRunControlChoice } from "../types";

const thinkingKeys = {
  off: "settings:modelSelection.thinking.off", none: "settings:modelSelection.thinking.none",
  minimal: "settings:modelSelection.thinking.minimal", low: "settings:modelSelection.thinking.low",
  medium: "settings:modelSelection.thinking.medium", high: "settings:modelSelection.thinking.high",
  xhigh: "settings:modelSelection.thinking.xhigh", max: "settings:modelSelection.thinking.max",
} as const;
export function thinkingLabel(level: string): string {
  return Object.hasOwn(thinkingKeys, level)
    ? i18n.t(thinkingKeys[level as keyof typeof thinkingKeys])
    : level ? level.slice(0, 1).toUpperCase() + level.slice(1) : "";
}

// Known host controls use localized presentation; provider-defined metadata stays intact.
export const runControlLabel = (control: ModelRunControl): string => control.id === "performance"
  ? i18n.t("settings:modelSelection.performance") : control.label;
export function runChoiceLabel(control: ModelRunControl, choice: ModelRunControlChoice): string {
  if (control.id === "performance" && (choice.id === "standard" || choice.id === "fast")) {
    return i18n.t(choice.id === "standard" ? "settings:modelSelection.standard" : "settings:modelSelection.fast");
  }
  return choice.label;
}
export function runChoiceDescription(control: ModelRunControl, choice: ModelRunControlChoice): string {
  if (control.id === "performance" && (choice.id === "standard" || choice.id === "fast")) {
    return i18n.t(choice.id === "standard" ? "settings:modelSelection.standardDescription" : "settings:modelSelection.fastDescription");
  }
  return choice.description;
}

export function modelRunSelection(resolved: ConfigSnapshot["modelSelection"]): ModelRunSelection {
  if (!resolved?.model) return emptyModelRunSelection();
  return {
    model_profile_id: resolved.model.id,
    thinking_level: resolved.thinking_level ?? "",
    run_options: { ...defaultRunOptions(resolved), ...resolved.run_options },
  };
}

const emptyModelRunSelection = (): ModelRunSelection => ({
  model_profile_id: "",
  thinking_level: "",
  run_options: {},
});

export const defaultThinkingLevel = (levels: readonly string[]) =>
  levels.includes("medium") ? "medium" : (levels[0] ?? "");

export const defaultRunOptions = (model: Pick<ModelCatalog["models"][number], "run_controls">) =>
  Object.fromEntries(
    model.run_controls.map((control) => [control.id, control.default_choice]),
  );

export const visibleCatalogModels = (catalog: ModelCatalog | null) => {
  if (!catalog) return [];
  const hidden = new Set(catalog.hidden_model_profile_ids);
  return catalog.models.filter((model) => !hidden.has(model.id));
};

/** A model Chat can answer with now: in the catalog, shown in Chat and on a connected account. */
export const usableCatalogModel = (catalog: ModelCatalog | null, modelProfileId: string) =>
  visibleCatalogModels(catalog).find((model) => model.id === modelProfileId && model.credential_source !== "missing");

export const resolveExplicitSelection = (
  catalog: ModelCatalog | null,
  selection: ModelRunSelection,
): ModelRunSelection => {
  const model = usableCatalogModel(catalog, selection.model_profile_id);
  if (!model) return emptyModelRunSelection();
  const options = selection.run_options;
  const supported = model.run_controls.length === Object.keys(options).length
    && model.run_controls.every(control => control.choices.some(choice => choice.id === options[control.id]));
  return {
    model_profile_id: model.id,
    thinking_level: model.thinking_levels.includes(selection.thinking_level) ? selection.thinking_level : "",
    run_options: supported ? options : defaultRunOptions(model),
  };
};

/** Two choices answer with the same model, reasoning level and run options. */
export const sameRunSelection = (a: ModelRunSelection, b: ModelRunSelection) =>
  a.model_profile_id === b.model_profile_id && a.thinking_level === b.thinking_level &&
  Object.keys(a.run_options).length === Object.keys(b.run_options).length &&
  Object.entries(a.run_options).every(([key, value]) => b.run_options[key] === value);

