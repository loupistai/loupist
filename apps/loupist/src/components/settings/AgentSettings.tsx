import { RunDiagnostics } from "./RunDiagnostics";
import { formatNumber } from "../../i18n/format";
import i18n, { useTranslation } from "../../i18n";
import { loupistOptionLabel } from "../../lib/loupistFolder";
import { Disclosure } from "../ui/disclosure";
import { useCallback, useEffect, useState } from "react";
import { CheckIcon, FolderOpenIcon, RefreshCwIcon } from "lucide-react";
import type { ConfigDocument, ConfigScope, ConfigSnapshot, LoupistSummary, LoupistBridge, ModelCatalog } from "../../types";
import { notifyConfigurationChanged, useConfiguration } from "../../app/useConfiguration";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { NativeSelect } from "../ui/native-select";
import { Switch } from "../ui/switch";
import { thinkingLabel, runControlLabel, runChoiceLabel, visibleCatalogModels } from "../../lib/modelSelection";
import { ProviderPlanLine } from "../models/ProviderPlanLine";

function leaf(document: unknown, path: string): unknown {
  return path.split(".").reduce<unknown>((value, key) => value && typeof value === "object" ? (value as Record<string, unknown>)[key] : undefined, document);
}
function patch(path: string, value: unknown): ConfigDocument {
  return path.split(".").reverse().reduce<unknown>((result, key) => ({ [key]: result }), value) as ConfigDocument;
}
type FieldProps = { snapshot: ConfigSnapshot; path: string; relatedPaths?: string[]; label: string; description?: string; disabled: boolean; onReset: (path: string) => void };

function SettingField({ snapshot, path, relatedPaths = [], label, description, disabled, onReset, children }: FieldProps & { children: React.ReactNode }) {
  useTranslation("settings");
  const paths = [path, ...relatedPaths];
  const explicit = paths.some((key) => leaf(snapshot.explicit, key) !== undefined);
  const sources = paths.map((key) => snapshot.sources[key] ?? "default");
  const source = sources.includes("project") ? "project" : sources.includes("global") ? "global" : "default";
  return <div className="agent-config-field">
    <div className="agent-config-label"><label htmlFor={`config-${path}`}>{label}</label>{description ? <p className="agent-config-description" id={`config-description-${path}`}>{description}</p> : null}{source === "default" ? null : <span className="agent-config-source">{source === "global" ? snapshot.scope.kind === "project" ? i18n.t("settings:agentSettings.usingGlobalDefaults") : i18n.t("settings:agentSettings.yourGlobalDefault") : i18n.t("settings:agentSettings.customizedForThisLoupist")}</span>}</div>
    <div className="agent-config-control">{children}{explicit ? <Button type="button" variant="ghost" size="sm" disabled={disabled} onClick={() => onReset(path)} aria-label={i18n.t("settings:agentSettings.resetLabel", { label: label })} title={snapshot.scope.kind === "project" ? i18n.t("settings:agentSettings.useGlobalDefault") : i18n.t("settings:agentSettings.useAppDefault")}>{snapshot.scope.kind === "project" ? i18n.t("settings:agentSettings.useDefault") : i18n.t("settings:agentSettings.reset")}</Button> : null}</div>
  </div>;
}

function NumberSetting(props: FieldProps & { max: number; onDirtyChange: (path: string, dirty: boolean) => void; onSave: (path: string, value: number) => Promise<boolean> }) {
  useTranslation("settings");
  const effective = Number(leaf(props.snapshot.effective, props.path));
  const [value, setValue] = useState(String(effective));
  useEffect(() => { setValue(String(effective)); }, [effective, props.snapshot.path]);
  const number = Number(value);
  const valid = value.trim() !== "" && Number.isSafeInteger(number) && number >= 0 && number <= props.max;
  const dirty = !valid || number !== effective;
  const { onDirtyChange, path } = props;
  useEffect(() => { onDirtyChange(path, dirty); }, [onDirtyChange, path, dirty]);
  useEffect(() => () => onDirtyChange(path, false), [onDirtyChange, path]);
  return <SettingField {...props}><div className="agent-config-number"><Input id={`config-${props.path}`} type="number" aria-describedby={props.description ? `config-description-${props.path}` : undefined} min={0} max={props.max} step={1} disabled={props.disabled} value={value} onChange={(event) => setValue(event.target.value)} aria-invalid={!valid} /><Button size="sm" variant="outline" disabled={props.disabled || !valid || number === effective} onClick={() => void props.onSave(props.path, number)} aria-label={i18n.t("settings:agentSettings.saveLabel", { label: props.label })}>{i18n.t("settings:agentSettings.save")}</Button></div></SettingField>;
}

