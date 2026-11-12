import { ChevronDownIcon, ExternalLinkIcon, LoaderCircleIcon, MoreHorizontalIcon, RefreshCwIcon, SearchIcon } from "lucide-react";
import { useState } from "react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { formatDate, formatDateTime, formatNumber } from "../../i18n/format";
import { useTranslation } from "../../i18n";
import { providerShortName } from "../../chat/providerNames";
import type { ConfiguredModel, ModelProviderOption, ProviderStatusView } from "../../types";
import type { ConnectSheetTarget } from "./ConnectSheet";
import { ProviderStateLabel } from "./ProviderWays";

/** "Last reached its limit" stays this long after the notice clears: a dated note, not a meter. */
export const LAST_LIMIT_WINDOW_MS = 7 * 24 * 60 * 60 * 1000;

/** The name a card goes by: the sign-in product for a plan, the company for a key. */
export const providerCardTitle = (option: ModelProviderOption) =>
  option.custom ? option.custom.name : providerShortName(option.id, option.status.credential_kind) ?? option.label;

const hostOf = (address: string) => {
  try { return new URL(address).host; } catch { return address; }
};

/** How a card reconnects: a plan signs in again, a key opens its key sheet, an endpoint opens its form. */
export function reconnectTarget(option: ModelProviderOption): ConnectSheetTarget {
  if (option.custom) return { kind: "custom", edit: option.custom };
  const kind = option.status.credential_kind;
  const signIn = kind === "plan" || ((kind === "none" || kind === "environment") && Boolean(option.sign_in_flow));
  return { kind: "provider", provider: option.id, method: signIn && option.oauth ? "sign_in" : "api_key" };
}

/** True when a Disconnected card still matters: something keeps pointing at the account. */
export const stillUsed = (status: ProviderStatusView) =>
  status.used_by.schedules > 0 || status.used_by.chat_default || status.used_by.world_models > 0;

type ProviderCardProps = {
  option: ModelProviderOption;
  models: ConfiguredModel[];
  hiddenModels: ReadonlySet<string>;
  /** The Chat default when it belongs to this provider, null for Auto, undefined when another provider's model is the default. */
  chatModel: ConfiguredModel | null | undefined;
  loading: boolean;
  removing: boolean;
  loadError: string | null | undefined;
  /** A provider mutation or visibility update is running somewhere in the panel. */
  locked: boolean;
  reloadDisabled: boolean;
  visibilityBusyId: string | null;
  now?: number;
  onReload: () => void;
  onConnect: (target: ConnectSheetTarget) => void;
  onOpenUsage: (provider: string) => void;
  onRemove: () => void;
  onSetVisibility: (modelProfileId: string, visible: boolean) => void;
  onSetAllVisible: (visible: boolean) => void;
};

