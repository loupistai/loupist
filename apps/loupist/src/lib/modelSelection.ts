import type {
  EffectiveConfig,
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

export const resolveConfiguredModelRunSelection = (
  catalog: ModelCatalog | null,
  explicit: ModelRunSelection | undefined,
  defaults: Pick<EffectiveConfig["chat"], "model_profile_id" | "thinking_level" | "run_options"> | undefined,
  lastProfileId?: string,
): ModelRunSelection => {
  if (explicit) return resolveModelRunSelection(catalog, explicit.model_profile_id, explicit.thinking_level, explicit.run_options);
  if (defaults?.model_profile_id && !visibleCatalogModels(catalog).some((model) => model.id === defaults.model_profile_id)) return emptyModelRunSelection();
  const initial = resolveModelRunSelection(catalog, defaults?.model_profile_id ?? lastProfileId, defaults?.thinking_level ?? undefined);
  if (!initial.model_profile_id) return initial;
  const selected = catalog!.models.find((model) => model.id === initial.model_profile_id)!;
  if (defaults?.thinking_level != null && !selected.thinking_levels.includes(defaults.thinking_level)) return emptyModelRunSelection();
  if (Object.entries(defaults?.run_options ?? {}).some(([key, value]) => !selected.run_controls.some((control) => control.id === key && control.choices.some((choice) => choice.id === value)))) return emptyModelRunSelection();
  return { ...initial, run_options: { ...initial.run_options, ...defaults?.run_options } };
};

const emptyModelRunSelection = (): ModelRunSelection => ({
  model_profile_id: "",
  thinking_level: "",
  run_options: {},
});

export const defaultThinkingLevel = (levels: readonly string[]) =>
  levels.includes("medium") ? "medium" : (levels[0] ?? "");

export const defaultRunOptions = (model: ModelCatalog["models"][number]) =>
  Object.fromEntries(
    model.run_controls.map((control) => [control.id, control.default_choice]),
  );

export const visibleCatalogModels = (catalog: ModelCatalog | null) => {
  if (!catalog) return [];
  const hidden = new Set(catalog.hidden_model_profile_ids);
  return catalog.models.filter((model) => !hidden.has(model.id));
};

const resolveModelRunSelection = (
  catalog: ModelCatalog | null,
  preferredProfileId?: string,
  preferredThinkingLevel?: string,
  preferredRunOptions?: Record<string, string>,
): ModelRunSelection => {
  const models = visibleCatalogModels(catalog);
  const preferred = preferredProfileId
    ? models.find((item) => item.id === preferredProfileId)
    : models[0];
  const preferredIsHidden = Boolean(
    preferredProfileId &&
      catalog?.hidden_model_profile_ids.includes(preferredProfileId),
  );
  const model = preferred ?? (preferredIsHidden ? models[0] : undefined);
  if (!model) return emptyModelRunSelection();
  return {
    model_profile_id: model.id,
    thinking_level:
      preferredThinkingLevel === undefined
        ? defaultThinkingLevel(model.thinking_levels)
        : model.thinking_levels.includes(preferredThinkingLevel)
          ? preferredThinkingLevel
          : "",
    run_options:
      preferredRunOptions !== undefined &&
      model.run_controls.length === Object.keys(preferredRunOptions).length &&
      model.run_controls.every((control) =>
        control.choices.some(
          (choice) => choice.id === preferredRunOptions[control.id],
        ),
      )
        ? preferredRunOptions
        : defaultRunOptions(model),
  };
};
