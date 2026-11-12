import i18n, { useTranslation, Trans } from "../../i18n";
import { useEffect, useId, useMemo, useRef, useState, type ReactElement, type ReactNode } from "react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type {
  ConfiguredModel,
  ModelProviderOption,
  ModelRunSelection,
} from "../../types";
import { defaultRunOptions, defaultThinkingLevel, thinkingLabel, runControlLabel, runChoiceLabel, runChoiceDescription } from "../../lib/modelSelection";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { ChevronDownIcon, Settings2Icon } from "lucide-react";

export type ModelSelectionMenuProps = {
  models: ConfiguredModel[];
  /** Show the search field only from this many models (the composer passes 8); every model by default. */
  searchFrom?: number;
  providers: ModelProviderOption[];
  selection: ModelRunSelection;
  disabled?: boolean;
  ariaLabel?: string;
  renderTrigger?: (label: string) => ReactElement;
  contextLabel?: string;
  onManageModels?: () => void;
  onSelectionChange: (selection: ModelRunSelection) => void;
  openSignal?: number;
  /** The composer's "Auto" row: the app's default run profile, used when nothing is chosen for this Loupist. */
  auto?: { active: boolean; onSelect: () => void };
  /** A billing line under the model list ("Using your ChatGPT plan · Manage usage"). */
  planLine?: ReactNode;
};

type ProviderModels = {
  id: string;
  label: string;
  models: ConfiguredModel[];
};

const groupCatalog = (
  models: ConfiguredModel[],
  providers: ModelProviderOption[],
): ProviderModels[] => {
  const modelsByProvider = new Map<string, ConfiguredModel[]>();
  for (const model of models) {
    const group = modelsByProvider.get(model.provider) ?? [];
    group.push(model);
    modelsByProvider.set(model.provider, group);
  }

  const groups = providers.flatMap((provider) => {
    const providerModels = modelsByProvider.get(provider.id);
    if (!providerModels?.length) return [];
    modelsByProvider.delete(provider.id);
    return [{ id: provider.id, label: provider.label, models: providerModels }];
  });

  for (const [provider, providerModels] of modelsByProvider) {
    groups.push({ id: provider, label: provider, models: providerModels });
  }
  return groups;
};

const keepMenuOpen = (event: Event) => {
  event.preventDefault();
};

