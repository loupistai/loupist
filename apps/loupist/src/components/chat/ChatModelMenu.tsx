import i18n, { useTranslation } from "@/i18n";
import { memo } from "react";
import { PromptInputButton } from "../ai-elements/prompt-input";
import { ModelSelectionMenu, type ModelSelectionMenuProps } from "../models/ModelSelectionMenu";
import { defaultThinkingLevel, thinkingLabel } from "../../lib/modelSelection";
import { ChevronDownIcon } from "lucide-react";
import { planLineKind, ProviderPlanLine } from "../models/ProviderPlanLine";

const t = i18n.getFixedT(null, "chat");

type Props = Omit<ModelSelectionMenuProps, "renderTrigger" | "ariaLabel" | "planLine"> & {
  noAccount?: boolean;
  onOpenUsage?: (provider: string) => void;
};

export const ChatModelMenu = memo(function ChatModelMenu({ noAccount, onOpenUsage, ...props }: Props) {
  useTranslation("chat");
  if (noAccount) {
    return <PromptInputButton size="sm" className="composer-model-menu" data-testid="composer-model-menu" disabled={props.disabled} onClick={props.onManageModels}>
      <span>{t("chatModelMenu.noAccount")}</span>
    </PromptInputButton>;
  }
  const level = thinkingLabel(props.selection.thinking_level);
  const selected = props.models.find((model) => model.id === props.selection.model_profile_id);
  const name = props.modelName ?? selected?.name ?? t("chatPendingQueue.unavailableModel");
  const billing = props.providers.find((provider) => provider.id === selected?.provider);
  const planLine = onOpenUsage && planLineKind(billing) ? <ProviderPlanLine provider={billing} onOpenUsage={onOpenUsage} /> : undefined;
  // Picking a model never changes the level silently: the level in use stays when the new
  // model lists it; otherwise that model's default applies.
  const keepLevel = (model: { thinking_levels: string[] }) =>
    props.selection.thinking_level && model.thinking_levels.includes(props.selection.thinking_level) ? props.selection.thinking_level : defaultThinkingLevel(model.thinking_levels);
  return <ModelSelectionMenu levelOnModelPick={keepLevel} contextLabel={t("chatModelMenu.modelForThisMessage")} searchFrom={8} {...props} planLine={planLine} renderTrigger={(label) => (
    <PromptInputButton size="sm" className="composer-model-menu" data-testid="composer-model-menu" aria-label={`${name} · ${t("chatModelMenu.modelSettingsForThisMessage")}${level ? ` · ${level}` : ""}`} aria-description={label}
      disabled={props.disabled || (!props.models.length && !props.onManageModels)} tooltip={label}>
      <span className="composer-model-chip">{name}</span>
      {level && <span className="composer-thinking-level">{level}</span>}
      <ChevronDownIcon aria-hidden="true" />
    </PromptInputButton>
  )} />;
});
