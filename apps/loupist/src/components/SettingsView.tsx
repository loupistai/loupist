import { NoticeSettingsRow } from "./onboarding/Notice";
import { MarketplaceView } from "./marketplace/MarketplaceView";
import { LoupistAvatar } from "./app/AppHeader";
import i18n, { useTranslation } from "../i18n";
import { SkillsSettings } from "./settings/SkillsSettings";
import { RefreshCwIcon } from "lucide-react";
import { Button } from "./ui/button";
import { PageHeader } from "./ui/page-header";
import type { LanguagePreference } from "../i18n/useLanguage";
import type { ThemePreference } from "../app/useTheme";
import { SETTINGS_SECTIONS, type SettingsSection } from "../app/navigation";
import { KeyboardSettings } from "./settings/KeyboardSettings";
import type { ModelCatalog } from "../types";
import { AppearanceSettings } from "./settings/AppearanceSettings";
import { ModelSettingsPanel, type ModelProviderActions } from "./settings/ModelSettingsPanel";
import type { ConnectSheetTarget } from "./settings/ConnectSheet";
import { UpdateSettingsCard } from "./settings/UpdateSettingsCard";
import type { useAppUpdate } from "./settings/useAppUpdate";
import type { ConfigScope, LoupistSummary, LoupistBridge } from "../types";
import { AgentSettings, ConfigurationDiagnostics } from "./settings/AgentSettings";
import { notifyConfigurationChanged, type useConfiguration } from "../app/useConfiguration";
import { useState } from "react";
import { sharingReleaseVisible } from "../sharing/releaseVisibility";
import { ShareSheet } from "./sharing/ShareSheet";

type Props = {
  bridge: LoupistBridge;
  loupists: LoupistSummary[];
  connectionsEntryId?: string | null;
  onOpenLoupistSettings?: (id: string) => void;
  onCreateLoupist?: () => void;
  onImportLoupist?: () => void;
  onRemoveLoupist?: (id: string) => void;
  onOpenLoupists?: () => void;
  /** Opens one Loupist's settings at Skills & connections. */
  onOpenLoupistConnections?: (id: string) => void;
  currentLoupistId: string | null;
  onManageProviders: () => void;
  onDraftSkill?: (scope: ConfigScope, brief: string) => void | Promise<void>;
  appearanceConfiguration: ReturnType<typeof useConfiguration>;
  section: SettingsSection;
  catalog: ModelCatalog | null;
  theme: ThemePreference;
  language: LanguagePreference | "en-XA";
  onLanguageChange: (language: LanguagePreference | "en-XA") => void;
  loadError: string | null;
  onThemeChange: (theme: ThemePreference) => void;
  providerActions: ModelProviderActions;
  /** A connect sheet requested from elsewhere (a Chat error action); consumed once. */
  connectTarget?: ConnectSheetTarget | null;
  onConnectTargetConsumed?: () => void;
  update: ReturnType<typeof useAppUpdate>;
};

