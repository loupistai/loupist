import i18n from "@/i18n/index";
import { menuShortcutKey, nativeDestinations } from "./nativeMenu";

/** A page the stage shows. World and Lens are full pages; their chat tools stay in the side panel. */
export type AppTab = "chat" | "following" | "map" | "lens" | "schedule" | "settings";
export type SettingsSection = "model" | "loupists" | "connections" | "agent" | "skills" | "general" | "appearance" | "keyboard" | "updates";

type AppPage = {
  /** Names the page in its error boundary and loading status. */
  readonly label: string;
  /** The screen-reader location heading. Without one, the page is named by its open conversation. */
  readonly heading?: string;
};

export const APP_PAGES: Record<AppTab, AppPage> = {
  chat: { get label() { return i18n.t("app:shell.tabs.chat"); } },
  following: { get label() { return i18n.t("app:shell.tabs.following"); }, get heading() { return i18n.t("app:shell.tabs.following"); } },
  map: { get label() { return i18n.t("app:shell.tabs.map"); }, get heading() { return i18n.t("app:shell.tabs.map"); } },
  lens: { get label() { return i18n.t("app:shell.tabs.lens"); }, get heading() { return i18n.t("app:shell.tabs.lens"); } },
  schedule: { get label() { return i18n.t("app:shell.tabs.schedule"); }, get heading() { return i18n.t("app:shell.tabs.schedule"); } },
  settings: { get label() { return i18n.t("app:shell.settings"); }, get heading() { return i18n.t("app:shell.settings"); } },
};

/** Stable renderer IDs in the native View menu. */
export type AppDestinationId = Exclude<AppTab, "settings">;

type AppDestination = {
  readonly id: AppDestinationId;
  readonly page: AppTab;
  readonly label: string;
  /** The command palette's Actions row. */
  readonly action: string;
  readonly searchAliases?: readonly string[];
};

/** Sidebar places in the View menu's shortcut order. */
export const APP_DESTINATIONS: readonly AppDestination[] = nativeDestinations.map(row => {
  const id = row.id as AppDestinationId;
  return { id, page: id, searchAliases: id === "map" ? ["map", "지도"] : undefined, get label() { return APP_PAGES[id].label; }, get action() {
    switch (id) {
      case "chat": return i18n.t("app:palette.actions.openChat");
      case "map": return i18n.t("app:palette.actions.openMap");
      case "schedule": return i18n.t("app:shell.taskHistory");
      default: return APP_PAGES[id].label;
    }
  } };
});

export const isAppDestination = (value: string): value is AppDestinationId => APP_DESTINATIONS.some((item) => item.id === value);
/** ⌘1 opens the first destination. Another key names none. */
export const destinationForKey = (key: string) => APP_DESTINATIONS.find(item => menuShortcutKey(item.id) === key);

type SettingsSectionDefinition = {
  id: SettingsSection;
  label: string;
  group: "research" | "application";
  keywords: string;
  description: string;
  caption?: string;
};

const SETTINGS_SECTIONS: readonly SettingsSectionDefinition[] = [
  // A new user sets up a model first, then finds their Loupists and data connections.
  { id: "model", get label() { return i18n.t("app:navigation.modelsProviders"); }, group: "research", get keywords() { return i18n.t("app:navigation.accountsConnectApiKeySignInModels"); }, get description() { return i18n.t("app:navigation.connectYourAccountsAndChooseTheModelsAvailable"); }, get caption() { return i18n.t("app:navigation.modelsCaption"); } },
  { id: "loupists", get label() { return i18n.t("app:shell.loupists"); }, group: "research", get keywords() { return i18n.t("settings:loupists.keywords"); }, get description() { return i18n.t("settings:loupists.description"); } },
  { id: "connections", get label() { return i18n.t("app:navigation.connections"); }, group: "research", get keywords() { return i18n.t("app:navigation.connectionsDataSourcesApiKeyDartKrxOpenbb"); }, get description() { return i18n.t("app:navigation.addDataSourcesAndChooseWhichLoupistsCan"); } },
  { id: "agent", get label() { return i18n.t("app:navigation.chatPreferences"); }, group: "research", get keywords() { return i18n.t("app:navigation.agentDefaultsReasoningThinkingRetryTokensContextConfiguration"); }, get description() { return i18n.t("app:navigation.setModelAndMemoryDefaultsGloballyOrFor"); } },
  { id: "skills", get label() { return i18n.t("app:navigation.skills"); }, group: "research", get keywords() { return i18n.t("app:navigation.skillsCreatorInstallerInstructionsWorkflowsCreateInstallZip"); }, get description() { return i18n.t("app:navigation.addAndManageReusableWorkflowsForYourLoupists"); }, get caption() { return i18n.t("settings:skillsSettings.caption"); } },
  { id: "general", get label() { return i18n.t("settings:general.label"); }, group: "application", get keywords() { return i18n.t("settings:general.keywords"); }, get description() { return i18n.t("settings:general.description"); } },
  { id: "appearance", get label() { return i18n.t("app:navigation.appearance"); }, group: "application", get keywords() { return i18n.t("app:navigation.themeLightDarkSystemDisplay"); }, get description() { return i18n.t("app:navigation.makeYourWorkspaceFeelAtHome"); } },
  { id: "keyboard", get label() { return i18n.t("app:navigation.keyboard"); }, group: "application", get keywords() { return i18n.t("app:navigation.keyboardKeywords"); }, get description() { return i18n.t("app:navigation.keyboardDescription"); } },
  { id: "updates", get label() { return i18n.t("app:navigation.updates"); }, group: "application", get keywords() { return i18n.t("app:navigation.versionReleaseInstallDownload"); }, get description() { return i18n.t("app:navigation.keepLoupistUpToDate"); } },
];

export function settingsSections(userSkills: boolean) {
  return userSkills ? SETTINGS_SECTIONS : SETTINGS_SECTIONS.filter(({ id }) => id !== "skills");
}

export function availableSettingsSection(section: SettingsSection, userSkills: boolean): SettingsSection {
  return section === "skills" && !userSkills ? "general" : section;
}
