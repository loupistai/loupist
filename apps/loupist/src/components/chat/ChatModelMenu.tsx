import i18n, { useTranslation } from "@/i18n";
import { memo } from "react";
import { PromptInputButton } from "../ai-elements/prompt-input";
import { ModelSelectionMenu, type ModelSelectionMenuProps } from "../models/ModelSelectionMenu";
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
 * "Auto" when nothing is chosen, the chosen model's name as a chip otherwise,
 * and "Connect an AI account" when no model can run.
 */
export const ChatModelMenu = memo(function ChatModelMenu({ auto, onResetToAuto, onOpenUsage, ...props }: Props) {
  useTranslation("chat");
  // Only a missing account asks to connect; hidden models still open the menu, which explains them.
  const connected = props.providers.some((provider) => provider.credential_source !== "missing")
    || props.models.some((model) => model.credential_source !== "missing");
  if (!connected && props.onManageModels) {
    return <PromptInputButton size="sm" className="composer-model-menu" disabled={props.disabled} onClick={props.onManageModels}>
      <span>{t("chatModelMenu.connectAccount")}</span>
    </PromptInputButton>;
  }
  const selected = props.models.find((model) => model.id === props.selection.model_profile_id);
  // The billing line follows the account that answers: the chosen model's provider, or Auto's.
  const billing = props.providers.find((provider) => provider.id === (selected ?? props.models[0])?.provider);
  const planLine = onOpenUsage && planLineKind(billing) ? <ProviderPlanLine provider={billing} onOpenUsage={onOpenUsage} /> : undefined;
  return <ModelSelectionMenu contextLabel={t("chatModelMenu.modelForThisMessage")} {...props} planLine={planLine} auto={{ active: auto, onSelect: onResetToAuto }} renderTrigger={(label) => (
    // The model behind "Auto" is one hover away for people who want it, never on the screen by default.
    <PromptInputButton size="sm" className="composer-model-menu" aria-label={t("chatModelMenu.modelSettingsForThisMessage")} aria-description={label}
      disabled={props.disabled || (!props.models.length && !props.onManageModels)} tooltip={label}>
      {auto || !selected ? <span>{t("chatModelMenu.auto")}</span> : <span className="composer-model-chip">{selected.name}</span>}
      <ChevronDownIcon aria-hidden="true" />
    </PromptInputButton>
  )} />;
});
