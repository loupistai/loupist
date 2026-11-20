import { useEffect, useState } from "react";
import type { useConfiguration } from "../app/useConfiguration";
import type { LanguagePreference, PriceColorsPreference } from "../contracts/config";
import i18n, { pseudoLocaleEnabled, useTranslation } from "./index";

export type { LanguagePreference, PriceColorsPreference } from "../contracts/config";
export type InterfaceLanguage = "en" | "ko" | "en-XA";
let previewLanguage: InterfaceLanguage | undefined;

/** The colour that means "up": a stored choice wins; auto takes the locale's convention (red-up in Korean, Chinese and Japanese). */
export function resolvePriceColors(preference: PriceColorsPreference, locale: string): "red-up" | "green-up" {
  if (preference !== "auto") return preference;
  return /^(ko|zh|ja)(?:-|$)/i.test(locale) ? "red-up" : "green-up";
}

/** The locale whose convention "auto" takes: the system's first language when the interface follows the system (a Japanese Mac in English still reads red as up), else the chosen one. */
export function autoPriceLocale(preference: LanguagePreference | InterfaceLanguage, systemLanguages: readonly string[], resolved: string) {
  return preference === "system" ? systemLanguages[0] ?? resolved : resolved;
}

export function setPreviewLanguage(language: InterfaceLanguage) {
  previewLanguage = language === "en-XA" && !pseudoLocaleEnabled ? "en" : language;
  void i18n.changeLanguage(previewLanguage);
  document.documentElement.lang = previewLanguage;
  document.documentElement.dataset.priceColors = resolvePriceColors("auto", previewLanguage);
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
  const priceColors = configuration.snapshot?.effective.appearance.price_colors ?? "auto";
  const resolved = previewLanguage ?? (pseudo && pseudoLocaleEnabled ? "en-XA" : resolveLanguage(preference, systemLanguages));
  useEffect(() => {
    const update = () => setSystemLanguages([...navigator.languages]);
    window.addEventListener("languagechange", update);
    return () => window.removeEventListener("languagechange", update);
  }, []);
  useEffect(() => {
    void active.changeLanguage(resolved);
    document.documentElement.lang = resolved;
    document.documentElement.dataset.priceColors = resolvePriceColors(priceColors, autoPriceLocale(previewLanguage ?? preference, systemLanguages, resolved));
  }, [active, resolved, preference, priceColors, systemLanguages]);
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