export function ConfigurationDiagnostics({ snapshot }: { snapshot: ConfigSnapshot | null }) {
  useTranslation("settings");
  if (!snapshot?.diagnostics.length) return null;
  return <div className="inline-error config-diagnostics" role="alert"><strong>{i18n.t("settings:agentSettings.newResponsesStayPausedUntilThisIsFixed")}</strong>{snapshot.diagnostics.map((diagnostic, index) => <p key={index}>{diagnostic.line !== null ? i18n.t("settings:agentSettings.diagnosticLine", { line: formatNumber(diagnostic.line), message: diagnostic.message }) : i18n.t("settings:agentSettings.diagnostic", { message: diagnostic.message })}</p>)}</div>;
}

export function AgentSettings({ bridge, loupists, catalog, scope, onScopeChange }: {
  bridge: LoupistBridge; loupists: LoupistSummary[]; catalog: ModelCatalog | null; scope: ConfigScope; onScopeChange?: (scope: ConfigScope) => void;
}) {
  useTranslation("settings");
  const config = useConfiguration(bridge, scope);
  const snapshot = config.snapshot;
  const disabled = config.busy || Boolean(snapshot?.diagnostics.length);
  const scopeKey = scope.kind === "global" ? "global" : scope.loupistId;
  const [savedScope, setSavedScope] = useState<string | null>(null);
  const [dirtyNumbers, setDirtyNumbers] = useState<ReadonlySet<string>>(() => new Set());
  const onNumberDirtyChange = useCallback((path: string, dirty: boolean) => {
    setDirtyNumbers((current) => {
      if (current.has(path) === dirty) return current;
      const next = new Set(current);
      if (dirty) next.add(path);
      else next.delete(path);
      return next;
    });
    if (dirty) setSavedScope(null);
  }, []);
  useEffect(() => { setSavedScope(null); }, [scopeKey]);
  const update = async (set: ConfigDocument, unset: string[][] = []) => {
    setSavedScope(null);
    const saved = await config.update(set, unset);
    if (saved) setSavedScope(scopeKey);
    return saved;
  };
  const save = (path: string, value: unknown) => update(patch(path, value));
  const reset = (path: string) => { void update({}, [path.split(".")]); };
  const field = (path: string, label: string, description?: string): FieldProps => ({ snapshot: snapshot!, path, label, description, disabled, onReset: reset });
  const models = visibleCatalogModels(catalog);
  const profileId = snapshot?.effective.chat.model_profile_id ?? "";
  const currentModel = models.find((model) => model.id === profileId);
  const fallbackId = scope.kind === "project" ? loupists.find((loupist) => loupist.id === scope.loupistId)?.last_model_profile_id : undefined;
  const optionModel = profileId ? currentModel : models.find((model) => model.id === fallbackId) ?? models[0];
  const unavailableOptions = Object.entries(snapshot?.effective.chat.run_options ?? {}).filter(([key, value]) => !optionModel?.run_controls.some((control) => control.id === key && control.choices.some((choice) => choice.id === value)));
  const thinking = snapshot?.effective.chat.thinking_level;
  const thinkingLevels = currentModel?.thinking_levels ?? [...new Set(models.flatMap((model) => model.thinking_levels))];
  return <>
    {onScopeChange ? <div className="agent-config-scope">
      <div className="agent-config-scope-copy"><label htmlFor="config-scope">{i18n.t("settings:agentSettings.settingsApplyTo")}</label><p>{scope.kind === "global" ? i18n.t("settings:agentSettings.setTheDefaultsEveryLoupistStartsWith") : i18n.t("settings:agentSettings.customizeThisLoupistOtherSettingsFollowYourGlobal")}</p></div>
      <NativeSelect id="config-scope" value={scopeKey} onChange={(event) => onScopeChange(event.target.value === "global" ? { kind: "global" } : { kind: "project", loupistId: event.target.value })}><option value="global">{i18n.t("settings:agentSettings.globalDefaults")}</option>{loupists.map((loupist) => <option key={loupist.id} value={loupist.id} disabled={loupist.availability.status !== "available"}>{loupistOptionLabel(loupist, loupists)}</option>)}</NativeSelect>
    </div> : null}
    <div className="agent-config-save-status" role="status" aria-live="polite">{config.busy ? i18n.t("settings:agentSettings.saving") : dirtyNumbers.size ? i18n.t("settings:agentSettings.unsavedAdvancedChangesUseSaveBesideEachField") : savedScope === scopeKey && !config.error ? <><CheckIcon aria-hidden="true" /> {i18n.t("settings:agentSettings.savedAppliesToNewResponses")}</> : i18n.t("settings:agentSettings.selectionsSaveAutomaticallyChangesApplyToNewResponses")}</div>
    {config.error ? <div className="inline-error config-diagnostics" role="alert"><p>{config.error}</p><Button variant="outline" size="sm" onClick={() => notifyConfigurationChanged()} disabled={config.busy}><RefreshCwIcon /> {i18n.t("settings:agentSettings.reload")}</Button></div> : null}
    <ConfigurationDiagnostics snapshot={snapshot} />
    {snapshot ? <>
      <section className="settings-section" aria-labelledby="chat-defaults-title"><div className="settings-section-heading"><h3 id="chat-defaults-title">{i18n.t("settings:agentSettings.chatDefaults")}</h3><p>{i18n.t("settings:agentSettings.startEachChatYourWayYouCanChange")}</p></div><div className="agent-config-fields">
        <SettingField {...field("chat.model_profile_id", i18n.t("settings:agentSettings.defaultModel"), i18n.t("settings:agentSettings.theModelToUseWhenStartingAChat"))}><NativeSelect id="config-chat.model_profile_id" aria-describedby="config-description-chat.model_profile_id" disabled={disabled} value={profileId} onChange={(event) => event.target.value ? void save("chat.model_profile_id", event.target.value) : reset("chat.model_profile_id")}><option value="">{scope.kind === "project" ? i18n.t("settings:agentSettings.useGlobalDefaultModel") : i18n.t("settings:agentSettings.automaticRecommendedModel")}</option>{profileId && !currentModel ? <option value={profileId}>{i18n.t("settings:agentSettings.unavailableModel")}</option> : null}{models.map((model) => <option key={model.id} value={model.id}>{model.name}</option>)}</NativeSelect></SettingField>
        <ProviderPlanLine className="agent-config-plan-line" provider={catalog?.providers.find((provider) => provider.id === optionModel?.provider)} onOpenUsage={(provider) => { void bridge.providerOpenUsage({ provider }).catch(() => {}); }} />
        <SettingField {...field("chat.thinking_level", i18n.t("settings:agentSettings.reasoningLevel"), i18n.t("settings:agentSettings.howMuchTimeTheModelSpendsThinkingBefore"))}><NativeSelect id="config-chat.thinking_level" aria-describedby="config-description-chat.thinking_level" disabled={disabled} value={thinking ?? "__automatic__"} onChange={(event) => event.target.value === "__automatic__" ? reset("chat.thinking_level") : void save("chat.thinking_level", event.target.value)}><option value="__automatic__">{scope.kind === "project" ? i18n.t("settings:agentSettings.useGlobalDefaultReasoningLevel") : i18n.t("settings:agentSettings.modelDefault")}</option>{thinking !== null && thinking !== undefined && !thinkingLevels.includes(thinking) ? <option value={thinking}>{i18n.t("settings:agentSettings.unavailableThinking", { level: thinking })}</option> : null}{thinkingLevels.map((level) => <option value={level} key={level}>{thinkingLabel(level) || i18n.t("settings:agentSettings.none")}</option>)}</NativeSelect></SettingField>
        <SettingField
          {...field("chat.use_memory", i18n.t("settings:loupistSettings.rememberNewChats"), i18n.t("settings:agentSettings.privateChatDefaultDescription"))}
          relatedPaths={["chat.update_memory"]}
          onReset={() => void update({}, [["chat", "use_memory"], ["chat", "update_memory"]])}
        >
          <Switch
            id="config-chat.use_memory"
            aria-describedby="config-description-chat.use_memory"
            disabled={disabled}
            checked={snapshot.effective.chat.use_memory && snapshot.effective.chat.update_memory}
            onCheckedChange={(remember) => void update({ chat: { use_memory: remember, update_memory: remember } })}
          />
        </SettingField>
        {unavailableOptions.length ? <div className="inline-error config-diagnostics" role="alert"><strong>{optionModel ? i18n.t("settings:agentSettings.unavailableOptionsForModel", { model: optionModel.name }) : i18n.t("settings:agentSettings.unavailableOptions")}</strong><p>{i18n.t("settings:agentSettings.resetTheseOptionsBeforeStartingChatOrChoose")}</p><ul>{unavailableOptions.map(([key], index) => {
          const local = snapshot.explicit?.chat?.run_options?.[key] !== undefined;
          const source = snapshot.sources[`chat.run_options.${key}`];
          const control = models.flatMap((model) => model.run_controls).find((candidate) => candidate.id === key);
          const label = control ? runControlLabel(control) : i18n.t("settings:agentSettings.unavailableOptionIndex", { index: formatNumber(index + 1) });
          return <li key={key}><span>{label}</span> <span>({source === "project" ? i18n.t("settings:agentSettings.thisLoupist") : source === "global" ? i18n.t("settings:agentSettings.global") : i18n.t("settings:agentSettings.appDefault")})</span> {local ? <Button variant="ghost" size="sm" disabled={disabled} onClick={() => void update({}, [["chat", "run_options", key]])} aria-label={i18n.t("settings:agentSettings.resetLabel", { label: label })}>{i18n.t("settings:agentSettings.resetOption")}</Button> : scope.kind === "project" && onScopeChange ? <Button variant="ghost" size="sm" onClick={() => onScopeChange({ kind: "global" })} aria-label={i18n.t("settings:agentSettings.editGlobalLabel", { label: label })}>{i18n.t("settings:agentSettings.editGlobalDefaults")}</Button> : null}</li>;
        })}</ul></div> : null}
      </div></section>
      <Disclosure className="agent-config-advanced" data-testid="agent-config-advanced" title={i18n.t("settings:agentSettings.advanced")} description={i18n.t("settings:agentSettings.contextLimitsAndRecoveryFromFailedRequests")}>
        {optionModel?.run_controls.length ? <div className="agent-config-group" data-run-controls><h4>{i18n.t("settings:agentSettings.performanceGroup")}</h4><div className="agent-config-fields">
          {optionModel?.run_controls.map((control) => <SettingField key={control.id} {...field(`chat.run_options.${control.id}`, runControlLabel(control), control.id === "performance" ? i18n.t("settings:agentSettings.performanceDescription") : undefined)}><NativeSelect id={`config-chat.run_options.${control.id}`} aria-describedby={control.id === "performance" ? `config-description-chat.run_options.${control.id}` : undefined} disabled={disabled} value={snapshot.effective.chat.run_options[control.id] ?? control.default_choice} onChange={(event) => void update({ chat: { run_options: { [control.id]: event.target.value } } })}>{snapshot.effective.chat.run_options[control.id] !== undefined && !control.choices.some((choice) => choice.id === snapshot.effective.chat.run_options[control.id]) ? <option value={snapshot.effective.chat.run_options[control.id]}>{i18n.t("settings:agentSettings.unavailableChoice")}</option> : null}{control.choices.map((choice) => <option key={choice.id} value={choice.id}>{runChoiceLabel(control, choice)}</option>)}</NativeSelect></SettingField>)}
        </div></div> : null}
        <div className="agent-config-group"><h4>{i18n.t("settings:agentSettings.conversationContext")}</h4><p>{i18n.t("settings:agentSettings.standardFitsANormalResearchChatKeepMore")}</p><div className="agent-config-presets"><Button type="button" variant="outline" size="sm" disabled={disabled} onClick={() => void update({ pi: { compaction: { reserve_tokens: 16384, keep_recent_tokens: 20000 } } })}>{i18n.t("settings:agentSettings.standard")}</Button><Button type="button" variant="outline" size="sm" disabled={disabled} onClick={() => void update({ pi: { compaction: { reserve_tokens: 16384, keep_recent_tokens: 40000 } } })}>{i18n.t("settings:agentSettings.keepMore")}</Button></div><details><summary>{i18n.t("settings:agentSettings.customAmounts")}</summary><div className="agent-config-fields">
          <NumberSetting {...field("pi.compaction.reserve_tokens", i18n.t("settings:agentSettings.reservedTokens"), i18n.t("settings:agentSettings.leaveRoomForTheModelSNextResponse"))} max={Number.MAX_SAFE_INTEGER} onSave={save} onDirtyChange={onNumberDirtyChange} />
          <NumberSetting {...field("pi.compaction.keep_recent_tokens", i18n.t("settings:agentSettings.recentTokensToKeep"), i18n.t("settings:agentSettings.keepThisMuchRecentConversationBeforeSummarizingOlder"))} max={Number.MAX_SAFE_INTEGER} onSave={save} onDirtyChange={onNumberDirtyChange} />
        </div></details></div>
        <div className="agent-config-group"><h4>{i18n.t("settings:agentSettings.requestRecovery")}</h4><p>{i18n.t("settings:agentSettings.retryAutomaticallyWhenAProviderHasATemporary")}</p><div className="agent-config-fields">
          <SettingField {...field("pi.retry.enabled", i18n.t("settings:agentSettings.retryFailedRequests"))}><NativeSelect id="config-pi.retry.enabled" disabled={disabled} value={String(snapshot.effective.pi.retry.enabled)} onChange={(event) => void save("pi.retry.enabled", event.target.value === "true")}><option value="true">{i18n.t("settings:agentSettings.enabled")}</option><option value="false">{i18n.t("settings:agentSettings.disabled")}</option></NativeSelect></SettingField>
          <NumberSetting {...field("pi.retry.max_retries", i18n.t("settings:agentSettings.maximumRetries"), i18n.t("settings:agentSettings.numberOfAttemptsAfterTheFirstFailure"))} max={10} onSave={save} onDirtyChange={onNumberDirtyChange} />
          <details><summary>{i18n.t("settings:agentSettings.customTiming")}</summary>
          <NumberSetting {...field("pi.retry.base_delay_ms", i18n.t("settings:agentSettings.initialRetryDelayMs"), i18n.t("settings:agentSettings.waitThisLongBeforeTheFirstRetry"))} max={300000} onSave={save} onDirtyChange={onNumberDirtyChange} />
          <NumberSetting {...field("pi.retry.max_agent_delay_ms", i18n.t("settings:agentSettings.maximumRetryDelayMs"), i18n.t("settings:agentSettings.limitHowLongAProviderCanAskThe"))} max={300000} onSave={save} onDirtyChange={onNumberDirtyChange} />
          </details>
        </div></div>
        <RunDiagnostics bridge={bridge} loupistId={scope.kind === "project" ? scope.loupistId : null} />
      </Disclosure>
    </> : !config.error ? <p className="settings-loading" role="status">{i18n.t("settings:agentSettings.loadingChatSettings")}</p> : null}
    <Disclosure className="agent-config-file-details" data-testid="agent-config-file-details" title={i18n.t("settings:agentSettings.preferencesFile")} description={i18n.t("settings:agentSettings.openTheFolderOrReloadExternalChanges")}>
      <div className="agent-config-file" data-testid="agent-config-file"><div><Button variant="outline" size="sm" title={snapshot?.path} onClick={() => void config.reveal()}><FolderOpenIcon /> {i18n.t("settings:agentSettings.openPreferencesFile")}</Button>{!config.error ? <Button variant="ghost" size="sm" onClick={() => notifyConfigurationChanged()} disabled={config.busy}><RefreshCwIcon /> {i18n.t("settings:agentSettings.reload")}</Button> : null}</div></div>
    </Disclosure>
  </>;
}
