import i18n from "@/i18n/index";
export type AppTab = "chat" | "map" | "lens" | "schedule" | "settings";
export type SettingsSection = "loupists" | "connections" | "model" | "agent" | "skills" | "appearance" | "keyboard" | "updates";
export const APP_TAB_LABELS: Record<AppTab, string> = {
  get chat() { return i18n.t("app:shell.tabs.chat"); },
  get map() { return i18n.t("app:shell.tabs.map"); },
  get lens() { return i18n.t("app:shell.tabs.lens"); },
  get schedule() { return i18n.t("app:shell.tabs.schedule"); },
  get settings() { return i18n.t("app:shell.settings"); },
};

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