export function SettingsView({
  bridge,
  loupists, connectionsEntryId, onOpenLoupistSettings, onCreateLoupist, onImportLoupist, onRemoveLoupist, onOpenLoupists, onOpenLoupistConnections,
  currentLoupistId,
  onManageProviders,
  onDraftSkill,
  appearanceConfiguration,
  section,
  catalog,
  theme,
  language,
  onLanguageChange,
  loadError,
  onThemeChange,
  providerActions,
  connectTarget,
  onConnectTargetConsumed,
  update,
}: Props) {
  useTranslation("settings");
  const page = SETTINGS_SECTIONS.find((item) => item.id === section)!;
  const [shareTarget, setShareTarget] = useState<LoupistSummary | null>(null);
  const showSharing = sharingReleaseVisible();
  return (
    <section className="settings-page" aria-labelledby={section === "connections" ? "marketplace-title" : "settings-title"} tabIndex={section === "keyboard" || section === "updates" ? 0 : undefined} onFocus={(event) => { if (event.target instanceof HTMLElement) event.target.scrollIntoView({ block: "nearest", inline: "nearest" }); }}>
      {section !== "connections" ? <PageHeader className="settings-heading" titleId="settings-title" title={page.label}
        description={page.description} /> : null}

      <div className="settings-stack">
        {section === "connections" ? <MarketplaceView bridge={bridge} loupists={loupists} currentLoupistId={currentLoupistId} initialEntryId={connectionsEntryId} onManageLoupists={() => currentLoupistId && onOpenLoupistConnections ? onOpenLoupistConnections(currentLoupistId) : onOpenLoupists?.()} onOpenModelSettings={onManageProviders} /> : null}
        {section === "loupists" ? <div className="settings-loupists"><div className="settings-loupist-actions"><Button onClick={onCreateLoupist}>{i18n.t("app:shell.newLoupist")}</Button><Button variant="outline" data-loupist-import onClick={onImportLoupist}>{i18n.t("settings:loupists.import")}</Button></div><ul>{loupists.map(loupist => <li key={loupist.id}><LoupistAvatar loupist={loupist} /><div><strong>{loupist.name}</strong><p>{loupist.philosophy}</p></div>{showSharing ? <Button variant="outline" data-share-loupist={loupist.id} onClick={() => setShareTarget(loupist)}>{i18n.t("sharing:share")}</Button> : null}<Button variant="outline" onClick={() => onOpenLoupistSettings?.(loupist.id)}>{i18n.t("settings:loupists.openSettings")}</Button><Button variant="ghost" onClick={() => onRemoveLoupist?.(loupist.id)}>{i18n.t("app:shell.removeLoupist")}</Button></li>)}</ul>
          {showSharing && shareTarget ? <ShareSheet bridge={bridge} loupistId={shareTarget.id} loupistName={shareTarget.name} scope={{ kind: "loupist" }} open onOpenChange={open => { if (!open) setShareTarget(null); }} /> : null}</div> : null}
        {section === "model" ? (
          <ModelSettingsPanel catalog={catalog} loadError={loadError} bridge={bridge} chatModelProfileId={appearanceConfiguration.snapshot?.effective.chat.model_profile_id}
            connectTarget={connectTarget} onConnectTargetConsumed={onConnectTargetConsumed} {...providerActions} />
        ) : null}

        {section === "appearance" ? (
          <><AppearanceSettings language={language} onLanguageChange={onLanguageChange} theme={theme} onThemeChange={onThemeChange} disabled={appearanceConfiguration.busy || !appearanceConfiguration.snapshot || Boolean(appearanceConfiguration.snapshot.diagnostics.length)} />{appearanceConfiguration.error ? <p role="alert" className="inline-error">{appearanceConfiguration.error}</p> : null}<ConfigurationDiagnostics snapshot={appearanceConfiguration.snapshot} /><div className="settings-footnote"><span>{i18n.t("settings:settingsView.savedAutomaticallyForAllLoupistsOnThisDevice")}</span><Button type="button" variant="ghost" size="sm" disabled={appearanceConfiguration.busy} onClick={() => notifyConfigurationChanged()}><RefreshCwIcon /> {i18n.t("settings:settingsView.reload")}</Button></div></>
        ) : null}

        {section === "keyboard" ? <KeyboardSettings /> : null}
        {section === "skills" ? <SkillsSettings bridge={bridge} loupists={loupists} scope={{ kind: "global" }} onDraftSkill={onDraftSkill} /> : null}

        {section === "agent" ? <><NoticeSettingsRow /><AgentSettings bridge={bridge} loupists={loupists} catalog={catalog} scope={{ kind: "global" }} /></> : null}

        {section === "updates" ? (
          <UpdateSettingsCard status={update.status} phase={update.phase} error={update.error} onCheck={update.checkForUpdates} onInstall={(offer_id) => update.installUpdate({ offer_id })} />
        ) : null}

      </div>
    </section>
  );
}