/** One provider (spec providers §2.3): state, credential, billing, one notice, then its models. */
export function ProviderCard({ option, models, hiddenModels, chatModel, loading, removing, loadError, locked, reloadDisabled, visibilityBusyId, now = Date.now(), onReload, onConnect, onOpenUsage, onRemove, onSetVisibility, onSetAllVisible }: ProviderCardProps) {
  const { t } = useTranslation("settings");
  const [expanded, setExpanded] = useState(false);
  const [query, setQuery] = useState("");
  const status = option.status;
  const title = providerCardTitle(option);
  const plan = status.credential_kind === "plan";
  const needle = query.trim().toLowerCase();
  const filtered = needle ? models.filter((model) => `${model.name} ${model.model}`.toLowerCase().includes(needle)) : models;

  if (status.state === "disconnected") {
    return <li><article className="provider-card" data-provider-card={option.id} data-state="disconnected" aria-label={title}>
      <header className="provider-card-heading"><h3>{title}</h3><ProviderStateLabel status={status} /></header>
      {status.used_by.schedules > 0 ? <p className="provider-card-meta">{t("providers.usedBy", { count: status.used_by.schedules, formattedCount: formatNumber(status.used_by.schedules) })}</p> : null}
      <div className="provider-card-footer"><span className="provider-card-actions">
        <Button type="button" size="sm" disabled={locked} onClick={() => onConnect(reconnectTarget(option))}>{t("providers.connect")}</Button>
        <Button type="button" size="sm" variant="ghost" disabled={locked} onClick={onRemove}>{removing ? <LoaderCircleIcon className="animate-spin" /> : null}{t("providers.remove")}</Button>
      </span></div>
    </article></li>;
  }

  const credential = option.custom
    ? t(status.credential_kind === "local" ? "providers.viaLocal" : "providers.viaCustom", { address: hostOf(option.custom.base_url) })
    : status.credential_kind === "environment" ? t("providers.viaEnvironment")
      : plan ? (status.connected_at_ms !== null ? t("providers.viaSignIn", { provider: title, date: formatDate(status.connected_at_ms) }) : title)
        : t("providers.viaKey");
  const notice = status.notice;
  const lastLimit = !notice && status.last_limit_at_ms !== null && now - status.last_limit_at_ms < LAST_LIMIT_WINDOW_MS ? status.last_limit_at_ms : null;
  const reconnect = status.state === "needs_reconnect";
  const removeText = t(option.custom ? "providers.remove" : "providers.disconnect");
  const removeLabel = t(option.custom ? "modelSettingsPanel.customRemoveLabel" : "modelSettingsPanel.disconnectLabel", { label: title });
  // Only the account that owns the Chat default names it; Auto is said once, in Chat defaults.
  const chatUses = chatModel ? t("providers.chatDefault", { model: chatModel.name }) : null;
  const canReconnect = Boolean(option.custom || option.api_key || option.oauth);
  const reconnectText = option.api_key && !option.oauth && !option.custom ? t("providers.changeKey") : t("providers.reconnectEllipsis");
  return (
    <li>
      <article className="provider-card" data-provider-card={option.id} data-state={status.state} aria-label={title}>
        <header className="provider-card-heading"><h3>{title}</h3><ProviderStateLabel status={status} /></header>
        <p className="provider-card-meta">{credential}</p>
        {reconnect ? <p className="provider-card-body">{t("providers.reconnectBody")}</p> : null}
        {plan && option.billing_note === "chatgpt_plan" ? <p className="provider-card-body" data-plan-line="chatgpt">
          {t("providers.usingPlan")}<span aria-hidden="true"> · </span>
          <button type="button" className="connect-link" onClick={() => onOpenUsage(option.id)}>{t("providers.manageUsage")}<ExternalLinkIcon aria-hidden="true" /></button>
        </p> : null}
        {notice ? <div className="provider-card-row" data-tone="warning" role="status" data-notice={notice.kind}>
          <p>{notice.kind === "plan_limit" ? t("providers.noticePlanLimit", { dateTime: formatDateTime(notice.at_ms) })
            : notice.kind === "plan_not_eligible" ? t("providers.noticeNotEligible") : t("providers.noticeNoCredit")}</p>
          {notice.kind === "plan_limit" ? <Button type="button" size="sm" variant="ghost" onClick={() => onOpenUsage(option.id)}>{t("providers.manageUsage")}<ExternalLinkIcon aria-hidden="true" /></Button>
            : notice.kind === "plan_not_eligible" ? <Button type="button" size="sm" variant="ghost" disabled={locked} onClick={() => onConnect({ kind: "provider", provider: "openai", method: "api_key" })}>{t("providers.useApiKey")}</Button> : null}
        </div> : lastLimit !== null ? <p className="provider-card-meta">{t("providers.lastLimit", { date: formatDate(lastLimit) })}</p> : null}
        {loadError ? <Alert variant="destructive"><AlertTitle>{t("modelSettingsPanel.loadModelsErrorTitle", { provider: title })}</AlertTitle><AlertDescription>{loadError}
          {!expanded ? <Button type="button" size="sm" variant="ghost" aria-label={t("modelSettingsPanel.reloadModelsForLabel", { label: title })} disabled={reloadDisabled} onClick={onReload}>
            {loading ? <LoaderCircleIcon className="animate-spin" /> : <RefreshCwIcon />}{t("modelSettingsPanel.reloadModels")}
          </Button> : null}</AlertDescription></Alert> : null}
        <div className="provider-card-footer">
          {chatUses ? <span className="provider-card-meta">{chatUses}</span> : <span />}
          <span className="provider-card-actions">
            <Button type="button" size="sm" variant="ghost" aria-expanded={expanded} aria-controls={`provider-models-${option.id}`} onClick={() => setExpanded((value) => !value)}>
              {t("providers.models", { formattedCount: formatNumber(models.length) })}<ChevronDownIcon className={expanded ? "rotate-180" : ""} aria-hidden="true" />
            </Button>
            {/* Only an account that needs it shows a filled Reconnect; otherwise Reconnect or Change key and Disconnect sit in More. */}
            {reconnect && canReconnect ? <Button type="button" size="sm" aria-label={t("modelSettingsPanel.reconnectLabel", { label: title })} disabled={locked} onClick={() => onConnect(reconnectTarget(option))}>{t("providers.reconnect")}</Button> : null}
            <DropdownMenu>
              <DropdownMenuTrigger asChild><Button type="button" size="icon-sm" variant="ghost" aria-label={t("providers.moreActions", { label: title })} disabled={locked}>{removing ? <LoaderCircleIcon className="animate-spin" /> : <MoreHorizontalIcon />}</Button></DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                {canReconnect && !reconnect ? <DropdownMenuItem onSelect={() => onConnect(reconnectTarget(option))}>{reconnectText}</DropdownMenuItem> : null}
                <DropdownMenuItem aria-label={removeLabel} disabled={option.credential_source !== "saved"} title={option.credential_source === "environment" ? t("modelSettingsPanel.thisProviderIsConnectedFromTheEnvironment") : undefined} onSelect={onRemove}>{removeText}</DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </span>
        </div>
        {expanded ? (
          <div id={`provider-models-${option.id}`} className="provider-card-models">
            <div className="settings-model-toolbar">
              <div className="relative min-w-0 flex-1">
                <SearchIcon className="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden="true" />
                <Input aria-label={t("modelSettingsPanel.searchLabelModels", { label: title })} className="pl-9" value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t("modelSettingsPanel.searchModels")} />
              </div>
              <Button type="button" size="sm" variant="ghost" disabled={locked || !models.length} onClick={() => onSetAllVisible(true)}>{t("modelSettingsPanel.showAll")}</Button>
              <Button type="button" size="sm" variant="ghost" disabled={locked || !models.length} onClick={() => onSetAllVisible(false)}>{t("modelSettingsPanel.hideAll")}</Button>
              <Button type="button" size="sm" variant="ghost" aria-label={t("modelSettingsPanel.reloadModelsForLabel", { label: title })} disabled={reloadDisabled} onClick={onReload}>
                {loading ? <LoaderCircleIcon className="animate-spin" /> : <RefreshCwIcon />}{t("modelSettingsPanel.reloadModels")}
              </Button>
            </div>
            <div className="settings-model-list" aria-label={t("modelSettingsPanel.labelModels", { label: title })}>
              {filtered.map((model, index) => (
                <div key={model.id} className={`settings-model-row ${index ? "has-divider" : ""}`} title={model.model}>
                  <span className="min-w-0 flex-1">
                    <strong className="block truncate text-sm">{model.name}</strong>
                    <span className="provider-card-meta">{t("providers.context", { tokens: formatNumber(model.context_window) })}</span>
                  </span>
                  {visibilityBusyId === model.id ? <LoaderCircleIcon className="size-4 animate-spin text-muted-foreground" /> : null}
                  <label className="provider-card-switch">
                    <span>{t("providers.inMenu")}</span>
                    <Switch aria-label={t("modelSettingsPanel.showNameInChat", { name: model.name })} checked={!hiddenModels.has(model.id)} disabled={locked} onCheckedChange={(checked) => onSetVisibility(model.id, checked)} />
                  </label>
                </div>
              ))}
              {!filtered.length ? <p className="p-6 text-center text-sm text-muted-foreground">{models.length ? t("modelSettingsPanel.noMatchingModels") : loading ? t("modelSettingsPanel.loadingModels") : t("modelSettingsPanel.modelsNotLoadedYet")}</p> : null}
            </div>
          </div>
        ) : null}
      </article>
    </li>
  );
}
