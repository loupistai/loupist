import i18n from "@/i18n/index";
import type { WorkspacePanelKind } from "./useWorkspacePanel";

/**
 * A page the stage shows. Map and Lens are panels beside Chat, not pages.
 * `following` is the Following page (what you follow, shared by every Loupist); it has
 * no ⌘ key and opens from its sidebar row and the palette.
 */
export type AppTab = "chat" | "following" | "schedule" | "settings";
export type SettingsSection = "loupists" | "connections" | "model" | "agent" | "skills" | "appearance" | "keyboard" | "updates";

type AppPage = {
  /** Names the page in its error boundary and loading status. */
  readonly label: string;
  /** The screen-reader location heading. Without one, the page is named by its open conversation. */
  readonly heading?: string;
};

export const APP_PAGES: Record<AppTab, AppPage> = {
  chat: { get label() { return i18n.t("app:shell.tabs.chat"); } },
  following: { get label() { return i18n.t("app:shell.tabs.following"); }, get heading() { return i18n.t("app:shell.tabs.following"); } },
  schedule: { get label() { return i18n.t("app:shell.tabs.schedule"); }, get heading() { return i18n.t("app:shell.taskHistory"); } },
  settings: { get label() { return i18n.t("app:shell.settings"); }, get heading() { return i18n.t("app:shell.settings"); } },
};

/** The ids the native View menu sends (`src-tauri/src/lib.rs`), in ⌘1–⌘4 order. */
export type AppDestinationId = "chat" | "map" | "lens" | "schedule";

type AppDestination = {
  readonly id: AppDestinationId;
  /** A page, or a panel kind that opens beside Chat. */
  readonly target: { readonly page: AppTab } | { readonly panel: Exclude<WorkspacePanelKind, "materials"> };
  readonly label: string;
  /** The command palette's Actions row. */
  readonly action: string;
};

/**
 * Every top-level destination, in key order: ⌘1–⌘4 in the header, the palette's open
 * rows, the native menu and Settings › Keyboard read this list. A new page adds its
 * row here and its page in `APP_PAGES`.
 */
export const APP_DESTINATIONS: readonly AppDestination[] = [
  { id: "chat", target: { page: "chat" }, get label() { return i18n.t("app:shell.tabs.chat"); }, get action() { return i18n.t("app:palette.actions.openChat"); } },
  { id: "map", target: { panel: "world" }, get label() { return i18n.t("app:shell.tabs.map"); }, get action() { return i18n.t("app:palette.actions.openMap"); } },
  { id: "lens", target: { panel: "lens" }, get label() { return i18n.t("app:shell.tabs.lens"); }, get action() { return i18n.t("app:shell.tabs.lens"); } },
  { id: "schedule", target: { page: "schedule" }, get label() { return i18n.t("app:shell.tabs.schedule"); }, get action() { return i18n.t("app:shell.taskHistory"); } },
];

export const isAppDestination = (value: string): value is AppDestinationId => APP_DESTINATIONS.some((item) => item.id === value);
/** ⌘1 opens the first destination. Another key names none. */
export const destinationForKey = (key: string) => (/^[1-9]$/.test(key) ? APP_DESTINATIONS[Number(key) - 1] : undefined);

type SettingsSectionDefinition = {
  id: SettingsSection;
  label: string;
  group: "research" | "application";
  keywords: string;
  description: string;
  caption?: string;
};

export const SETTINGS_SECTIONS: readonly SettingsSectionDefinition[] = [
  { id: "loupists", get label() { return i18n.t("app:shell.loupists"); }, group: "research", get keywords() { return i18n.t("settings:loupists.keywords"); }, get description() { return i18n.t("settings:loupists.description"); } },
  { id: "connections", get label() { return i18n.t("app:navigation.connections"); }, group: "research", get keywords() { return i18n.t("app:navigation.connectionsDataSourcesApiKeyDartKrxOpenbb"); }, get description() { return i18n.t("app:navigation.addDataSourcesAndChooseWhichLoupistsCan"); } },
  { id: "model", get label() { return i18n.t("app:navigation.modelsProviders"); }, group: "research", get keywords() { return i18n.t("app:navigation.accountsConnectApiKeySignInModels"); }, get description() { return i18n.t("app:navigation.connectYourAccountsAndChooseTheModelsAvailable"); }, get caption() { return i18n.t("app:navigation.modelsCaption"); } },
  { id: "agent", get label() { return i18n.t("app:navigation.chatPreferences"); }, group: "research", get keywords() { return i18n.t("app:navigation.agentDefaultsReasoningThinkingRetryTokensContextConfiguration"); }, get description() { return i18n.t("app:navigation.setModelAndMemoryDefaultsGloballyOrFor"); } },
  { id: "skills", get label() { return i18n.t("app:navigation.skills"); }, group: "research", get keywords() { return i18n.t("app:navigation.skillsCreatorInstallerInstructionsWorkflowsCreateInstallZip"); }, get description() { return i18n.t("app:navigation.addAndManageReusableWorkflowsForYourLoupists"); }, get caption() { return i18n.t("app:navigation.skillsCaption"); } },
  { id: "appearance", get label() { return i18n.t("app:navigation.appearance"); }, group: "application", get keywords() { return i18n.t("app:navigation.themeLightDarkSystemDisplay"); }, get description() { return i18n.t("app:navigation.makeYourWorkspaceFeelAtHome"); } },
  { id: "keyboard", get label() { return i18n.t("app:navigation.keyboard"); }, group: "application", get keywords() { return i18n.t("app:navigation.keyboardKeywords"); }, get description() { return i18n.t("app:navigation.keyboardDescription"); } },
  { id: "updates", get label() { return i18n.t("app:navigation.updates"); }, group: "application", get keywords() { return i18n.t("app:navigation.versionReleaseInstallDownload"); }, get description() { return i18n.t("app:navigation.keepLoupistUpToDate"); } },
];
