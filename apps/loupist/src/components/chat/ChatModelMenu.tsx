import i18n, { useTranslation } from "@/i18n";
import { memo } from "react";
import { PromptInputButton } from "../ai-elements/prompt-input";
import { ModelSelectionMenu, type ModelSelectionMenuProps } from "../models/ModelSelectionMenu";
import { defaultThinkingLevel, thinkingLabel } from "../../lib/modelSelection";
import { ChevronDownIcon } from "lucide-react";
import { planLineKind, ProviderPlanLine } from "../models/ProviderPlanLine";

const t = i18n.getFixedT(null, "chat");

type Props = Omit<ModelSelectionMenuProps, "renderTrigger" | "ariaLabel" | "auto" | "planLine"> & {
  /** True when nothing is chosen for this Loupist: the app's default run profile applies. */
  auto: boolean;
  onResetToAuto: () => void;
  /** Rust opens the provider's usage page from the plan line. */
  onOpenUsage?: (provider: string) => void;
};

/**
 * The composer's run-profile control. It never shows a model id by default:
 * "Auto" when nothing is chosen and the chosen model's name otherwise, both as a
 * quiet label at the composer's trailing edge, and "Connect an AI account" (the
 * filled send slot) when no model can run.
 */
export const ChatModelMenu = memo(function ChatModelMenu({ auto, onResetToAuto, onOpenUsage, ...props }: Props) {
  useTranslation("chat");
  // Only a missing account asks to connect; hidden models still open the menu, which explains them.
  const connected = props.providers.some((provider) => provider.credential_source !== "missing")
    || props.models.some((model) => model.credential_source !== "missing");
  if (!connected && props.onManageModels) {
    return <PromptInputButton size="sm" className="composer-model-menu" data-testid="composer-model-menu" disabled={props.disabled} onClick={props.onManageModels}>
      <span>{t("chatModelMenu.connectAccount")}</span>
    </PromptInputButton>;
  }
  const level = thinkingLabel(props.selection.thinking_level);
  const selected = props.models.find((model) => model.id === props.selection.model_profile_id);
  // Auto is chosen by Rust at admission. The last-used model is not a forecast
  // of that choice; show a plan only when its account is unambiguous.
  const connectedProviders = new Set(props.models.filter((model) => model.credential_source !== "missing").map((model) => model.provider));
  const billingProvider = auto ? connectedProviders.size === 1 ? [...connectedProviders][0] : undefined : selected?.provider;
  const billing = props.providers.find((provider) => provider.id === billingProvider);
  const planLine = onOpenUsage && planLineKind(billing) ? <ProviderPlanLine provider={billing} onOpenUsage={onOpenUsage} /> : undefined;
  // Picking a model never changes the level silently: the level in use stays when the new
  // model lists it; otherwise that model's default applies.
  const keepLevel = (model: { thinking_levels: string[] }) =>
    props.selection.thinking_level && model.thinking_levels.includes(props.selection.thinking_level) ? props.selection.thinking_level : defaultThinkingLevel(model.thinking_levels);
  return <ModelSelectionMenu levelOnModelPick={keepLevel} contextLabel={t("chatModelMenu.modelForThisMessage")} searchFrom={8} {...props} planLine={planLine} auto={{ active: auto, onSelect: onResetToAuto }} renderTrigger={(label) => (
    <PromptInputButton size="sm" className="composer-model-menu" data-testid="composer-model-menu" aria-label={`${auto || !selected ? t("chatModelMenu.auto") : selected.name} · ${t("chatModelMenu.modelSettingsForThisMessage")}${level ? ` · ${level}` : ""}`} aria-description={auto ? t("chatModelMenu.autoDescription") : label}
      disabled={props.disabled || (!props.models.length && !props.onManageModels)} tooltip={auto ? t("chatModelMenu.autoDescription") : label}>
      {auto || !selected ? <span>{t("chatModelMenu.auto")}</span> : <span className="composer-model-chip">{selected.name}</span>}
      {level && <span className="composer-thinking-level">{level}</span>}
      <ChevronDownIcon aria-hidden="true" />
    </PromptInputButton>
  )} />;
});
