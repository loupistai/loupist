import i18n, { useTranslation } from "../../i18n";
import { KeyRoundIcon, LoaderCircleIcon, PlusIcon } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { errorMessage } from "../../lib/errors";
import type {
  ConfiguredModel,
  CustomModelProvider,
  LoupistBridge,
  ModelCatalog,
  ModelProviderOption,
  ModelVisibilityUpdateRequest,
  ProviderStatusView,
} from "../../types";
import { ConnectSheet, type ConnectSheetBridge, type ConnectSheetTarget } from "./ConnectSheet";
import { ProviderCard, providerCardTitle, stillUsed } from "./ProviderCard";

export type ModelProviderActions = {
  onLoadModels: (provider: string) => Promise<ModelCatalog>;
  onUpdateModelVisibility: (
    request: ModelVisibilityUpdateRequest,
  ) => Promise<ModelCatalog>;
  /** Signs a built-in provider out; for a Disconnected card it dismisses the leftover record. */
  onDisconnect: (provider: string) => Promise<ModelCatalog>;
  onRemoveCustomProvider: (provider: string) => Promise<ModelCatalog>;
  /** Every catalog the connect sheet returns. */
  onCatalogChange: (catalog: ModelCatalog) => void;
};

type Props = ModelProviderActions & {
  catalog: ModelCatalog | null;
  loadError: string | null;
  bridge: ConnectSheetBridge & Pick<LoupistBridge, "providerOpenUsage">;
  /** The effective Chat default; null or empty means Auto. */
  chatModelProfileId?: string | null;
  /** A connect sheet another surface asked for (a Chat error's "Use an API key"). */
  connectTarget?: ConnectSheetTarget | null;
  onConnectTargetConsumed?: () => void;
};

const missingStatus = (source: CustomModelProvider["credential_source"]): ProviderStatusView => ({
  state: source === "missing" ? "disconnected" : "connected",
  credential_kind: source === "missing" ? "none" : source === "environment" ? "environment" : "custom",
  notice: null, last_limit_at_ms: null, connected_at_ms: null,
  used_by: { schedules: 0, chat_default: false, world_models: 0 },
});

/** A user-added endpoint shown alongside the built-in providers; its key is entered in the custom sheet. */
const customOption = (provider: CustomModelProvider): ModelProviderOption => ({
  id: provider.id,
  label: provider.name,
  credential_label: "",
  description: provider.base_url,
  api_key: false,
  oauth: null,
  credential_source: provider.credential_source,
  recommended: false,
  group: "advanced",
  console_url: null,
  sign_in_flow: null,
  billing_note: null,
  status: provider.status ?? missingStatus(provider.credential_source),
  custom: provider,
});

/** A card appears for a working credential, one that needs reconnecting, or a disconnected account something still uses. */
const shownAsCard = (option: ModelProviderOption) =>
  option.credential_source !== "missing"
  || option.status.state === "needs_reconnect"
  || (option.status.state === "disconnected" && stillUsed(option.status));

const modelsForProvider = (
  catalog: ModelCatalog | null,
  providerId: string,
): ConfiguredModel[] =>
  (catalog?.models ?? []).filter((model) => model.provider === providerId);

type BusyOperation = "disconnect" | null;

