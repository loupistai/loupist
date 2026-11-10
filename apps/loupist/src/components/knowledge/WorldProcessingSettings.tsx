import i18n, { useTranslation } from "../../i18n";
import { Disclosure } from "@/components/ui/disclosure";
import { useEffect, useRef, useState } from "react";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Badge } from "../ui/badge";
import { ModelSelectionMenu } from "../models/ModelSelectionMenu";
import { worldReadinessLabels } from "../../knowledge/worldReadiness";
import { errorMessage } from "../../lib/errors";
import { formatNumber } from "@/i18n/format";
import type { GraphProcessingSettings, GraphProcessingView, GraphReadinessNote, KeepAwakeStatus, LoupistBridge, ModelCatalog } from "../../types";

type Props = {
  bridge: LoupistBridge;
  loupistId: string;
  active?: boolean;
  collapsible?: boolean;
  onChanged: () => void;
  onManageProviders?: () => void;
};

type Loaded = { loupistId: string; view: GraphProcessingView; draft: GraphProcessingSettings; catalog: ModelCatalog };
const readinessNotes = (): Record<GraphReadinessNote, string> => ({
  recovery_required: i18n.t("settings:worldSettings.readiness.recoveryRequired"),
  recovering: i18n.t("settings:worldSettings.readiness.recovering"),
  runtime_unavailable: i18n.t("settings:worldSettings.readiness.runtimeUnavailable"),
  awaiting_provider: i18n.t("settings:worldSettings.readiness.awaitingProvider"),
  choose_model: i18n.t("settings:worldSettings.readiness.chooseModel"),
  model_unavailable: i18n.t("settings:worldSettings.readiness.modelUnavailable"),
  connect_provider: i18n.t("settings:worldSettings.readiness.connectProvider"),
  isolation_unsupported: i18n.t("settings:worldSettings.readiness.isolationUnsupported"),
  runtime_missing: i18n.t("settings:worldSettings.readiness.runtimeMissing"),
  choose_connected_model: i18n.t("settings:worldSettings.readiness.chooseConnectedModel"),
  not_connected: i18n.t("settings:worldSettings.readiness.notConnected"),
  account_waiting: i18n.t("settings:worldSettings.readiness.accountWaiting"),
});
const recoveryComplete = () => i18n.t("knowledge:worldProcessingSettings.worldProcessingRecoveredSavedSourcesAreRetainedInterruptedProcessingWas");
const KEEP_AWAKE_REFRESH_MS = 5_000;

/** One host choice for every Loupist. It saves at once and never joins the per-Loupist form. */
function KeepAwakeSetting({ bridge, active }: { bridge: LoupistBridge; active: boolean }) {
  const { t } = useTranslation("knowledge");
  const [status, setStatus] = useState<KeepAwakeStatus | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The newest read or save wins; an older answer never overwrites it.
  const latest = useRef(0);
  const enabled = status?.enabled ?? false;
  const read = (isCurrent: () => boolean) => {
    const ticket = ++latest.current;
    return bridge.worldKeepAwakeGet().then((next) => {
      if (isCurrent() && latest.current === ticket) setStatus(next);
    }, () => undefined); // A failed read keeps the last status.
  };
  useEffect(() => {
    if (!active) return;
    let current = true;
    void read(() => current);
    return () => { current = false; };
  }, [active, bridge]);
  // While enabled, refresh how much World work is left.
  useEffect(() => {
    if (!active || !enabled || saving) return;
    let current = true;
    let timer: ReturnType<typeof setTimeout>;
    const poll = () => { timer = setTimeout(() => void read(() => current).then(() => { if (current) poll(); }), KEEP_AWAKE_REFRESH_MS); };
    poll();
    return () => { current = false; clearTimeout(timer); };
  }, [active, bridge, enabled, saving]);

  if (!status?.supported) return null;
  const toggle = (next: boolean) => {
    if (saving) return;
    const ticket = ++latest.current;
    setSaving(true);
    setError(null);
    void bridge.worldKeepAwakeSet({ enabled: next }).then((result) => {
      if (latest.current === ticket) setStatus(result);
    }, (cause: unknown) => {
      setError(errorMessage(cause, t("worldProcessingSettings.couldNotSaveTheKeepAwakeSetting")));
    }).finally(() => setSaving(false));
  };
  return <div className="world-processing-keep-awake">
    <label>
      <input className="knowledge-checkbox" type="checkbox" checked={status.enabled} disabled={saving} onChange={(event) => toggle(event.target.checked)} />
      {t("worldProcessingSettings.keepThisMacAwakeWhileTheWorldIsProcessing")}</label>
    <p className="knowledge-hint">{t("worldProcessingSettings.keepAwakeAppliesToEveryLoupistOnlyWhilePluggedIn")}</p>
    {status.active ? <p role="status">{t("worldProcessingSettings.keepingThisMacAwakeSourcesLeft", { count: status.remaining, formattedCount: formatNumber(status.remaining) })}</p> : null}
    {status.enabled && !status.active && status.onBattery ? <p className="knowledge-hint">{t("worldProcessingSettings.onBatteryThisMacCanSleepAsUsual")}</p> : null}
    {error ? <p role="alert">{error}</p> : null}
  </div>;
}

