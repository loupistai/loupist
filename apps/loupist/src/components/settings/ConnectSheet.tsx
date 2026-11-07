import { useCallback, useEffect, useId, useState, type ReactNode } from "react";
import { AlertCircleIcon, CheckCircle2Icon, ChevronLeftIcon, ExternalLinkIcon, LoaderCircleIcon } from "lucide-react";
import { useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
import { Button } from "@/components/ui/button";
import { Dialog, DialogBody, DialogContent, DialogHeader, DialogTitle, focusDialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { errorMessage } from "@/lib/errors";
import { providerKeyName, providerShortName, UPSTAGE_COMPANY } from "@/chat/providerNames";
import type {
  CustomModelProvider, CustomProviderApi, LocalDiscovery, LocalEngine, LoupistBridge, ModelCatalog,
  ProviderCustomTestOutcome,
} from "@/types";
import { ProviderWays, SIGN_IN_LABEL } from "./ProviderWays";
import { SignInFlow } from "./SignInFlow";
import { useSignIn, type SignInController } from "./signIn";
import "@/styles/connect.css";

/**
 * What the connect sheet opens on (providers spec §2.2). Settings › Add a
 * provider opens the picker; a card's Reconnect, a notice's "Use an API key"
 * and Chat's error actions open one provider directly.
 */
export type ConnectSheetTarget =
  | { kind: "picker" }
  /** A built-in provider: its sign-in (waiting card in the sheet) or its API-key sheet. */
  | { kind: "provider"; provider: string; method: "sign_in" | "api_key" }
  | { kind: "upstage" }
  | { kind: "local" }
  | { kind: "custom"; edit?: CustomModelProvider };

export type ConnectSheetBridge = Pick<LoupistBridge,
  | "providerConnect" | "providerConnectCancel" | "providerConnectOpenBrowser" | "providerConnectSubmitCode"
  | "providerConfigure" | "providerCustomSave" | "providerPresetSave" | "providerLocalDiscover"
  | "providerCustomTest" | "providerOpenConsole">;

export type ConnectSheetProps = {
  target: ConnectSheetTarget | null;
  catalog: ModelCatalog | null;
  bridge: ConnectSheetBridge;
  /** Every catalog Rust returns while connecting. */
  onCatalogChange: (catalog: ModelCatalog) => void;
  /** A provider finished connecting; `label` is its display name. */
  onConnected?: (provider: string, label: string) => void;
  onClose: () => void;
};

/** Research needs at least this much context (spec providers §2.2). */
export const MIN_RESEARCH_CONTEXT = 20_000;
const MAX_CUSTOM_MODELS = 20;
const signInLabel = (provider: string) => SIGN_IN_LABEL[provider as keyof typeof SIGN_IN_LABEL] ?? SIGN_IN_LABEL.openai;
const TEST_COPY = {
  ok: "providers.testOk", unreachable: "providers.testUnreachable", unauthorized: "providers.testUnauthorized",
  timeout: "providers.testTimeout", not_found: "providers.testNotFound", wrong_dialect: "providers.testWrongDialect", invalid: "providers.testInvalid",
} as const satisfies Record<ProviderCustomTestOutcome, string>;
const DIALECTS = [
  ["openai-responses", "providers.dialectResponses"], ["openai-completions", "providers.dialectCompletions"], ["anthropic-messages", "providers.dialectAnthropic"],
] as const satisfies ReadonlyArray<readonly [CustomProviderApi, string]>;

const errorCode = (cause: unknown) => cause && typeof cause === "object" && "code" in cause && typeof cause.code === "string" ? cause.code : null;
const parseModelIds = (value: string) => [...new Set(value.split(/[\n,]/).map(item => item.trim()).filter(Boolean))];
const hasApiPath = (address: string) => {
  try { return new URL(address).pathname.replace(/\/+$/, "") !== ""; } catch { return false; }
};

/** The name a connected provider goes by once Rust returns its catalog. */
export function connectedName(catalog: ModelCatalog, provider: string) {
  const option = catalog.providers.find(item => item.id === provider);
  if (option) return providerShortName(provider, option.status.credential_kind) ?? option.label;
  return catalog.custom_providers?.find(item => item.id === provider)?.name ?? provider;
}

/** Settings and onboarding share this dialog; onboarding opens it from its own list of ways. */
export function ConnectSheet({ target, catalog, bridge, onCatalogChange, onConnected, onClose }: ConnectSheetProps) {
  const { t } = useTranslation("settings");
  const { t: to } = useTranslation("onboarding");
  const [view, setView] = useState<ConnectSheetTarget | null>(target);
  const [busy, setBusy] = useState(false);
  useEffect(() => { setView(target); setBusy(false); }, [target]);
  const finish = useCallback((provider: string, next: ModelCatalog) => {
    onCatalogChange(next);
    onConnected?.(provider, connectedName(next, provider));
    onClose();
  }, [onCatalogChange, onConnected, onClose]);
  const flow = useSignIn(bridge, finish);
  const open = target !== null && view !== null;
  const back = () => {
    if (flow.busy) void flow.cancel();
    flow.dismiss();
    if (target?.kind === "picker") setView({ kind: "picker" }); else onClose();
  };
  const close = () => {
    if (busy) return;
    if (flow.busy) void flow.cancel();
    onClose();
  };
  const option = view?.kind === "provider" ? catalog?.providers.find(item => item.id === view.provider) : undefined;
  const title = !view ? "" : view.kind === "picker" ? t("providers.addProvider")
    : view.kind === "provider" ? (view.method === "sign_in" ? to(signInLabel(view.provider)) : providerKeyName(view.provider, option?.label ?? view.provider))
      : view.kind === "upstage" ? providerKeyName("upstage", UPSTAGE_COMPANY)
        : view.kind === "local" ? to("wizard.local")
          : view.edit ? t("modelSettingsPanel.customEditTitle") : to("wizard.custom");
  const sheetKind = !view ? "closed" : view.kind === "provider" ? view.method : view.kind;
  return <Dialog open={open} onOpenChange={next => { if (!next) close(); }}>
    <DialogContent className="connect-sheet" data-connect-sheet={sheetKind} showCloseButton={!busy} onOpenAutoFocus={focusDialogTitle}
      onEscapeKeyDown={event => { if (flow.busy) { event.preventDefault(); void flow.cancel(); } else if (busy) event.preventDefault(); }}
      onInteractOutside={event => { if (busy || flow.busy) event.preventDefault(); }}>
      <DialogHeader className="connect-sheet-header">
        {view && view.kind !== "picker" ? <Button type="button" variant="ghost" size="sm" className="connect-sheet-back" disabled={busy} onClick={back}><ChevronLeftIcon />{t("providers.allWays")}</Button> : null}
        <DialogTitle>{title}</DialogTitle>
      </DialogHeader>
      <DialogBody className="connect-sheet-body">
        {view?.kind === "picker" ? <ProviderWays catalog={catalog} includeMore actions={{
          onSignIn: provider => { setView({ kind: "provider", provider, method: "sign_in" }); if (provider !== "anthropic") void flow.start(provider); },
          onKey: provider => setView({ kind: "provider", provider, method: "api_key" }),
          onUpstage: () => setView({ kind: "upstage" }),
          onLocal: () => setView({ kind: "local" }),
          onCustom: () => setView({ kind: "custom" }),
        }} /> : null}
        {view?.kind === "provider" && view.method === "sign_in" ? <SignInView provider={view.provider} flow={flow} autoStart={target?.kind === "provider"}
          onUseApiKey={() => { flow.dismiss(); setView({ kind: "provider", provider: "openai", method: "api_key" }); }} /> : null}
        {view?.kind === "provider" && view.method === "api_key" ? <KeyView key={view.provider} provider={view.provider} catalog={catalog} bridge={bridge} onBusy={setBusy} onDone={finish} /> : null}
        {view?.kind === "upstage" ? <KeyView key="upstage" provider="upstage" catalog={catalog} bridge={bridge} onBusy={setBusy} onDone={finish} /> : null}
        {view?.kind === "local" ? <LocalView bridge={bridge} onBusy={setBusy} onDone={finish} /> : null}
        {view?.kind === "custom" ? <CustomView key={view.edit?.id ?? "new"} edit={view.edit} bridge={bridge} onBusy={setBusy} onDone={finish} /> : null}
      </DialogBody>
    </DialogContent>
  </Dialog>;
}

function SignInView({ provider, flow, autoStart, onUseApiKey }: { provider: string; flow: SignInController; autoStart: boolean; onUseApiKey: () => void }) {
  const { t } = useTranslation("onboarding");
  // Reconnect from a card opens straight into the browser. Only on open.
  useEffect(() => { if (autoStart) void flow.start(provider); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  return <div className="connect-sheet-section">
    {provider === "openai" ? <p className="connect-meta">{t("wizard.chatgptNote")}</p> : null}
    {flow.state.phase === "idle" ? <Button type="button" className="connect-primary" data-signin-start onClick={() => void flow.start(provider)}>{t(signInLabel(provider))}</Button> : null}
    <SignInFlow flow={flow} onUseApiKey={onUseApiKey} />
  </div>;
}

function KeyView({ provider, catalog, bridge, onBusy, onDone }: { provider: string; catalog: ModelCatalog | null; bridge: ConnectSheetBridge; onBusy: (busy: boolean) => void; onDone: (provider: string, catalog: ModelCatalog) => void }) {
  const { t } = useTranslation("settings");
  const id = useId();
  const option = catalog?.providers.find(item => item.id === provider);
  const name = providerKeyName(provider, option?.label ?? provider);
  const [key, setKey] = useState("");
  const [shown, setShown] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const preset = provider === "upstage";
  const hasConsole = preset || Boolean(option?.console_url);
  const submit = async () => {
    if (!key.trim() || saving) return;
    setSaving(true); onBusy(true); setError(null);
    try {
      const next = preset ? await bridge.providerPresetSave({ preset: "upstage", api_key: key.trim() }) : await bridge.providerConfigure({ provider, api_key: key.trim() });
      onBusy(false);
      const saved = preset ? next.custom_providers?.find(item => item.name === name)?.id ?? provider : provider;
      onDone(saved, next);
    } catch (cause) {
      onBusy(false); setSaving(false);
      setError(errorCode(cause) === "key_rejected" ? t("providers.keyRejected", { provider: preset ? UPSTAGE_COMPANY : name }) : errorMessage(cause, t("modelSettingsPanel.couldNotConfigureThisProvider")));
    }
  };
  return <form className="connect-sheet-section" data-key-sheet={provider} onSubmit={event => { event.preventDefault(); void submit(); }}>
    <div className="connect-sheet-scroll">
    {hasConsole ? <button type="button" className="connect-link" data-get-key onClick={() => void bridge.providerOpenConsole({ provider }).catch(() => {})}>{t("providers.getKey", { provider: preset ? UPSTAGE_COMPANY : name })}<ExternalLinkIcon aria-hidden="true" /></button> : null}
    {provider === "openai" && option?.status.credential_kind === "plan" ? <p className="connect-note" data-tone="info" data-replaces-chatgpt>{t("providers.replacesChatgpt")}</p> : null}
    <div className="connect-field">
      <Label htmlFor={`${id}-key`}>{t("providers.apiKey")}</Label>
      <div className="connect-secret">
        <Input id={`${id}-key`} type={shown ? "text" : "password"} autoComplete="new-password" spellCheck={false} value={key} maxLength={8192} disabled={saving} aria-describedby={`${id}-stays`} onChange={event => { setKey(event.target.value); setError(null); }} />
        <Button type="button" variant="ghost" size="sm" className="connect-secret-toggle" aria-pressed={shown} onClick={() => setShown(value => !value)}>{t(shown ? "providers.hideKey" : "providers.showKey")}</Button>
      </div>
      <p id={`${id}-stays`} className="connect-meta">{t("providers.keyStays", { provider: preset ? UPSTAGE_COMPANY : name })}</p>
    </div>
    {error ? <p className="connect-note" data-tone="negative" role="alert">{error}</p> : null}
    </div>
    <div className="connect-actions"><Button type="submit" disabled={!key.trim() || saving}>{saving ? <LoaderCircleIcon className="animate-spin" aria-hidden="true" /> : null}{t("providers.connect")}</Button></div>
  </form>;
}

function Spinning({ children }: { children: ReactNode }) {
  return <p className="connect-progress" role="status"><LoaderCircleIcon className="connect-spinner animate-spin" aria-hidden="true" /><span>{children}</span></p>;
}

const usable = (model: LocalEngine["models"][number]) => model.context_tokens === null || model.context_tokens >= MIN_RESEARCH_CONTEXT;

function LocalView({ bridge, onBusy, onDone }: { bridge: ConnectSheetBridge; onBusy: (busy: boolean) => void; onDone: (provider: string, catalog: ModelCatalog) => void }) {
  const { t } = useTranslation("settings");
  const [discovery, setDiscovery] = useState<LocalDiscovery | null>(null);
  const [searching, setSearching] = useState(true);
  const [choice, setChoice] = useState<{ engine: string; model: string } | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const look = useCallback(async () => {
    setSearching(true); setError(null);
    try {
      const found = await bridge.providerLocalDiscover();
      setDiscovery(found);
      const engine = found.engines.find(item => item.running && item.models.some(usable));
      const model = engine?.models.find(usable);
      setChoice(engine && model ? { engine: engine.engine, model: model.id } : null);
    } catch (cause) {
      setDiscovery(null);
      setError(errorMessage(cause, t("providers.localNone")));
    } finally { setSearching(false); }
  }, [bridge, t]);
  useEffect(() => { void look(); }, [look]);
  const running = discovery?.engines.filter(engine => engine.running) ?? [];
  const chosen = discovery?.engines.find(engine => engine.engine === choice?.engine);
  const connect = async () => {
    if (!chosen || !choice || saving) return;
    setSaving(true); onBusy(true); setError(null);
    try {
      const next = await bridge.providerCustomSave({ name: chosen.label, base_url: chosen.base_url, api: "openai-completions", model_ids: [choice.model] });
      onBusy(false);
      const saved = next.custom_providers?.find(item => item.base_url === chosen.base_url.replace(/\/+$/u, "") && item.name === chosen.label)?.id ?? chosen.engine;
      onDone(saved, next);
    } catch (cause) {
      onBusy(false); setSaving(false);
      setError(errorMessage(cause, t("modelSettingsPanel.couldNotConfigureThisProvider")));
    }
  };
  return <div className="connect-sheet-section" data-local-sheet>
    <div className="connect-sheet-scroll">
    {searching ? <Spinning>{t("providers.localSearching")}</Spinning> : null}
    {!searching && discovery && !running.length ? <p className="connect-note" data-tone="warning" data-local-none>{t("providers.localNone")}</p> : null}
    {!searching && discovery ? discovery.engines.map(engine => engine.running ? <fieldset key={engine.engine} className="connect-engine" data-engine={engine.engine}>
      <legend>{t("providers.localFound", { engine: engine.label, formattedCount: formatNumber(engine.models.length) })}</legend>
      {engine.models.map(model => {
        const short = !usable(model);
        const checked = choice?.engine === engine.engine && choice.model === model.id;
        return <label key={model.id} className="connect-model" data-disabled={short || undefined} data-selected={checked || undefined}>
          <input type="radio" name="connect-local-model" value={model.id} checked={checked} disabled={short || saving} onChange={() => setChoice({ engine: engine.engine, model: model.id })} />
          <span className="connect-model-copy">
            <span className="connect-model-name">{model.name}</span>
            <span className="connect-model-context gg-num">{model.context_tokens === null ? t("providers.contextUnknown") : t("providers.context", { tokens: formatNumber(model.context_tokens) })}</span>
            {short ? <span className="connect-model-short">{t("providers.tooShort")}</span> : null}
          </span>
        </label>;
      })}
    </fieldset> : <p key={engine.engine} className="connect-engine-off" data-engine={engine.engine}>{t("providers.localOff", { engine: engine.label })}</p>) : null}
    {!searching ? <p className="connect-meta">{t("providers.contextNote")}</p> : null}
    {error ? <p className="connect-note" data-tone="negative" role="alert">{error}</p> : null}
    </div>
    <div className="connect-actions">
      <Button type="button" variant="ghost" disabled={searching || saving} onClick={() => void look()}>{t("providers.lookAgain")}</Button>
      {chosen ? <Button type="button" data-local-connect disabled={searching || saving} onClick={() => void connect()}>{saving ? <LoaderCircleIcon className="animate-spin" aria-hidden="true" /> : null}{t("providers.localConnect", { engine: chosen.label })}</Button> : null}
    </div>
  </div>;
}

/** The address as Rust stores it: a parsed URL without a trailing slash. */
function sameAddress(value: string) {
  try { return new URL(value.trim()).href.replace(/\/+$/u, ""); } catch { return value.trim(); }
}

function CustomView({ edit, bridge, onBusy, onDone }: { edit?: CustomModelProvider; bridge: ConnectSheetBridge; onBusy: (busy: boolean) => void; onDone: (provider: string, catalog: ModelCatalog) => void }) {
  const { t } = useTranslation("settings");
  const id = useId();
  const [name, setName] = useState(edit?.name ?? "");
  const [address, setAddress] = useState(edit?.base_url ?? "");
  const [api, setApi] = useState<CustomProviderApi>(edit?.api ?? "openai-completions");
  const [key, setKey] = useState("");
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<{ outcome: ProviderCustomTestOutcome; models: string[] } | null>(null);
  const [checked, setChecked] = useState<ReadonlySet<string>>(() => new Set());
  const [typing, setTyping] = useState(Boolean(edit));
  const [typed, setTyped] = useState(edit?.model_ids.join("\n") ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // A changed address, dialect or key needs a new Test.
  const invalidate = () => { setResult(null); setChecked(new Set()); setError(null); };
  const test = async () => {
    if (!address.trim() || testing) return;
    setTesting(true); setError(null);
    try {
      const outcome = await bridge.providerCustomTest({ base_url: address.trim(), api, api_key: key.trim() || undefined });
      setResult(outcome);
      setChecked(new Set(outcome.outcome === "ok" ? outcome.models.slice(0, MAX_CUSTOM_MODELS) : []));
    } catch (cause) {
      setResult(null);
      setError(errorMessage(cause, t("providers.testUnreachable")));
    } finally { setTesting(false); }
  };
  const typedIds = parseModelIds(typed);
  // Rust drops a saved key when the address changes; it never follows the endpoint to a new host.
  const moved = edit ? sameAddress(address) !== edit.base_url : false;
  const modelIds = typing ? typedIds : [...checked];
  const tested = result?.outcome === "ok";
  const ready = Boolean(name.trim() && address.trim() && modelIds.length && modelIds.length <= MAX_CUSTOM_MODELS && (tested || (typing && typedIds.length)));
  const add = async () => {
    if (!ready || saving) return;
    setSaving(true); onBusy(true); setError(null);
    try {
      const next = await bridge.providerCustomSave({ provider: edit?.id, name: name.trim(), base_url: address.trim(), api, api_key: key.trim() || undefined, model_ids: modelIds });
      onBusy(false);
      const saved = edit?.id ?? next.custom_providers?.find(item => item.name === name.trim())?.id ?? name.trim();
      onDone(saved, next);
    } catch (cause) {
      onBusy(false); setSaving(false);
      setError(errorMessage(cause, t("modelSettingsPanel.couldNotConfigureThisProvider")));
    }
  };
  const toggle = (model: string, on: boolean) => setChecked(current => {
    const next = new Set(current);
    if (on && next.size < MAX_CUSTOM_MODELS) next.add(model); else next.delete(model);
    return next;
  });
  return <form className="connect-sheet-section" data-custom-sheet onSubmit={event => { event.preventDefault(); void add(); }}>
    <div className="connect-sheet-scroll">
    <div className="connect-field">
      <Label htmlFor={`${id}-name`}>{t("providers.name")}</Label>
      <Input id={`${id}-name`} value={name} maxLength={60} disabled={saving} placeholder={t("modelSettingsPanel.customNamePlaceholder")} onChange={event => setName(event.target.value)} />
    </div>
    <div className="connect-field">
      <Label htmlFor={`${id}-address`}>{t("providers.address")}</Label>
      <Input id={`${id}-address`} type="url" inputMode="url" value={address} maxLength={512} disabled={saving} spellCheck={false} aria-describedby={`${id}-address-hint`} onChange={event => { setAddress(event.target.value); invalidate(); }} />
      <p id={`${id}-address-hint`} className="connect-meta">{t("providers.addressHint")}</p>
    </div>
    <div className="connect-field">
      <span className="connect-field-label" id={`${id}-dialect`}>{t("providers.dialect")}</span>
      <div className="connect-segmented" role="radiogroup" aria-labelledby={`${id}-dialect`}>
        {DIALECTS.map(([value, label]) => <label key={value} data-selected={api === value || undefined}>
          <input type="radio" name={`${id}-dialect`} value={value} checked={api === value} disabled={saving} onChange={() => { setApi(value); invalidate(); }} />
          <span>{t(label)}</span>
        </label>)}
      </div>
    </div>
    <div className="connect-field">
      <Label htmlFor={`${id}-key`}>{t("providers.apiKeyOptional")}</Label>
      <Input id={`${id}-key`} type="password" autoComplete="new-password" spellCheck={false} value={key} maxLength={8192} disabled={saving} aria-describedby={edit ? `${id}-key-hint` : undefined} onChange={event => { setKey(event.target.value); invalidate(); }} />
      {edit ? <p id={`${id}-key-hint`} className="connect-meta" data-key-hint={moved ? "cleared" : "kept"}>{t(moved ? "providers.addressClearsKey" : "modelSettingsPanel.customApiKeyKeep")}</p> : null}
    </div>
    <div className="connect-actions connect-actions-start">
      <Button type="button" variant="outline" data-custom-test disabled={!address.trim() || testing || saving} onClick={() => void test()}>{testing ? <LoaderCircleIcon className="animate-spin" aria-hidden="true" /> : null}{t("providers.test")}</Button>
    </div>
    {result ? <p className="connect-test-result" role="status" data-outcome={result.outcome}>
      {result.outcome === "ok" ? <CheckCircle2Icon aria-hidden="true" /> : <AlertCircleIcon aria-hidden="true" />}
      <span>{t(result.outcome === "not_found" && hasApiPath(address) ? "providers.testNotFoundPath" : TEST_COPY[result.outcome], { formattedCount: formatNumber(result.models.length) })}</span>
    </p> : null}
    {tested && !typing && result.models.length ? <fieldset className="connect-test-models" aria-label={t("providers.models", { formattedCount: formatNumber(result.models.length) })}>
      {result.models.map(model => <label key={model} className="connect-model">
        <input type="checkbox" checked={checked.has(model)} disabled={saving || (!checked.has(model) && checked.size >= MAX_CUSTOM_MODELS)} onChange={event => toggle(model, event.target.checked)} />
        <span className="connect-model-name">{model}</span>
      </label>)}
    </fieldset> : null}
    {!typing ? <button type="button" className="connect-link" data-type-ids onClick={() => setTyping(true)}>{t("providers.typeIds")}</button> : <div className="connect-field">
      <Label htmlFor={`${id}-ids`}>{t("modelSettingsPanel.customModelIds")}</Label>
      <Textarea id={`${id}-ids`} rows={3} value={typed} disabled={saving} spellCheck={false} aria-describedby={`${id}-ids-hint`} onChange={event => setTyped(event.target.value)} />
      <p id={`${id}-ids-hint`} className="connect-meta">{t("modelSettingsPanel.customModelIdsHint")}</p>
    </div>}
    {error ? <p className="connect-note" data-tone="negative" role="alert">{error}</p> : null}
    </div>
    <div className="connect-actions">
      <Button type="submit" data-custom-add disabled={!ready || saving}>{saving ? <LoaderCircleIcon className="animate-spin" aria-hidden="true" /> : null}{t(edit ? "modelSettingsPanel.customSave" : "providers.add")}</Button>
    </div>
  </form>;
}
