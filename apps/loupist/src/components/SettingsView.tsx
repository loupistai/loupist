import { SettingsSearchPanel } from "./settings/SettingsSearchPanel";
import type { SettingResult } from "./settings/settingSearch";
import { ApplicationSettings } from "./settings/ApplicationSettings";
import { MoreHorizontalIcon } from "lucide-react";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "./ui/dropdown-menu";
import { MarketplaceView } from "./marketplace/MarketplaceView";
import { LoupistAvatar } from "./app/AppHeader";
import i18n, { useTranslation } from "../i18n";
import { SkillsSettings } from "./settings/SkillsSettings";
import { Button } from "./ui/button";
import { PageHeader } from "./ui/page-header";
import type { LanguagePreference } from "../i18n/useLanguage";
import type { ThemePreference } from "../app/useTheme";
import { availableSettingsSection, settingsSections, type SettingsSection } from "../app/navigation";
import { useExtensionAvailability } from "../app/extensionAvailability";
import { KeyboardSettings } from "./settings/KeyboardSettings";
import type { ModelCatalog } from "../types";
import { AppearanceSettings } from "./settings/AppearanceSettings";
import { ModelSettingsPanel, type ModelProviderActions } from "./settings/ModelSettingsPanel";
import type { ConnectSheetTarget } from "./settings/ConnectSheet";
import { UpdateSettingsCard } from "./settings/UpdateSettingsCard";
import type { useAppUpdate } from "./settings/useAppUpdate";
import type { ConfigScope, LoupistSummary, LoupistBridge } from "../types";
import { AgentSettings, ConfigurationDiagnostics } from "./settings/AgentSettings";
import type { useConfiguration } from "../app/useConfiguration";
import { useSharedDisplayFx } from "../app/useDisplayFx";
import { useState } from "react";
import { loupistIdentity } from "../lib/loupistFolder";
import { revealFocused } from "../lib/revealFocused";
import { sharingReleaseVisible } from "../sharing/releaseVisibility";
import { ShareSheet } from "./sharing/ShareSheet";

