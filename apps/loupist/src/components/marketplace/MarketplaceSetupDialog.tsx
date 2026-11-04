import { connectionText } from "./connectionCopy";
import i18n, { useTranslation } from "@/i18n";
import { Disclosure } from "../ui/disclosure";
import { ExternalLinkIcon, ShieldCheckIcon, LoaderCircleIcon, SlidersHorizontalIcon, Trash2Icon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { notifyConfigurationChanged } from "../../app/useConfiguration";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogBody,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type {
  MarketplaceConnectorStatus,
  MarketplaceEntry,
} from "../../marketplace/types";
import { CONNECTOR_READINESS_LABELS } from "../../marketplace/readiness";
import { errorMessage } from "../../lib/errors";
import type { LoupistBridge } from "../../types";
import { FREE_STATE_LABELS, setupEntryLabel } from "./MarketplaceCard";

type SetupMessage = {
  tone: "error" | "success";
  title: string;
  detail: string;
};

const VERIFICATION_LABELS: Record<
  MarketplaceConnectorStatus["credentials"][number]["verification"],
  string
> = {
  get never() { return i18n.t("connections:marketplaceSetupDialog.notVerified"); },
  get verified() { return i18n.t("connections:marketplaceSetupDialog.verified"); },
  get rejected() { return i18n.t("connections:marketplaceSetupDialog.verificationFailed"); },
  get temporarily_unavailable() { return i18n.t("connections:marketplaceSetupDialog.verificationUnavailable"); },
};

function selectOptionLabel(option: string) {
  const labels: Record<string, string> = {
    real: i18n.t("connections:marketplaceSetupDialog.live"),
    demo: i18n.t("connections:marketplaceSetupDialog.demo"),
    automatic: i18n.t("connections:marketplaceSetupDialog.automatic"),
    model_only: i18n.t("connections:marketplaceSetupDialog.modelSearchOnly"),
    exa_only: i18n.t("connections:marketplaceSetupDialog.exaOnly"),
  };
  return labels[option] ?? connectionText(option.charAt(0).toUpperCase() + option.slice(1));
}

export function MarketplaceSetupDialog({
  bridge,
  entry,
  connector,
  onClose,
  onChanged,
  onManageLoupists,
  onOpenModelSettings,
}: {
  bridge: LoupistBridge;
  entry: MarketplaceEntry | null;
  connector?: MarketplaceConnectorStatus;
  onClose: () => void;
  onChanged: () => Promise<void>;
  onManageLoupists?: () => void;
  onOpenModelSettings?: () => void;
}) {
  useTranslation();
  const [configDraft, setConfigDraft] = useState<Record<string, string>>({});
  const [credentialDraft, setCredentialDraft] = useState<
    Record<string, string>
  >({});
  const [setupBusy, setSetupBusy] = useState<"save" | "delete" | "refresh" | null>(null);
  const [setupProgress, setSetupProgress] = useState("");
  const [refreshNeeded, setRefreshNeeded] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [setupMessage, setSetupMessage] = useState<SetupMessage | null>(null);
  const [credentialToDelete, setCredentialToDelete] = useState<string | null>(
    null,
  );

  const dialogTitle = !entry ? "" : !entry.setup || connector?.readiness === "runtime_unavailable" ? connectionText(entry.name) : entry.id === "community.web-research" ? i18n.t("connections:marketplaceSetupDialog.settingsTitle", { name: connectionText(entry.name) }) : setupEntryLabel(entry, connector);
  const selectedEntryId = entry?.id ?? null;
  const selectedEntryIdRef = useRef(selectedEntryId);
  selectedEntryIdRef.current = selectedEntryId;
  const operationRef = useRef(0);
  const activeOperationRef = useRef<number | null>(null);

  useEffect(() => {
    operationRef.current += 1;
    activeOperationRef.current = null;
    setSetupBusy(null);
    setSetupProgress("");
    setRefreshNeeded(false);
    setDeleteError(null);
    if (!entry) {
      setConfigDraft({});
      setCredentialDraft({});
      setSetupBusy(null);
      setSetupMessage(null);
      setCredentialToDelete(null);
      return;
    }
    setConfigDraft(
      Object.fromEntries(
        (entry.setup?.config_fields ?? []).map((field) => [
          field.id,
          connector?.config[field.id] ??
            (field.kind === "select" ? (field.options[0] ?? "") : ""),
        ]),
      ),
    );
    setCredentialDraft({});
    setSetupMessage(null);
    setCredentialToDelete(null);
    // A snapshot refresh after save must not clear the result message.
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reset only when a different capability opens
  }, [selectedEntryId]);

  async function refreshSnapshot(entryId: string, operation: number) {
    try {
      await onChanged();
      if (selectedEntryIdRef.current === entryId && operationRef.current === operation) setRefreshNeeded(false);
      return true;
    } catch {
      if (selectedEntryIdRef.current === entryId && operationRef.current === operation) setRefreshNeeded(true);
      return false;
    }
  }

  async function retryRefresh() {
    if (!entry || activeOperationRef.current !== null) return;
    const entryId = entry.id;
    const operation = ++operationRef.current;
    activeOperationRef.current = operation;
    setSetupBusy("refresh");
    const refreshed = await refreshSnapshot(entryId, operation);
    if (selectedEntryIdRef.current === entryId && operationRef.current === operation) {
      setSetupBusy(null);
      if (refreshed) setSetupMessage({ tone: "success", title: i18n.t("connections:marketplaceSetupDialog.statusRefreshed"), detail: i18n.t("connections:marketplaceSetupDialog.theLatestConnectionStatusIsShownAbove") });
    }
    if (activeOperationRef.current === operation) activeOperationRef.current = null;
  }

  async function saveVerifyAndEnable() {
    if (!entry?.setup || connector?.readiness === "runtime_unavailable" || activeOperationRef.current !== null) return;
    const setup = entry.setup;
    const operationEntryId = entry.id;
    const operation = ++operationRef.current;
    activeOperationRef.current = operation;
    const secretValues = { ...credentialDraft };
    let phase: "validation" | "configure" | "save" | "verify" = "validation";
    setSetupMessage(null);
    try {
      const config = Object.fromEntries(
        setup.config_fields.map((field) => [
          field.id,
          (configDraft[field.id] ?? "").trim(),
        ]),
      );
      for (const field of setup.config_fields) {
        const value = config[field.id] ?? "";
        if (
          (field.required && !value) ||
          value.length < field.min_length ||
          value.length > field.max_length ||
          (field.kind === "select" && !field.options.includes(value))
        ) {
          throw new Error(i18n.t("connections:marketplaceSetupDialog.enterAValid", { value1: connectionText(field.label) }));
        }
      }
      const secrets = Object.fromEntries(
        setup.credential_fields.flatMap((field) => {
          const secret = (secretValues[field.id] ?? "").trim();
          return secret ? [[field.id, secret]] : [];
        }),
      );
      const patchingCredentials = Object.keys(secrets).length > 0;
      const configurationInvalidatesCredentials =
        (setup.credential_scope_fields ?? []).some(
          (fieldId) => connector?.config[fieldId] !== config[fieldId],
        ) && connector?.credentials.some((credential) => credential.stored);
      for (const field of setup.credential_fields) {
        const secret = secrets[field.id] ?? "";
        const existing = connector?.credentials.find(
          (credential) => credential.credential_id === field.id,
        );
        if (
          secret.length > 0 &&
          (secret.length < field.min_length || secret.length > field.max_length)
        ) {
          throw new Error(i18n.t("connections:marketplaceSetupDialog.enterAValid", { value1: connectionText(field.label) }));
        }
        if (secret.length > 0 && /\s/u.test(secret)) {
          throw new Error(i18n.t("connections:marketplaceSetupDialog.cannotContainWhitespace", { label: connectionText(field.label) }));
        }
        if (
          field.required &&
          !secret &&
          (!existing?.stored || configurationInvalidatesCredentials)
        ) {
          throw new Error(i18n.t("connections:marketplaceSetupDialog.isRequired", { label: connectionText(field.label) }));
        }
      }
      setSetupBusy("save");
      if (setup.config_fields.length) {
        phase = "configure";
        setSetupProgress(i18n.t("connections:marketplaceSetupDialog.savingSettings"));
        await bridge.marketplaceConnectorConfigure({
          entry_id: entry.id,
          config,
        });
        notifyConfigurationChanged();
      }
      if (patchingCredentials) {
        phase = "save";
        setSetupProgress(i18n.t("connections:marketplaceSetupDialog.savingCredentials"));
        await bridge.marketplaceCredentialSave({
          entry_id: entry.id,
          secrets,
        });
        if (selectedEntryIdRef.current === operationEntryId && operationRef.current === operation) setCredentialDraft({});
      }
      if (setup.credential_fields.length) {
        phase = "verify";
        setSetupProgress(i18n.t("connections:marketplaceSetupDialog.verifyingConnection"));
        await bridge.marketplaceCredentialVerify({
          entry_id: entry.id,
        });
      }
      const refreshed = await refreshSnapshot(operationEntryId, operation);
      if (selectedEntryIdRef.current !== operationEntryId || operationRef.current !== operation) return;
      setSetupMessage({
        tone: "success",
        title: setup.credential_fields.length
          ? i18n.t("connections:marketplaceSetupDialog.verificationSuccessful")
          : i18n.t("connections:marketplaceSetupDialog.settingsSaved"),
        detail: connector?.readiness === "ready"
          ? i18n.t(refreshed ? "connections:marketplaceSetupDialog.savedAccess" : "connections:marketplaceSetupDialog.savedAccessRefresh", { name: connectionText(entry.name) })
          : i18n.t(refreshed ? "connections:marketplaceSetupDialog.readyEnabled" : "connections:marketplaceSetupDialog.readyEnabledRefresh", { name: connectionText(entry.name) }),
      });
    } catch (cause: unknown) {
      if (phase === "configure" && cause && typeof cause === "object" && "code" in cause && cause.code === "config_apply_pending") {
        notifyConfigurationChanged();
      }
      if (selectedEntryIdRef.current !== operationEntryId || operationRef.current !== operation) return;
      setSetupMessage({
        tone: "error",
        title: phase === "verify" ? i18n.t("connections:marketplaceSetupDialog.verificationFailed") : phase === "save" ? i18n.t("connections:marketplaceSetupDialog.couldNotSaveCredentials") : i18n.t("connections:marketplaceSetupDialog.couldNotSaveSetup"),
        detail: errorMessage(cause, i18n.t("connections:marketplaceSetupDialog.couldNotConfigure", { name: connectionText(entry.name) })),
      });
      await refreshSnapshot(operationEntryId, operation);
    } finally {
      if (activeOperationRef.current === operation) activeOperationRef.current = null;
      if (selectedEntryIdRef.current === operationEntryId && operationRef.current === operation) setSetupBusy(null);
    }
  }

  async function deleteCredential() {
    if (!entry || activeOperationRef.current !== null) return;
    const operationEntryId = entry.id;
    const operation = ++operationRef.current;
    activeOperationRef.current = operation;
    setCredentialDraft({});
    setSetupBusy("delete");
    setSetupMessage(null);
    setDeleteError(null);
    let deleted = false;
    try {
      await bridge.marketplaceCredentialDelete({
        entry_id: entry.id,
      });
      deleted = true;
      const refreshed = await refreshSnapshot(operationEntryId, operation);
      if (selectedEntryIdRef.current !== operationEntryId || operationRef.current !== operation) return;
      setSetupMessage({
        tone: "success",
        title: i18n.t("connections:marketplaceSetupDialog.credentialsDeleted"),
        detail: i18n.t(refreshed ? "connections:marketplaceSetupDialog.deletedAccess" : "connections:marketplaceSetupDialog.deletedAccessRefresh"),
      });
    } catch (cause: unknown) {
      if (selectedEntryIdRef.current !== operationEntryId || operationRef.current !== operation) return;
      setDeleteError(errorMessage(cause, i18n.t("connections:marketplaceSetupDialog.couldNotDeleteTheSavedCredentialsTryAgain")));
      setSetupMessage({ tone: "error", title: i18n.t("connections:marketplaceSetupDialog.couldNotDeleteCredentials"), detail: errorMessage(cause, i18n.t("connections:marketplaceSetupDialog.couldNotDeleteTheSavedCredentialsTryAgain")) });
      await refreshSnapshot(operationEntryId, operation);
    } finally {
      if (activeOperationRef.current === operation) activeOperationRef.current = null;
      if (selectedEntryIdRef.current === operationEntryId && operationRef.current === operation) {
        setCredentialDraft({});
        setSetupBusy(null);
        if (deleted) setCredentialToDelete(null);
      }
    }
  }

  const scopeChanged = entry?.setup?.credential_scope_fields?.some(
    (fieldId) => connector?.config[fieldId] !== (configDraft[fieldId] ?? "").trim(),
  ) && connector?.credentials.some((credential) => credential.stored);

  async function openSetupHelp(url: string) {
    const helpEntryId = selectedEntryId;
    setSetupMessage(null);
    try {
      await bridge.openExternalUrl(url);
    } catch (cause: unknown) {
      if (selectedEntryIdRef.current !== helpEntryId) return;
      setSetupMessage({
        tone: "error",
        title: i18n.t("connections:marketplaceSetupDialog.couldNotOpenSetupGuide"),
        detail: errorMessage(cause, i18n.t("connections:marketplaceSetupDialog.couldNotOpenTheSetupGuide")),
      });
    }
  }

  return (
    <>
      <Dialog
        open={Boolean(entry)}
        onOpenChange={(open) => {
          if (!open) onClose();
        }}
      >
        {entry && (
          <DialogContent className="marketplace-setup-dialog">
            <form
              className="dialog-form marketplace-setup-form"
              onSubmit={(event) => {
                event.preventDefault();
                void saveVerifyAndEnable();
              }}
            >
              <DialogHeader>
                <DialogTitle>{dialogTitle}</DialogTitle>
                <DialogDescription>{connectionText(entry.summary)}</DialogDescription>
              </DialogHeader>

              <DialogBody>
              <div className="marketplace-setup-overview">
                <div className="marketplace-setup-status">
                  <Badge variant="outline" data-readiness={connector?.readiness ?? "needs_configuration"}>
                    {connector ? connectionText(CONNECTOR_READINESS_LABELS[connector.readiness]) : i18n.t("connections:marketplaceSetupDialog.needsSetup")}
                  </Badge>
                </div>
                <span>{FREE_STATE_LABELS[entry.free_state]}</span>
              </div>
              {connector?.readiness === "runtime_unavailable" ? (
                <div className="marketplace-setup-message" data-tone="error" role="alert">
                  <strong>{i18n.t("connections:marketplaceSetupDialog.connectionUnavailable")}</strong>
                  <p>{i18n.t("connections:marketplaceSetupDialog.updateOrReinstallLoupistToRestoreThis")}</p>
                </div>
              ) : !entry.setup ? (
                <div className="marketplace-credential-note"><ShieldCheckIcon aria-hidden="true" /><p>{i18n.t("connections:marketplaceSetupDialog.noSetupNeededChooseWhichLoupistsCanUse")}</p></div>
              ) : null}

              <fieldset className="marketplace-setup-fields" disabled={Boolean(setupBusy) || connector?.readiness === "runtime_unavailable"}>
                {entry.setup?.config_fields.length ? <div className="marketplace-form-section-title"><h3>{entry.id === "community.web-research" ? i18n.t("connections:marketplaceSetupDialog.searchRouting") : i18n.t("connections:marketplaceSetupDialog.connectionSettings")}</h3><p>{entry.id === "community.web-research" ? i18n.t("connections:marketplaceSetupDialog.automaticChoosesAnAvailableSearchServiceForYour") : i18n.t("connections:marketplaceSetupDialog.theseSettingsApplyToThisConnectionAcrossYour")}</p></div> : null}
                {entry.setup?.config_fields.map((field) => {
                  const inputId = `marketplace-${entry.id}-${field.id}`;
                  return (
                    <div className="marketplace-setup-field" key={field.id}>
                      <div className="marketplace-setup-field-heading">
                        <Label htmlFor={inputId}>
                          {!field.required && field.kind !== "select" ? i18n.t("connections:marketplaceSetupDialog.optionalField", { label: connectionText(field.label) }) : connectionText(field.label)}
                        </Label>
                        {field.help_url && (
                          <Button
                            type="button"
                            variant="link"
                            size="xs"
                            aria-label={`${i18n.t("connections:marketplaceSetupDialog.setupHelp")} · ${i18n.t("connections:marketplaceSetupDialog.openSetupHelp", { label: connectionText(field.label) })}`}
                            disabled={Boolean(setupBusy)}
                            onClick={() => void openSetupHelp(field.help_url!)}
                          >
                            {i18n.t("connections:marketplaceSetupDialog.setupHelp")}<ExternalLinkIcon aria-hidden="true" />
                          </Button>
                        )}
                      </div>
                      {field.kind === "select" ? (
                        <Select
                          value={
                            configDraft[field.id] ?? field.options[0] ?? ""
                          }
                          disabled={Boolean(setupBusy)}
                          onValueChange={(value) =>
                            setConfigDraft((current) => ({
                              ...current,
                              [field.id]: value,
                            }))
                          }
                        >
                          <SelectTrigger id={inputId} aria-label={connectionText(field.label)}>
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent position="popper">
                            {field.options.map((option) => (
                              <SelectItem value={option} key={option}>
                                {selectOptionLabel(option)}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      ) : (
                        <Input
                          id={inputId}
                          type={field.kind === "email" ? "email" : "text"}
                          value={configDraft[field.id] ?? ""}
                          required={field.required}
                          minLength={field.min_length}
                          maxLength={field.max_length}
                          disabled={Boolean(setupBusy)}
                          autoComplete={
                            field.kind === "email" ? "email" : "off"
                          }
                          onChange={(event) =>
                            setConfigDraft((current) => ({
                              ...current,
                              [field.id]: event.target.value,
                            }))
                          }
                        />
                      )}
                    </div>
                  );
                })}

                {scopeChanged && (
                  <div className="marketplace-setup-message" data-tone="error" role="status">
                    <strong>{i18n.t("connections:marketplaceSetupDialog.newConnectionScope")}</strong>
                    <p>{i18n.t("connections:marketplaceSetupDialog.savingThisChangeRemovesTheSavedCredentialsAnd")}</p>
                  </div>
                )}

                {entry.setup?.credential_fields.length ? <div className="marketplace-form-section-title"><h3>{i18n.t("connections:marketplaceSetupDialog.apiCredentials")}</h3><p>{connector?.credentials.some((credential) => credential.stored) ? i18n.t("connections:marketplaceSetupDialog.leaveSavedFieldsBlankToKeepTheCurrent") : i18n.t("connections:marketplaceSetupDialog.addYourProviderCredentialsThenVerifyTheConnection")}</p></div> : null}
                {entry.setup?.credential_fields.map((field) => {
                  const inputId = `marketplace-${entry.id}-${field.id}`;
                  const status = connector?.credentials.find(
                    (credential) => credential.credential_id === field.id,
                  );
                  return (
                    <div className="marketplace-setup-field" key={field.id}>
                      <div className="marketplace-setup-field-heading">
                        <Label htmlFor={inputId}>
                          {!field.required ? i18n.t("connections:marketplaceSetupDialog.optionalField", { label: connectionText(field.label) }) : connectionText(field.label)}
                        </Label>
                        {field.help_url && (
                          <Button
                            type="button"
                            variant="link"
                            size="xs"
                            aria-label={`${i18n.t("connections:marketplaceSetupDialog.getAKey")} · ${i18n.t("connections:marketplaceSetupDialog.openSetupHelp", { label: connectionText(field.label) })}`}
                            disabled={Boolean(setupBusy)}
                            onClick={() => void openSetupHelp(field.help_url!)}
                          >
                            {i18n.t("connections:marketplaceSetupDialog.getAKey")}<ExternalLinkIcon aria-hidden="true" />
                          </Button>
                        )}
                      </div>
                      <Input
                        id={inputId}
                        type="password"
                        value={credentialDraft[field.id] ?? ""}
                        placeholder={
                          status?.stored
                            ? i18n.t("connections:marketplaceSetupDialog.savedSecurelyLeaveBlankToKeepIt")
                            : i18n.t("connections:marketplaceSetupDialog.enter", { label: connectionText(field.label) })
                        }
                        required={field.required && (!status?.stored || Boolean(scopeChanged))}
                        minLength={field.min_length}
                        maxLength={field.max_length}
                        disabled={Boolean(setupBusy)}
                        autoComplete="new-password"
                        aria-describedby={`${inputId}-status`}
                        onChange={(event) =>
                          setCredentialDraft((current) => ({
                            ...current,
                            [field.id]: event.target.value,
                          }))
                        }
                      />
                      <p id={`${inputId}-status`}>
                        {status?.stored ? (
                          <>
                            {status.active && status.pending
                              ? i18n.t("connections:marketplaceSetupDialog.currentConnectionRetained", { value1: status.verification === "never" ? i18n.t("connections:marketplaceSetupDialog.replacementNotVerified") : VERIFICATION_LABELS[status.verification] })
                              : i18n.t("connections:marketplaceSetupDialog.storedSecurely", { value1: VERIFICATION_LABELS[status.verification] })}
                          </>
                        ) : (
                          i18n.t("connections:marketplaceSetupDialog.notStored")
                        )}
                      </p>
                      {status?.krx_coverage && (
                        <div className="marketplace-service-coverage">
                          <p>{i18n.t("connections:marketplaceSetupDialog.krxCoverage")}</p>
                          {(["kospi", "kosdaq"] as const).map(market => (
                            <p key={market}>
                              {i18n.t(`connections:marketplaceSetupDialog.krxMarket.${market}`, { service: market === "kospi" ? "stk_bydd_trd" : "ksq_bydd_trd" })}
                              {": "}{i18n.t(`connections:marketplaceSetupDialog.krxAccess.${status.krx_coverage![market]}`)}
                            </p>
                          ))}
                        </div>
                      )}
                      {status?.last_error && !setupMessage ? <p className="marketplace-credential-error">{status.last_error}</p> : null}
                    </div>
                  );
                })}
              </fieldset>

              {entry.id === "koreainvestment.market-data" && (
                <div className="marketplace-credential-note">
                  <ShieldCheckIcon aria-hidden="true" />
                  <p>
                    {i18n.t("connections:marketplaceSetupDialog.accountLookupsNeedYourAccountNumberAndProduct")}</p>
                </div>
              )}

              {entry.id === "community.web-research" && (
                <div className="marketplace-credential-note">
                  <ShieldCheckIcon aria-hidden="true" />
                  <p>
                    {i18n.t("connections:marketplaceSetupDialog.automaticUsesBuiltInWebSearchWithOpenai")}</p>
                </div>
              )}

              {(entry.setup?.credential_fields.length ?? 0) > 0 && (
                <div className="marketplace-credential-note">
                  <ShieldCheckIcon aria-hidden="true" />
                  <p>
                    {i18n.t("connections:marketplaceSetupDialog.keysAreStoredSecurelyOnThisDeviceAnd")}</p>
                </div>
              )}

              {entry.id === "community.web-research" && onOpenModelSettings ? <Button type="button" variant="link" className="marketplace-inline-action" onClick={() => { onClose(); onOpenModelSettings(); }}>{i18n.t("connections:marketplaceSetupDialog.modelsProviders")}<ExternalLinkIcon /></Button> : null}

              <Disclosure className="marketplace-details" title={i18n.t("connections:marketplaceSetupDialog.aboutThisConnection")}>
                <dl>
                  <div><dt>{i18n.t("connections:marketplaceSetupDialog.publisher")}</dt><dd>{entry.publisher}</dd></div>
                  <div><dt>{i18n.t("connections:marketplaceSetupDialog.integration")}</dt><dd>{entry.trust === "first_party" ? i18n.t("connections:marketplaceSetupDialog.official") : i18n.t("connections:marketplaceSetupDialog.reviewedCommunityIntegration")}</dd></div>
                  <div><dt>{i18n.t("connections:marketplaceSetupDialog.dataSources")}</dt><dd>{connectionText(entry.data_authority)}</dd></div>
                  {entry.markets.length ? <div><dt>{i18n.t("connections:marketplaceSetupDialog.markets")}</dt><dd>{entry.markets.map(connectionText).join(", ")}</dd></div> : null}
                  {entry.asset_classes.length ? <div><dt>{i18n.t("connections:marketplaceSetupDialog.coverage")}</dt><dd>{entry.asset_classes.map(connectionText).join(", ")}</dd></div> : null}
                  {entry.freshness.length ? <div><dt>{i18n.t("connections:marketplaceSetupDialog.freshness")}</dt><dd>{entry.freshness.map(connectionText).join(", ")}</dd></div> : null}
                  <div><dt>{i18n.t("connections:marketplaceSetupDialog.attribution")}</dt><dd>{connectionText(entry.attribution)}</dd></div>
                </dl>
                {entry.terms_url ? <Button type="button" variant="link" disabled={Boolean(setupBusy)} onClick={() => void openSetupHelp(entry.terms_url!)}>{i18n.t("connections:marketplaceSetupDialog.providerTerms")}<ExternalLinkIcon /></Button> : null}
              </Disclosure>

              {connector?.credentials.some((credential) => credential.stored) ? (
                <div className="marketplace-remove-credentials">
                  <div><strong>{i18n.t("connections:marketplaceSetupDialog.removeThisConnection")}</strong><p>{i18n.t("connections:marketplaceSetupDialog.deleteSavedKeysAndTurnOffAccessFor")}</p></div>
                  <Button type="button" variant="ghost" size="sm" disabled={Boolean(setupBusy)} onClick={() => { setDeleteError(null); setCredentialToDelete(entry.id); }}><Trash2Icon />{i18n.t("connections:marketplaceSetupDialog.deleteSavedCredentials")}</Button>
                </div>
              ) : null}

              {setupMessage && (
                <div
                  className="marketplace-setup-message"
                  data-tone={setupMessage.tone}
                  role={setupMessage.tone === "error" ? "alert" : "status"}
                >
                  <strong>{setupMessage.title}</strong>
                  <p>{setupMessage.detail}</p>
                </div>
              )}

              {refreshNeeded && (
                <Button type="button" variant="outline" size="sm" disabled={Boolean(setupBusy)} onClick={() => void retryRefresh()}>
                  {i18n.t("connections:marketplaceSetupDialog.refreshStatus")}</Button>
              )}

              {setupBusy === "save" && (
                <div className="marketplace-setup-progress" role="status"><LoaderCircleIcon className="animate-spin" aria-hidden="true" /><p><strong>{setupProgress}</strong><span>{i18n.t("connections:marketplaceSetupDialog.youCanCloseThisWindowWhileTheCheck")}</span></p></div>
              )}

              </DialogBody>
              <DialogFooter>
                {onManageLoupists ? <Button type="button" variant="ghost" className="marketplace-manage-access" onClick={() => { onClose(); onManageLoupists(); }}><SlidersHorizontalIcon />{i18n.t("connections:marketplaceSetupDialog.loupistAccess")}</Button> : null}
                <Button type="button" variant="outline" onClick={onClose}>
                  {i18n.t("connections:marketplaceSetupDialog.done")}</Button>
                {entry.setup && connector?.readiness !== "runtime_unavailable" ? <Button type="submit" disabled={Boolean(setupBusy)}>
                  {setupBusy === "save" ? <LoaderCircleIcon className="animate-spin" aria-hidden="true" /> : null}
                  {setupBusy === "save"
                    ? setupProgress
                    : entry.setup.credential_fields.length
                      ? i18n.t("connections:marketplaceSetupDialog.saveVerify")
                      : i18n.t("connections:marketplaceSetupDialog.saveSetup")}
                </Button> : null}
              </DialogFooter>
            </form>
          </DialogContent>
        )}
      </Dialog>
      <Dialog
        open={Boolean(credentialToDelete)}
        onOpenChange={(open) => {
          if (!open && setupBusy !== "delete") setCredentialToDelete(null);
        }}
      >
        <DialogContent className="marketplace-delete-dialog" showCloseButton={setupBusy !== "delete"}>
          <DialogHeader>
            <DialogTitle>{i18n.t("connections:marketplaceSetupDialog.deleteSavedCredentials2")}</DialogTitle>
            <DialogDescription>
              {i18n.t("connections:marketplaceSetupDialog.deleteDescription", { name: (entry ? connectionText(entry.name) : undefined) ?? i18n.t("connections:marketplaceSetupDialog.thisTool") })}</DialogDescription>
          </DialogHeader>
          {deleteError && <p className="marketplace-setup-message" data-tone="error" role="alert">{deleteError}</p>}
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              disabled={setupBusy === "delete"}
              onClick={() => setCredentialToDelete(null)}
            >
              {i18n.t("connections:marketplaceSetupDialog.cancel")}</Button>
            <Button
              type="button"
              variant="destructive"
              disabled={setupBusy === "delete"}
              onClick={() => {
                if (credentialToDelete) void deleteCredential();
              }}
            >
              {setupBusy === "delete" ? i18n.t("connections:marketplaceSetupDialog.deleting") : i18n.t("connections:marketplaceSetupDialog.deleteCredentials")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