export function ModelSelectionMenu({
  models,
  searchFrom = 0,
  providers,
  selection,
  disabled = false,
  ariaLabel = i18n.t("settings:modelSelectionMenu.modelSettings"),
  renderTrigger,
  contextLabel = i18n.t("settings:modelSelectionMenu.modelSettings"),
  onManageModels,
  onSelectionChange,
  openSignal = 0,
  auto,
  planLine,
}: ModelSelectionMenuProps) {
  useTranslation(["settings", "chat"]);
  const [open, setOpen] = useState(false);
  // Open on a new request only; a remounted menu does not replay an old one.
  const seenSignal = useRef(openSignal);
  useEffect(() => {
    if (openSignal > 0 && openSignal !== seenSignal.current) setOpen(true);
    seenSignal.current = openSignal;
  }, [openSignal]);
  const [query, setQuery] = useState("");
  const [advanced, setAdvanced] = useState(false);
  const advancedId = useId();
  const modelRefs = useRef(new Map<string, HTMLDivElement>());
  const selectedModel = useMemo(
    () => models.find((model) => model.id === selection.model_profile_id),
    [models, selection.model_profile_id],
  );
  const catalog = useMemo(
    () => groupCatalog(models, providers).map((provider) => ({
      ...provider,
      models: provider.models.filter((model) => `${provider.label} ${model.name} ${model.model}`.toLowerCase().includes(query.toLowerCase().trim())),
    })).filter((provider) => provider.models.length),
    [models, providers, query],
  );
  const selectedOptionLabels = selectedModel?.run_controls.flatMap((control) => {
    const choice = control.choices.find(
      (candidate) => candidate.id === selection.run_options[control.id],
    );
    return choice && choice.id !== control.default_choice ? [runChoiceLabel(control, choice)] : [];
  }) ?? [];
  const triggerLabel = selectedModel
    ? i18n.t(selectedOptionLabels.length ? "settings:modelSelectionMenu.selectionWithOptions" : "settings:modelSelectionMenu.selection", {
        model: selectedModel.name,
        thinking: thinkingLabel(selection.thinking_level) || i18n.t("settings:modelSelectionMenu.chooseThinking"),
        options: selectedOptionLabels.join(" · "),
      })
    : i18n.t("settings:modelSelectionMenu.chooseModel");

  const selectionFor = (model: ConfiguredModel): ModelRunSelection =>
    model.id === selection.model_profile_id
      ? selection
      : {
          model_profile_id: model.id,
          thinking_level: defaultThinkingLevel(model.thinking_levels),
          run_options: defaultRunOptions(model),
        };

  return (
    <DropdownMenu modal open={open} onOpenChange={(next) => { setOpen(next); if (!next) { setQuery(""); setAdvanced(false); } }}>
      <DropdownMenuTrigger asChild aria-haspopup="dialog">
        {renderTrigger ? renderTrigger(triggerLabel) : (
          <Button type="button" variant="outline" aria-label={ariaLabel ? `${triggerLabel} · ${ariaLabel}` : undefined} aria-description={triggerLabel} disabled={disabled || (!models.length && !onManageModels)} className="world-model-menu">
            <span>{triggerLabel}</span>
            <ChevronDownIcon aria-hidden="true" />
          </Button>
        )}
      </DropdownMenuTrigger>
      {open ? (
        <DropdownMenuContent
          role="dialog"
          aria-modal="true"
          aria-orientation={undefined}
          aria-labelledby={undefined}
          className="composer-model-menu-panel"
          side="top"
          align="start"
          sideOffset={8}
          aria-label={i18n.t("settings:modelSelectionMenu.availableModels")}
          onCloseAutoFocus={() => setQuery("")}
          onKeyDown={event => {
            // This searchable picker is a dialog. Radix menus otherwise consume
            // Tab, leaving keyboard users unable to reach its other controls.
            if (event.key !== "Tab") return;
            const controls = [...event.currentTarget.querySelectorAll<HTMLElement>('input:not([disabled]), button:not([disabled]), [role^="menuitem"]:not([aria-disabled="true"])')]
              .filter(element => !element.closest('[hidden], [inert]') && (document.documentElement.clientWidth === 0 || element.getClientRects().length > 0));
            if (!controls.length) return;
            event.preventDefault();
            const current = controls.indexOf(document.activeElement as HTMLElement);
            controls[(current + (event.shiftKey ? -1 : 1) + controls.length) % controls.length]?.focus();
          }}
        >
          <div className="model-menu-header">
            <strong>{contextLabel}</strong>
            {/* A short list fits at a glance: the composer hides search under eight models (chat design 35). */}
            {models.length >= searchFrom ? <Input autoFocus type="search" aria-label={i18n.t("settings:modelSelectionMenu.searchModels")} placeholder={i18n.t("settings:modelSelectionMenu.searchProvidersOrModels")} value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "ArrowDown" || event.key === "ArrowUp") {
                  event.preventDefault();
                  const model = event.key === "ArrowDown"
                    ? catalog[0]?.models[0]
                    : catalog.at(-1)?.models.at(-1);
                  if (model) modelRefs.current.get(model.id)?.focus();
                }
                if (event.key === "Enter") event.preventDefault();
                if (event.key !== "Escape" && event.key !== "Tab") event.stopPropagation();
              }} /> : null}
          </div>
          <div className="model-menu-catalog" role="menu" aria-label={i18n.t("settings:modelSelectionMenu.availableModels")}>
          {auto && models.length ? <>
            <DropdownMenuRadioGroup value={auto.active ? "auto" : ""} onValueChange={() => auto.onSelect()}>
              <DropdownMenuRadioItem value="auto">
                {i18n.t(auto.active ? "chat:chatModelMenu.autoRecommended" : "chat:chatModelMenu.resetToAuto")}
              </DropdownMenuRadioItem>
            </DropdownMenuRadioGroup>
            <DropdownMenuSeparator />
            <DropdownMenuLabel>{i18n.t("chat:chatModelMenu.chooseModel")}</DropdownMenuLabel>
          </> : null}
          {catalog.length === 0 ? (
            <DropdownMenuItem disabled>{models.length ? i18n.t("settings:modelSelectionMenu.noMatchingModels") : i18n.t("settings:modelSelectionMenu.noConnectedModels")}</DropdownMenuItem>
          ) : (
            catalog.map((provider, index) => (
              <DropdownMenuRadioGroup
                key={provider.id}
                value={auto?.active ? "" : selection.model_profile_id}
                onValueChange={(profileId) => {
                  const model = provider.models.find((item) => item.id === profileId);
                  if (model) onSelectionChange(selectionFor(model));
                }}
              >
                {index > 0 ? <DropdownMenuSeparator /> : null}
                <DropdownMenuLabel>{provider.label}</DropdownMenuLabel>
                {provider.models.map((model) => (
                  <DropdownMenuRadioItem
                    key={model.id}
                    ref={(element) => {
                      if (element) modelRefs.current.set(model.id, element);
                      else modelRefs.current.delete(model.id);
                    }}
                    value={model.id}
                    onSelect={keepMenuOpen}
                  >
                    <span className="model-menu-name">{model.name}</span>
                    {model.credential_source === "missing" ? <small>{i18n.t("settings:modelSelectionMenu.notConnected")}</small> : null}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
            ))
          )}
          </div>
          {planLine ? <div className="model-menu-plan">{planLine}</div> : null}
          {selectedModel ? (
            <div className="model-menu-options" role="menu" aria-label={i18n.t("chat:chatModelMenu.advanced")}>
              <DropdownMenuSeparator />
              <DropdownMenuItem onSelect={(event) => { event.preventDefault(); setAdvanced(!advanced); }} aria-expanded={advanced} aria-controls={advanced ? advancedId : undefined} className="model-menu-advanced" data-testid="model-menu-advanced">
                {i18n.t("chat:chatModelMenu.advanced")}<ChevronDownIcon aria-hidden="true" />
              </DropdownMenuItem>
              {advanced ? <div id={advancedId}>
                <DropdownMenuLabel><Trans ns="settings" i18nKey="modelSelectionMenu.thinkingForModel" values={{ model: selectedModel.name }} components={[<span className="model-menu-current" />]} /></DropdownMenuLabel>
                <DropdownMenuRadioGroup
                  className="model-menu-choices" data-testid="model-menu-choices"
                  value={selection.thinking_level}
                  onValueChange={(thinking_level) =>
                    onSelectionChange({
                      ...selection,
                      thinking_level,
                    })
                  }
                >
                  {selectedModel.thinking_levels.map((level) => (
                    <DropdownMenuRadioItem
                      key={level}
                      value={level}
                      onSelect={keepMenuOpen}
                    >
                      {thinkingLabel(level)}
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
                {selectedModel.run_controls.map((control) => (
                  <DropdownMenuRadioGroup
                    className="model-menu-run-control"
                    key={control.id}
                    value={selection.run_options[control.id]}
                    onValueChange={(choice) =>
                      onSelectionChange({
                        ...selection,
                        run_options: {
                          ...selection.run_options,
                          [control.id]: choice,
                        },
                      })
                    }
                  >
                    <DropdownMenuLabel>{runControlLabel(control)}</DropdownMenuLabel>
                    {control.choices.map((choice) => (
                      <DropdownMenuRadioItem
                        key={choice.id}
                        value={choice.id}
                        title={runChoiceDescription(control, choice)}
                        onSelect={keepMenuOpen}
                      >
                        {runChoiceLabel(control, choice)}
                      </DropdownMenuRadioItem>
                    ))}
                  </DropdownMenuRadioGroup>
                ))}
              </div> : null}
            </div>
          ) : null}
          {onManageModels ? <div className="model-menu-footer" role="menu" aria-label={i18n.t("settings:modelSelectionMenu.manageModelsProviders")}><DropdownMenuItem onSelect={() => { setOpen(false); onManageModels(); }}><Settings2Icon />{i18n.t("settings:modelSelectionMenu.manageModelsProviders")}</DropdownMenuItem></div> : null}
        </DropdownMenuContent>
      ) : null}
    </DropdownMenu>
  );
}