type Props = {
  bridge: LoupistBridge;
  loupists: LoupistSummary[];
  permissionNoticeRequest?: number;
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
  onConnectAccount?: () => void;
  /** A Chat default was saved here; the composer's earlier pick yields to it. */
  onChatDefaultSaved?: (scope: ConfigScope) => void;
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

export function SettingsView({ searching = false, target, onIndexed, ...props }: Props & {
  searching?: boolean; target?: SettingResult; onIndexed?: (section: SettingsSection, entries: SettingResult[]) => void;
}) {
  const { userSkills } = useExtensionAvailability();
  const section = availableSettingsSection(props.section, userSkills);
  return settingsSections(userSkills).filter(item => searching || item.id === section).map(item => {
    const active = item.id === section;
    return <SettingsSearchPanel key={item.id} section={item.id} active={active} searching={searching}
      target={active && target?.section === item.id ? target : undefined} onIndexed={onIndexed}>
      <SettingsPage {...props} section={item.id} indexing={!active} searchControls={searching || target?.section === item.id} connectionsEntryId={active ? props.connectionsEntryId : undefined} permissionNoticeRequest={active ? props.permissionNoticeRequest : undefined}
        connectTarget={active ? props.connectTarget : null} />
    </SettingsSearchPanel>;
  });
}

function SettingsPage({
  indexing, searchControls,
  bridge,
  loupists, connectionsEntryId, permissionNoticeRequest, onOpenLoupistSettings, onCreateLoupist, onImportLoupist, onRemoveLoupist, onOpenLoupists, onOpenLoupistConnections,
  currentLoupistId,
  onManageProviders, onConnectAccount, onChatDefaultSaved,
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
}: Props & { indexing: boolean; searchControls: boolean }) {
  useTranslation("settings");
  const displayFx = useSharedDisplayFx();
  const { userSkills } = useExtensionAvailability();
  const page = settingsSections(userSkills).find((item) => item.id === section)!;
  const [shareTarget, setShareTarget] = useState<LoupistSummary | null>(null);
  const showSharing = sharingReleaseVisible();
  return (
    // A focused control is revealed only when this column clips it, so focus never scrolls the window.
    <section className="settings-page" aria-labelledby={section === "connections" ? "marketplace-title" : "settings-title"} tabIndex={section === "keyboard" || section === "updates" ? 0 : undefined} onFocus={revealFocused}>
      {/* The window's page header names the section; the page opens with what it is for. */}
      {section !== "connections" ? <PageHeader className="settings-heading" titleId="settings-title" title={page.label} hideTitle
        description={page.description} /> : null}

      <div className="settings-stack">
        {section === "connections" ? <MarketplaceView bridge={bridge} loupists={loupists} currentLoupistId={currentLoupistId} initialEntryId={connectionsEntryId} permissionNoticeRequest={permissionNoticeRequest} onManageLoupists={() => currentLoupistId && onOpenLoupistConnections ? onOpenLoupistConnections(currentLoupistId) : onOpenLoupists?.()} onOpenModelSettings={onManageProviders} /> : null}
        {section === "loupists" ? <div className="settings-loupists"><div className="settings-loupist-actions"><Button onClick={onCreateLoupist}>{i18n.t("app:shell.newLoupist")}</Button><Button variant="outline" data-loupist-import onClick={onImportLoupist}>{i18n.t("settings:loupists.import")}</Button></div><ul>{loupists.map(loupist => {
            const { label, distinction } = loupistIdentity(loupist, loupists);
            return <li key={loupist.id}><LoupistAvatar loupist={loupist} /><div><strong>{label}</strong>{loupist.philosophy && distinction !== loupist.philosophy.trim() ? <p>{loupist.philosophy}</p> : null}</div><Button variant="outline" size="sm" aria-label={i18n.t("settings:loupists.settingsFor", { name: label })} onClick={() => onOpenLoupistSettings?.(loupist.id)}>{i18n.t("settings:loupists.settings")}</Button>
            <DropdownMenu><DropdownMenuTrigger asChild><Button variant="ghost" size="icon-sm" aria-label={i18n.t("settings:loupists.moreFor", { name: label })}><MoreHorizontalIcon /></Button></DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                {showSharing ? <DropdownMenuItem data-share-loupist={loupist.id} onSelect={() => setShareTarget(loupist)}>{i18n.t("sharing:share")}</DropdownMenuItem> : null}
                <DropdownMenuItem onSelect={() => onRemoveLoupist?.(loupist.id)}>{i18n.t("settings:loupists.removeEllipsis")}</DropdownMenuItem>
              </DropdownMenuContent></DropdownMenu></li>;
          })}</ul>
          {showSharing && shareTarget ? <ShareSheet bridge={bridge} loupistId={shareTarget.id} loupistName={shareTarget.name} scope={{ kind: "loupist" }} open onOpenChange={open => { if (!open) setShareTarget(null); }} /> : null}</div> : null}
        {section === "model" ? (
          <ModelSettingsPanel searchControls={searchControls} discoverModels={!indexing} catalog={catalog} loadError={loadError} bridge={bridge} chatModelProfileId={appearanceConfiguration.snapshot?.modelSelection?.model?.id}
            connectTarget={connectTarget} onConnectTargetConsumed={onConnectTargetConsumed} {...providerActions} />
        ) : null}

        {section === "appearance" ? (
          <><AppearanceSettings language={language} onLanguageChange={onLanguageChange} theme={theme} onThemeChange={onThemeChange} priceColors={appearanceConfiguration.snapshot?.effective.appearance.price_colors} onPriceColorsChange={(price_colors) => { void appearanceConfiguration.update({ appearance: { price_colors } }); }}
            displayCurrency={appearanceConfiguration.snapshot?.effective.appearance.display_currency || undefined} displayFx={displayFx}
            /* A code is set globally; As listed removes the key. */
            onDisplayCurrencyChange={(code) => { void (code ? appearanceConfiguration.update({ appearance: { display_currency: code } }) : appearanceConfiguration.update({}, [["appearance", "display_currency"]])); }}
            onConnectMarketData={currentLoupistId && onOpenLoupistConnections ? () => onOpenLoupistConnections(currentLoupistId) : undefined}
            disabled={appearanceConfiguration.busy || !appearanceConfiguration.snapshot || Boolean(appearanceConfiguration.snapshot.diagnostics.length)} />{appearanceConfiguration.error ? <p role="alert" className="inline-error">{appearanceConfiguration.error}</p> : null}<ConfigurationDiagnostics snapshot={appearanceConfiguration.snapshot} /><p className="settings-footnote"><span>{i18n.t("settings:settingsView.savedAutomaticallyForAllLoupistsOnThisDevice")}</span></p></>
        ) : null}

        {section === "keyboard" ? <KeyboardSettings /> : null}
        {section === "skills" ? <SkillsSettings bridge={bridge} loupists={loupists} scope={{ kind: "global" }} onDraftSkill={onDraftSkill} /> : null}

        {section === "agent" ? <AgentSettings onConnectAccount={onConnectAccount} onChatDefaultSaved={onChatDefaultSaved} bridge={bridge} loupists={loupists} catalog={catalog} scope={{ kind: "global" }} /> : null}
        {section === "general" ? <ApplicationSettings bridge={bridge} /> : null}

        {section === "updates" ? (
          <UpdateSettingsCard status={update.status} phase={update.phase} error={update.error} onCheck={update.checkForUpdates} onInstall={update.installUpdate} />
        ) : null}

      </div>
    </section>
  );
}
