import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { pseudoResources } from "./pseudo";
import { koreanParticles } from "./koreanParticles";
import enCommon from "./locales/en/common.json";
import enApp from "./locales/en/app.json";
import enChat from "./locales/en/chat.json";
import enKnowledge from "./locales/en/knowledge.json";
import enSettings from "./locales/en/settings.json";
import enConnections from "./locales/en/connections.json";
import enBriefings from "./locales/en/briefings.json";
import enOnboarding from "./locales/en/onboarding.json";
import enAssets from "./locales/en/assets.json";
import koCommon from "./locales/ko/common.json";
import koApp from "./locales/ko/app.json";
import koChat from "./locales/ko/chat.json";
import koKnowledge from "./locales/ko/knowledge.json";
import koSettings from "./locales/ko/settings.json";
import koConnections from "./locales/ko/connections.json";
import koBriefings from "./locales/ko/briefings.json";
import koOnboarding from "./locales/ko/onboarding.json";
import koAssets from "./locales/ko/assets.json";
import enSharing from "./locales/en/sharing.json";
import koSharing from "./locales/ko/sharing.json";

export { useTranslation, Trans } from "react-i18next";
export const resources = {
  en: { common: enCommon, app: enApp, chat: enChat, knowledge: enKnowledge, settings: enSettings, connections: enConnections, briefings: enBriefings, onboarding: enOnboarding, assets: enAssets, sharing: enSharing },
  ko: { common: koCommon, app: koApp, chat: koChat, knowledge: koKnowledge, settings: koSettings, connections: koConnections, briefings: koBriefings, onboarding: koOnboarding, assets: koAssets, sharing: koSharing },
};
const namespaces = Object.keys(resources.en) as Array<keyof typeof resources.en>;
export const pseudoLocaleEnabled = import.meta.env.DEV || (typeof location !== "undefined" && location.pathname.endsWith("/preview.html"));

// Korean strings write both particle forms after a value ("{{name}}이(가)"); the post-processor picks one.
void i18n.use(initReactI18next).use(koreanParticles).init({
  resources: { ...resources, ...(pseudoLocaleEnabled ? { "en-XA": pseudoResources(resources.en) } : {}) },
  lng: "en",
  fallbackLng: "en",
  supportedLngs: ["en", "ko", ...(pseudoLocaleEnabled ? ["en-XA"] : [])],
  nonExplicitSupportedLngs: true,
  ns: namespaces,
  defaultNS: "common",
  initAsync: false,
  returnNull: false,
  interpolation: { escapeValue: false },
  postProcess: ["koreanParticles"],
  react: { useSuspense: false },
});
export default i18n;