/** Settings › Models & providers (spec providers §2.3): one card per account, connecting through the shared sheet. */
export function ModelSettingsPanel({
  catalog,
  loadError,
  bridge,
  chatModelProfileId,
  connectTarget,
  onConnectTargetConsumed,
  onLoadModels,
  onUpdateModelVisibility,
  onDisconnect,
  onRemoveCustomProvider,
  onCatalogChange,
}: Props) {
  useTranslation("settings");
  const [sheet, setSheet] = useState<ConnectSheetTarget | null>(null);
  const [busy, setBusy] = useState<BusyOperation>(null);
  const [busyProviderId, setBusyProviderId] = useState<string | null>(null);
  const [loadingProviderIds, setLoadingProviderIds] = useState<ReadonlySet<string>>(() => new Set());
  const [modelLoadErrors, setModelLoadErrors] = useState<Record<string, string | null>>({});
  const [visibilityBusyId, setVisibilityBusyId] = useState<string | null>(null);
  const [discoveredProviderIds, setDiscoveredProviderIds] = useState<ReadonlySet<string>>(() => new Set());
  const [pendingDisconnect, setPendingDisconnect] = useState<ModelProviderOption | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const operationRef = useRef(0);
  const operationActiveRef = useRef(false);
  const discoveryGenerationRef = useRef(0);
  const loadingProviderIdsRef = useRef(new Set<string>());

  useEffect(() => () => {
    operationRef.current += 1;
    discoveryGenerationRef.current += 1;
    loadingProviderIdsRef.current.clear();
  }, []);

  useEffect(() => {
    if (!connectTarget) return;
    setSheet(connectTarget);
    onConnectTargetConsumed?.();
  }, [connectTarget, onConnectTargetConsumed]);

  const options = useMemo(
    () => [...(catalog?.providers ?? []), ...(catalog?.custom_providers ?? []).map(customOption)],
    [catalog],
  );
  const cards = useMemo(() => options.filter(shownAsCard), [options]);
  const connectedProviders = useMemo(() => cards.filter((option) => option.credential_source !== "missing"), [cards]);
  const hiddenModels = useMemo(() => new Set(catalog?.hidden_model_profile_ids ?? []), [catalog]);
  const chatDefault = chatModelProfileId ? catalog?.models.find((model) => model.id === chatModelProfileId) : undefined;
  const providerMutationBusy = busy !== null;

  const markDiscovered = useCallback((nextProviderId: string) => {
    setDiscoveredProviderIds((current) => {
      if (current.has(nextProviderId)) return current;
      const next = new Set(current);
      next.add(nextProviderId);
      return next;
    });
  }, []);

  const refreshModels = async (nextProviderId: string) => {
    if (operationActiveRef.current || loadingProviderIdsRef.current.size > 0) return;
    const generation = discoveryGenerationRef.current;
    loadingProviderIdsRef.current.add(nextProviderId);
    setLoadingProviderIds(new Set(loadingProviderIdsRef.current));
    setModelLoadErrors((current) => ({ ...current, [nextProviderId]: null }));
    markDiscovered(nextProviderId);
    try {
      await onLoadModels(nextProviderId);
    } catch (cause) {
      if (generation !== discoveryGenerationRef.current) return;
      // Pi and Rust word these failures in English for logs; the card words its own.
      const busy = (cause as { code?: unknown } | null)?.code === "conflict";
      setModelLoadErrors((current) => ({ ...current, [nextProviderId]: i18n.t(busy ? "settings:modelSettingsPanel.modelsBusy" : "settings:modelSettingsPanel.couldNotRefreshModels") }));
    } finally {
      if (generation === discoveryGenerationRef.current) {
        loadingProviderIdsRef.current.delete(nextProviderId);
        setLoadingProviderIds(new Set(loadingProviderIdsRef.current));
      }
    }
  };
  const refreshModelsRef = useRef(refreshModels);
  refreshModelsRef.current = refreshModels;

  useEffect(() => {
    // A sign-in or key returns the provider's models itself; loading them alongside it collides.
    if (!catalog || busy !== null || sheet !== null || loadingProviderIds.size > 0) return;
    const pending = connectedProviders.find((option) => {
      const hasModels = catalog.models.some((model) => model.provider === option.id);
      return !hasModels && !discoveredProviderIds.has(option.id);
    });
    if (!pending) return;
    void refreshModelsRef.current(pending.id);
  }, [busy, catalog, connectedProviders, discoveredProviderIds, loadingProviderIds, sheet]);

  const updateVisibility = async (request: ModelVisibilityUpdateRequest) => {
    if (visibilityBusyId) return;
    setVisibilityBusyId(request.model_profile_id);
    setError(null);
    try {
      await onUpdateModelVisibility(request);
    } catch (cause) {
      setError(errorMessage(cause, i18n.t("settings:modelSettingsPanel.couldNotUpdateModelVisibility")));
    } finally {
      setVisibilityBusyId(null);
    }
  };

  const setProviderVisibility = async (nextProviderId: string, visible: boolean) => {
    if (visibilityBusyId || !catalog) return;
    const models = modelsForProvider(catalog, nextProviderId);
    setVisibilityBusyId(nextProviderId);
    setError(null);
    const remainingHidden = new Set(hiddenModels);
    try {
      for (const model of models) {
        const currentlyVisible = !remainingHidden.has(model.id);
        if (currentlyVisible === visible) continue;
        await onUpdateModelVisibility({ model_profile_id: model.id, visible_in_chat: visible });
        if (visible) remainingHidden.delete(model.id);
        else remainingHidden.add(model.id);
      }
    } catch (cause) {
      setError(errorMessage(cause, i18n.t("settings:modelSettingsPanel.couldNotUpdateModelVisibility")));
    } finally {
      setVisibilityBusyId(null);
    }
  };

  const disconnect = async (option: ModelProviderOption) => {
    const dismissing = option.status.state === "disconnected";
    if ((!dismissing && option.credential_source !== "saved") || busy || operationActiveRef.current) return;
    operationActiveRef.current = true;
    const operation = ++operationRef.current;
    const label = providerCardTitle(option);
    setBusy("disconnect");
    setBusyProviderId(option.id);
    setError(null);
    setStatus(dismissing ? null : i18n.t("settings:modelSettingsPanel.removingLabel", { label }));
    try {
      await (option.custom ? onRemoveCustomProvider(option.id) : onDisconnect(option.id));
      if (operation !== operationRef.current) return;
      setPendingDisconnect(null);
      setStatus(dismissing ? null : i18n.t("settings:modelSettingsPanel.labelWasRemoved", { label }));
    } catch (cause) {
      if (operation !== operationRef.current) return;
      setError(errorMessage(cause, i18n.t("settings:modelSettingsPanel.couldNotRemoveLabel", { label })));
      setStatus(null);
    } finally {
      operationActiveRef.current = false;
      if (operation === operationRef.current) {
        setBusy(null);
        setBusyProviderId(null);
      }
    }
  };

  const openSheet = (target: ConnectSheetTarget) => {
    setStatus(null);
    setError(null);
    setSheet(target);
  };

  return (
    <div className="settings-stack">
      {loadError ? (
        <Alert variant="destructive">
          <KeyRoundIcon />
          <AlertTitle>{i18n.t("settings:modelSettingsPanel.modelsUnavailable")}</AlertTitle>
          <AlertDescription>{loadError}</AlertDescription>
        </Alert>
      ) : null}

      <section aria-labelledby="connected-provider-title" className="grid gap-3">
        <div className="settings-section-heading">
          <div>
            <h2 id="connected-provider-title" className="font-heading text-base font-medium">
              {i18n.t("settings:modelSettingsPanel.modelProviders")}</h2>
          </div>
          {cards.length ? (
            <Button type="button" data-provider-connect disabled={providerMutationBusy} onClick={() => openSheet({ kind: "picker" })}>
              <PlusIcon />{i18n.t("settings:providers.addProvider")}</Button>
          ) : null}
        </div>

        {status ? (
          <Alert>
            <AlertTitle>{i18n.t("settings:modelSettingsPanel.providerUpdated")}</AlertTitle>
            <AlertDescription>{status}</AlertDescription>
          </Alert>
        ) : null}
        {error && !pendingDisconnect ? (
          <Alert variant="destructive">
            <KeyRoundIcon />
            <AlertTitle>{i18n.t("settings:modelSettingsPanel.providerUpdateFailed")}</AlertTitle>
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        ) : null}

        {cards.length ? (
          <ul className="provider-card-list" aria-label={i18n.t("settings:modelSettingsPanel.connectedProviders")}>
            {cards.map((option) => (
              <ProviderCard
                key={option.id}
                option={option}
                models={modelsForProvider(catalog, option.id)}
                hiddenModels={hiddenModels}
                chatModel={chatModelProfileId ? (chatDefault?.provider === option.id ? chatDefault : undefined) : null}
                loading={loadingProviderIds.has(option.id)}
                removing={busy === "disconnect" && busyProviderId === option.id}
                loadError={modelLoadErrors[option.id]}
                locked={providerMutationBusy || visibilityBusyId !== null}
                reloadDisabled={loadingProviderIds.size > 0 || providerMutationBusy || visibilityBusyId !== null}
                visibilityBusyId={visibilityBusyId}
                onReload={() => void refreshModels(option.id)}
                onConnect={openSheet}
                onOpenUsage={(provider) => { void bridge.providerOpenUsage({ provider }).catch((cause) => setError(errorMessage(cause, i18n.t("settings:modelSettingsPanel.providerUpdateFailed")))); }}
                onRemove={() => {
                  setError(null);
                  if (option.status.state === "disconnected") void disconnect(option);
                  else setPendingDisconnect(option);
                }}
                onSetVisibility={(model_profile_id, visible_in_chat) => void updateVisibility({ model_profile_id, visible_in_chat })}
                onSetAllVisible={(visible) => void setProviderVisibility(option.id, visible)}
              />
            ))}
          </ul>
        ) : catalog ? (
          <div className="settings-provider-empty">
            <p className="text-base font-medium">{i18n.t("settings:modelSettingsPanel.bringYourPreferredAI")}</p>
            <p className="mt-1 text-sm text-muted-foreground">
              {i18n.t("settings:modelSettingsPanel.signInWithAnExistingAccountOrAdd")}</p>
            <Button type="button" className="mt-4" data-provider-connect disabled={providerMutationBusy} onClick={() => openSheet({ kind: "picker" })}>
              <PlusIcon />{i18n.t("settings:providers.addProvider")}</Button>
          </div>
        ) : !loadError ? (
          <div className="settings-provider-empty" role="status">
            <LoaderCircleIcon className="mx-auto size-5 animate-spin text-muted-foreground" aria-hidden="true" />
            <p className="mt-3 text-sm text-muted-foreground">{i18n.t("settings:modelSettingsPanel.loadingModelProviders")}</p>
          </div>
        ) : null}
      </section>

      <ConnectSheet
        target={sheet}
        catalog={catalog}
        bridge={bridge}
        onCatalogChange={onCatalogChange}
        onConnected={(provider, label) => {
          markDiscovered(provider);
          setStatus(i18n.t("settings:modelSettingsPanel.providerConnected", { provider: label }));
        }}
        onClose={() => setSheet(null)}
      />

      <Dialog
        open={pendingDisconnect !== null}
        onOpenChange={(open) => {
          if (!open && busy !== "disconnect") setPendingDisconnect(null);
        }}
      >
        <DialogContent showCloseButton={busy !== "disconnect"}>
          <DialogHeader>
            <DialogTitle>
              {pendingDisconnect
                ? i18n.t(pendingDisconnect.custom ? "settings:modelSettingsPanel.customRemoveTitle" : "settings:modelSettingsPanel.disconnectConfirmationTitle", { label: providerCardTitle(pendingDisconnect) })
                : i18n.t("settings:modelSettingsPanel.disconnectProvider")}
            </DialogTitle>
            <DialogDescription>
              {pendingDisconnect
                ? i18n.t(pendingDisconnect.custom ? "settings:modelSettingsPanel.customRemoveBody" : "settings:modelSettingsPanel.savedCredentialsForLabelWillBeRemovedIts", { label: providerCardTitle(pendingDisconnect) })
                : i18n.t("settings:modelSettingsPanel.savedCredentialsWillBeRemoved")}
            </DialogDescription>
          </DialogHeader>
          {error ? <Alert variant="destructive"><AlertTitle>{i18n.t("settings:modelSettingsPanel.couldNotDisconnectProvider")}</AlertTitle><AlertDescription>{error}</AlertDescription></Alert> : null}
          <DialogFooter>
            <Button type="button" variant="outline" disabled={busy === "disconnect"} onClick={() => setPendingDisconnect(null)}>
              {i18n.t("settings:modelSettingsPanel.cancel")}</Button>
            <Button
              type="button"
              variant="destructive"
              disabled={busy === "disconnect" || !pendingDisconnect}
              onClick={() => {
                if (pendingDisconnect) void disconnect(pendingDisconnect);
              }}
            >
              {busy === "disconnect" ? i18n.t("settings:modelSettingsPanel.disconnecting") : i18n.t(pendingDisconnect?.custom ? "settings:modelSettingsPanel.customRemove" : "settings:modelSettingsPanel.disconnect")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