export function WorldProcessingSettings({ bridge, loupistId, active = true, collapsible = false, onChanged, onManageProviders }: Props) {
  const { t } = useTranslation("knowledge");
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [action, setAction] = useState<"save" | "pause" | "recover" | null>(null);
  const [recoveryPollFailed, setRecoveryPollFailed] = useState(false);
  const [recoveryPoll, setRecoveryPoll] = useState(0);
  const [reload, setReload] = useState(0);
  const serial = useRef(0);
  const [relationText, setRelationText] = useState("");
  const [anchorText, setAnchorText] = useState("");
  const loupistRef = useRef(loupistId);
  loupistRef.current = loupistId;
  const changedRef = useRef(onChanged);
  changedRef.current = onChanged;

  useEffect(() => {
    const ticket = ++serial.current;
    if (!active) return;
    setLoaded(null);
    setError(null);
    setNote(null);
    setAction(null);
    setRecoveryPollFailed(false);
    void Promise.all([bridge.knowledgeGraphSettingsGet({ loupistId }), bridge.modelCatalogGet()]).then(([view, catalog]) => {
      if (loupistRef.current !== loupistId || serial.current !== ticket) return;
      setLoaded({ loupistId, view, draft: view.settings, catalog });
      setRelationText((view.settings.relationHints ?? []).join(", "));
      setAnchorText((view.settings.anchors ?? []).map((anchor) => [anchor.name, ...(anchor.aliases ?? []), ...(anchor.identifiers ?? [])].join(" | ")).join("; "));
    }).catch((cause: unknown) => {
      if (loupistRef.current !== loupistId || serial.current !== ticket) return;
      setError(errorMessage(cause, t("worldProcessingSettings.couldNotOpenWorldProcessingSettings")));
    });
    return () => { serial.current += 1; };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `t` only words the failure; a language change must not reload.
  }, [active, bridge, loupistId, reload]);

  const current = loaded?.loupistId === loupistId ? loaded : null;
  const readiness = current?.view.readiness.status;
  const recovering = readiness === "recovering";
  useEffect(() => {
    if (!active || action || (readiness !== "ready" && readiness !== "recovering")) return;
    const ticket = serial.current;
    const interval = readiness === "recovering" ? 2_000 : 5_000;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    setRecoveryPollFailed(false);
    const refresh = async () => {
      try {
        const next = await bridge.knowledgeGraphSettingsGet({ loupistId });
        if (cancelled || loupistRef.current !== loupistId || serial.current !== ticket) return;
        setLoaded((previous) => previous?.loupistId === loupistId ? { ...previous, view: next } : previous);
        if (recovering) setError(null);
        if (next.readiness.status === readiness) timer = setTimeout(() => void refresh(), interval);
        else if (recovering) {
          setNote(next.readiness.status === "reconciliation_required" ? null : recoveryComplete());
          changedRef.current();
        } else setNote(null);
      } catch (cause: unknown) {
        if (cancelled || loupistRef.current !== loupistId || serial.current !== ticket) return;
        setError(errorMessage(cause, t("worldProcessingSettings.couldNotRefreshWorldProcessingStatus")));
        setRecoveryPollFailed(true);
      }
    };
    timer = setTimeout(() => void refresh(), interval);
    return () => { cancelled = true; clearTimeout(timer); };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `t` only words the failure; a language change must not reload.
  }, [active, action, bridge, loupistId, readiness, recovering, recoveryPoll]);

  if (!current) return error ? <div className="knowledge-settings-error"><p role="alert">{error}</p><Button type="button" variant="outline" onClick={() => setReload((value) => value + 1)}>{t("worldProcessingSettings.tryAgain")}</Button></div> : <p role="status">{t("worldProcessingSettings.openingWorldProcessingSettings")}</p>;
  const { draft, view, catalog } = current;
  const busy = action !== null || recovering;
  const models = catalog.models.filter((model) => model.credential_source !== "missing" && catalog.providers.find((provider) => provider.id === model.provider)?.credential_source !== "missing");
  const readinessLabel = view.readiness.status === "model_required" && !models.length ? t("worldProcessingSettings.waitingForAProvider") : worldReadinessLabels()[view.readiness.status];
  const readinessMessage = (view.readiness.note ? readinessNotes()[view.readiness.note] : null) || (!models.length ? t("worldProcessingSettings.noConnectedModelsConnectAProviderToStartProcessing") : null);
  const selectedModel = models.find((model) => model.id === draft.helperModelProfileId);
  const savedRelations = (view.settings.relationHints ?? []).join(", ");
  const savedAnchors = (view.settings.anchors ?? []).map((anchor) => [anchor.name, ...(anchor.aliases ?? []), ...(anchor.identifiers ?? [])].join(" | ")).join("; ");
  const changed = JSON.stringify(draft) !== JSON.stringify(view.settings) || relationText !== savedRelations || anchorText !== savedAnchors;
  const edit = (patch: Partial<GraphProcessingSettings>) => {
    setLoaded((previous) => previous?.loupistId === loupistId ? { ...previous, draft: { ...previous.draft, ...patch } } : previous);
    setNote(null);
  };
  const save = (pause?: boolean) => {
    if (busy) return;
    const ticket = ++serial.current;
    const captured = loupistId;
    setAction(pause === undefined ? "save" : "pause");
    setNote(null);
    setError(null);
    const settings = {
      ...draft,
      relationHints: relationText.split(",").map((item) => item.trim()).filter(Boolean).slice(0, 32),
      anchors: anchorText === savedAnchors ? view.settings.anchors : anchorText.split(";").map((line) => line.split("|").map((item) => item.trim()).filter(Boolean)).filter((parts) => parts.length).slice(0, 200).map((parts) => ({ name: parts[0], aliases: parts.slice(1), identifiers: [] })),
    };
    const request = pause === undefined
      ? bridge.knowledgeGraphSettingsSet({ loupistId: captured, settings })
      : bridge.knowledgeGraphPause({ loupistId: captured, paused: pause });
    void request.then((saved) => {
      if (loupistRef.current !== captured || serial.current !== ticket) return;
      setLoaded((previous) => previous?.loupistId === captured ? { ...previous, view: saved, draft: pause === undefined ? saved.settings : { ...previous.draft, paused: saved.settings.paused } } : previous);
      if (pause === undefined) {
        setRelationText((saved.settings.relationHints ?? []).join(", "));
        setAnchorText((saved.settings.anchors ?? []).map((anchor) => [anchor.name, ...(anchor.aliases ?? []), ...(anchor.identifiers ?? [])].join(" | ")).join("; "));
      }
      setNote(pause === undefined ? t("worldProcessingSettings.worldProcessingSettingsSavedForThisLoupist") : saved.settings.paused ? t("worldProcessingSettings.worldProcessingPaused") : t("worldProcessingSettings.worldProcessingResumed"));
      onChanged();
    }).catch((cause: unknown) => {
      if (loupistRef.current !== captured || serial.current !== ticket) return;
      setError(errorMessage(cause, t("worldProcessingSettings.couldNotSaveWorldProcessingSettings")));
    }).finally(() => { if (serial.current === ticket) setAction(null); });
  };
  const recover = () => {
    if (busy || view.readiness.status !== "reconciliation_required") return;
    const ticket = ++serial.current;
    const captured = loupistId;
    setAction("recover");
    setNote(null);
    setError(null);
    void bridge.knowledgeGraphRecover({ loupistId: captured }).then((next) => {
      if (loupistRef.current !== captured || serial.current !== ticket) return;
      setLoaded((previous) => previous?.loupistId === captured ? { ...previous, view: next } : previous);
      if (next.readiness.status !== "recovering") {
        setNote(next.readiness.status === "reconciliation_required" ? null : recoveryComplete());
        onChanged();
      }
    }).catch((cause: unknown) => {
      if (loupistRef.current !== captured || serial.current !== ticket) return;
      setError(errorMessage(cause, t("worldProcessingSettings.couldNotRecoverWorldProcessing")));
    }).finally(() => { if (serial.current === ticket) setAction(null); });
  };

  const form = <form className="knowledge-processing-form" aria-label={t("worldProcessingSettings.worldProcessingSettings")} onSubmit={(event) => { event.preventDefault(); save(); }}>
    <p className="knowledge-hint">{t("worldProcessingSettings.processingSendsSavedSourceTextToYourSelectedProviderAnd")}</p>
    <div className="world-processing-status" aria-label={t("worldProcessingSettings.worldProcessingReadiness")}>
      <Badge variant={view.readiness.status === "ready" ? "secondary" : "outline"}>{readinessLabel}</Badge>
      {view.readiness.modelName ? <span>{view.readiness.providerName ? `${view.readiness.providerName} · ` : ""}{view.readiness.modelName}</span> : null}
      {readinessMessage ? <p className="knowledge-hint">{readinessMessage}</p> : null}
    </div>
    {view.readiness.status === "reconciliation_required" || recovering ? <div className="world-processing-recovery">
      <p>{t("worldProcessingSettings.recoveryRestoresTheLastConfirmedWorldAndKeepsYourSaved")}</p>
      {recovering ? <p role="status">{t("worldProcessingSettings.restoringConfirmedWorldData")}</p> : <Button type="button" variant="outline" disabled={busy} onClick={recover}>{action === "recover" ? t("worldProcessingSettings.startingRecovery") : t("worldProcessingSettings.recoverWorldProcessing")}</Button>}
      {recoveryPollFailed ? <Button type="button" variant="outline" onClick={() => setRecoveryPoll((value) => value + 1)}>{t("worldProcessingSettings.refreshRecoveryStatus")}</Button> : null}
    </div> : null}
    <fieldset disabled={busy}>
      <legend className="sr-only">{t("worldProcessingSettings.processingPreferencesForThisLoupist")}</legend>
      <label className="world-processing-opt-in">
        <input className="knowledge-checkbox" type="checkbox" checked={draft.enrichmentEnabled} onChange={(event) => edit({ enrichmentEnabled: event.target.checked })} />
        {t("worldProcessingSettings.automaticallyProcessSavedSources")}</label>
      <div className="world-processing-model">
        <span>{t("worldProcessingSettings.processingModel")}</span>
        {models.length ? <ModelSelectionMenu models={models} providers={catalog.providers} disabled={busy} ariaLabel={t("worldProcessingSettings.worldProcessingModel")} selection={{ model_profile_id: draft.helperModelProfileId ?? "", thinking_level: draft.thinkingLevel, run_options: draft.runOptions }}
          onSelectionChange={(selection) => edit({ helperModelProfileId: selection.model_profile_id, thinkingLevel: selection.thinking_level, runOptions: selection.run_options })} /> : null}
        {models.length ? <p className="knowledge-hint">{t("worldProcessingSettings.thisChoiceIsSeparateFromTheModelSelectedInChat")}</p> : null}
        {models.length > 0 && draft.helperModelProfileId && !selectedModel ? <p>{t("worldProcessingSettings.theSavedModelIsUnavailableReconnectItsProviderOrChoose")}</p> : null}
        {onManageProviders ? <Button type="button" variant="ghost" onClick={onManageProviders}>{view.readiness.status === "provider_disconnected" ? t("worldProcessingSettings.reconnectProvider") : t("worldProcessingSettings.manageProviders")}</Button> : null}
      </div>
      <Disclosure title={t("worldProcessingSettings.researchGuidance")} description={t("worldProcessingSettings.focusRelationshipNamesAndAnchorEntities")}>
      <label>{t("worldProcessingSettings.whatWillThisLoupistResearch")}<Input aria-label={t("worldProcessingSettings.researchFocus")} value={draft.researchFocus ?? ""} maxLength={1000} onChange={(event) => edit({ researchFocus: event.target.value })} />
        <span className="knowledge-hint">{t("worldProcessingSettings.optionalLaterSourcesAreReadWithThisEmphasisItDoes")}</span>
      </label>
      <label>{t("worldProcessingSettings.preferredRelationshipNames")}<Input aria-label={t("worldProcessingSettings.preferredRelationshipNames")} value={relationText} onChange={(event) => setRelationText(event.target.value)} />
        <span className="knowledge-hint">{t("worldProcessingSettings.optionalUsedWhenTheyFitNewRelationshipsAreStillKept")}</span>
      </label>
      <label>{t("worldProcessingSettings.anchorEntities")}<Input aria-label={t("worldProcessingSettings.anchorEntities")} placeholder="Samsung Electronics | 삼성전자 | 005930" value={anchorText} onChange={(event) => setAnchorText(event.target.value)} />
        <span className="knowledge-hint">{t("worldProcessingSettings.separateEntitiesWithSemicolonsTheFirstNameIsTheMain")}</span>
      </label>
      </Disclosure>
      <Disclosure title={t("worldProcessingSettings.advancedLimits")}>
        <div className="knowledge-processing-fields">
          <label>{t("worldProcessingSettings.maximumModelRequestsPerSource")}<Input aria-label={t("worldProcessingSettings.maximumModelRequestsPerSource")} type="number" min={1} max={64} step={1} required value={draft.maxLlmCalls} onChange={(event) => edit({ maxLlmCalls: Number(event.target.value) })} /><span className="knowledge-hint">{t("worldProcessingSettings.164RequestsMoreRequestsCanIncreaseCost")}</span></label>
          <label>{t("worldProcessingSettings.timeLimitPerRequestSeconds")}<Input aria-label={t("worldProcessingSettings.timeLimitPerRequestSeconds")} type="number" min={1} max={300} step={1} required value={draft.timeoutMs / 1000} onChange={(event) => edit({ timeoutMs: Number(event.target.value) * 1000 })} /><span className="knowledge-hint">{t("worldProcessingSettings.1300SecondsPerRequest")}</span></label>
          <label>{t("worldProcessingSettings.fastModelForRoutineChecks")}<Input aria-label={t("worldProcessingSettings.fastModel")} value={draft.fastModel ?? ""} onChange={(event) => edit({ fastModel: event.target.value.trim() || null })} /><span className="knowledge-hint">{t("worldProcessingSettings.optionalFasterModelForRoutineChecksSuchAsSpottingDuplicates")}</span></label>
        </div>
      </Disclosure>
    </fieldset>
    {changed ? <p className="knowledge-hint">{t("worldProcessingSettings.unsavedChangesStatusAboveReflectsTheSavedSettings")}</p> : null}
    <div className="knowledge-inspector-actions">
      <Button type="submit" variant="outline" disabled={busy || !changed || (draft.enrichmentEnabled && !selectedModel)}>{action === "save" || action === "pause" ? t("worldProcessingSettings.savingSettings") : t("worldProcessingSettings.saveProcessingSettings")}</Button>
      {changed ? <Button type="button" variant="ghost" disabled={busy} onClick={() => { setLoaded((previous) => previous?.loupistId === loupistId ? { ...previous, draft: previous.view.settings } : previous); setRelationText(savedRelations); setAnchorText(savedAnchors); setError(null); setNote(null); }}>{t("worldProcessingSettings.discardChanges")}</Button> : null}
      <Button type="button" variant="ghost" disabled={busy} onClick={() => save(!draft.paused)}>{draft.paused ? t("worldProcessingSettings.resumeProcessing") : t("worldProcessingSettings.pauseProcessing")}</Button>
      <Button type="button" variant="ghost" disabled={busy || changed} onClick={() => setReload((value) => value + 1)}>{t("worldProcessingSettings.refreshStatus")}</Button>
    </div>
    {error ? <p role="alert">{error}</p> : null}
    {note ? <p role="status">{note}</p> : null}
  </form>;
  const keepAwake = <KeepAwakeSetting bridge={bridge} active={active} />;
  return collapsible ? <details className="knowledge-advanced world-processing-panel">
    <summary><span>{t("worldProcessingSettings.worldProcessing")}</span><Badge variant="outline">{readinessLabel}</Badge>{view.readiness.modelName ? <span>{view.readiness.modelName}</span> : <span className="knowledge-hint">{t("worldProcessingSettings.chooseModel")}</span>}</summary>
    {form}
    {keepAwake}
  </details> : <>{form}{keepAwake}</>;
}
