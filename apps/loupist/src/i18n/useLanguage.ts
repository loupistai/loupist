import { useEffect, useState } from "react";
import type { useConfiguration } from "../app/useConfiguration";
import type { LanguagePreference } from "../contracts/config";
import i18n, { pseudoLocaleEnabled, useTranslation } from "./index";

export type { LanguagePreference } from "../contracts/config";
export type InterfaceLanguage = "en" | "ko" | "en-XA";
let previewLanguage: InterfaceLanguage | undefined;

export function setPreviewLanguage(language: InterfaceLanguage) {
  previewLanguage = language === "en-XA" && !pseudoLocaleEnabled ? "en" : language;
  void i18n.changeLanguage(previewLanguage);
  document.documentElement.lang = previewLanguage;
  document.documentElement.dataset.priceColors = /^(ko|zh|ja)(?:-|$)/i.test(previewLanguage) ? "red-up" : "green-up";
}

export function resolveLanguage(preference: LanguagePreference, languages: readonly string[] = navigator.languages): "en" | "ko" {
  if (preference !== "system") return preference;
  for (const language of languages) {
    if (/^ko(?:-|$)/i.test(language)) return "ko";
    if (/^en(?:-|$)/i.test(language)) return "en";
  }
  return "en";
}

/** Shares the global appearance reader with Theme to preserve save errors and revisions. */
export function useLanguage(configuration: ReturnType<typeof useConfiguration>) {
  const { i18n: active } = useTranslation();
  const [systemLanguages, setSystemLanguages] = useState(() => navigator.languages);
  const [pseudo, setPseudo] = useState(false);
  const preference = configuration.snapshot?.effective.appearance.language ?? "system";
  const resolved = previewLanguage ?? (pseudo && pseudoLocaleEnabled ? "en-XA" : resolveLanguage(preference, systemLanguages));
  useEffect(() => {
    const update = () => setSystemLanguages([...navigator.languages]);
    window.addEventListener("languagechange", update);
    return () => window.removeEventListener("languagechange", update);
  }, []);
  useEffect(() => {
    void active.changeLanguage(resolved);
    document.documentElement.lang = resolved;
    const priceLocale = preference === "system" && !previewLanguage ? systemLanguages[0] ?? resolved : resolved;
    document.documentElement.dataset.priceColors = /^(ko|zh|ja)(?:-|$)/i.test(priceLocale) ? "red-up" : "green-up";
  }, [active, resolved, preference, systemLanguages]);
  const setPreference = (language: LanguagePreference | "en-XA") => {
    if (language === "en-XA") {
      if (pseudoLocaleEnabled) {
        previewLanguage = undefined;
        setPseudo(true);
      }
      return;
    }
    previewLanguage = undefined;
    setPseudo(false);
    void configuration.update({ appearance: { language } });
  };
  return { preference: previewLanguage ?? (pseudo ? "en-XA" as const : preference), resolved, setPreference };
}
