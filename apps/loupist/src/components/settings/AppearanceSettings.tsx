import { CheckIcon, LaptopIcon, LanguagesIcon, MoonIcon, SunIcon } from "lucide-react";
import { Card, CardContent } from "@/components/ui/card";
import type { ThemePreference } from "../../app/useTheme";
import { autoPriceLocale, resolvePriceColors, type LanguagePreference, type PriceColorsPreference } from "../../i18n/useLanguage";
import { pseudoLocaleEnabled, useTranslation } from "../../i18n";
import { ChangePill } from "../market/ChangePill";
import type { DisplayFx } from "../../app/useDisplayFx";
import { DisplayCurrencySetting } from "./DisplayCurrencySetting";

type Props = {
  theme: ThemePreference;
  onThemeChange: (theme: ThemePreference) => void;
  language: LanguagePreference | "en-XA";
  onLanguageChange: (language: LanguagePreference | "en-XA") => void;
  priceColors?: PriceColorsPreference;
  onPriceColorsChange?: (priceColors: PriceColorsPreference) => void;
  /** The configured display currency; undefined is As listed. */
  displayCurrency?: string;
  /** A code saves it globally; null removes the key (As listed). */
  onDisplayCurrencyChange?: (code: string | null) => void;
  displayFx?: DisplayFx & { retry?: () => void };
  /** Opens the active Loupist's Connections; absent without one. */
  onConnectMarketData?: () => void;
  disabled?: boolean;
};
const themes = [
  { id: "light", icon: SunIcon },
  { id: "dark", icon: MoonIcon },
  { id: "system", icon: LaptopIcon },
] as const;
const languages: Array<{ id: LanguagePreference | "en-XA"; label: string }> = [
  { id: "system", label: "System" },
  { id: "en", label: "English" },
  { id: "ko", label: "한국어" },
  ...(pseudoLocaleEnabled ? [{ id: "en-XA" as const, label: "Pseudo (en-XA)" }] : []),
];
const priceChoices = [
  { id: "auto", label: "appearance.priceColors.auto" },
  { id: "red-up", label: "appearance.priceColors.redUp" },
  { id: "green-up", label: "appearance.priceColors.greenUp" },
] as const;
const OFF: DisplayFx = { status: "off" };

export function AppearanceSettings({ theme, onThemeChange, language, onLanguageChange, priceColors = "auto", onPriceColorsChange, displayCurrency, onDisplayCurrencyChange, displayFx = OFF, onConnectMarketData, disabled }: Props) {
  const { t, i18n } = useTranslation("settings");
  // Each choice shows its own colours; Automatic shows what it resolves to here.
  const automatic = resolvePriceColors("auto", autoPriceLocale(language, navigator.languages, i18n.language));
  return (
    <Card className="appearance-card">
      <CardContent className="space-y-4">
        <h3 className="text-sm font-semibold">{t("appearance.theme.label")}</h3>
        <div className="theme-grid" role="group" aria-label={t("appearance.theme.label")}>
          {themes.map((option) => {
            const ThemeIcon = option.icon;
            const label = t(`appearance.theme.${option.id}`);
            return (
              <button type="button" key={option.id} className="theme-card" data-active={theme === option.id}
                title={t("appearance.theme.optionLabel", { theme: label })} aria-pressed={theme === option.id}
                disabled={disabled} onClick={() => onThemeChange(option.id)}>
                <span className="theme-preview" data-theme-preview={option.id} aria-hidden="true"><i><em /><em /><em /></i><span><b /><b /><b /><em /></span></span>
                <span className="theme-option-label"><ThemeIcon /><span>
                  <strong>{label}</strong>{option.id === "system" ? <small>{t("appearance.theme.systemDescription")}</small> : null}
                </span>{theme === option.id && <CheckIcon />}</span>
              </button>
            );
          })}
        </div>
        <h3 className="text-sm font-semibold">{t("appearance.language.label")}</h3>
        <p className="text-sm text-muted-foreground">{t("appearance.language.description")}</p>
        <div className="theme-grid" role="group" aria-label={t("appearance.language.label")}>
          {languages.map((option) => (
            <button type="button" key={option.id} className="theme-card" data-active={language === option.id}
              aria-pressed={language === option.id} disabled={disabled} onClick={() => onLanguageChange(option.id)}>
              <span className="theme-option-label"><LanguagesIcon /><strong>{option.id === "system" ? t("appearance.language.system") : option.id === "en-XA" ? t("appearance.language.pseudo") : option.label}</strong>{language === option.id && <CheckIcon />}</span>
            </button>
          ))}
        </div>
        <h3 className="text-sm font-semibold">{t("appearance.priceColors.label")}</h3>
        <p className="text-sm text-muted-foreground">{t("appearance.priceColors.description")}</p>
        <div className="theme-grid" role="group" aria-label={t("appearance.priceColors.label")} data-price-colors-setting>
          {priceChoices.map((option) => (
            <button type="button" key={option.id} className="theme-card price-colors-card" data-active={priceColors === option.id}
              data-price-choice={option.id} aria-pressed={priceColors === option.id} disabled={disabled || !onPriceColorsChange}
              onClick={() => onPriceColorsChange?.(option.id)}>
              <span className="price-colors-sample" data-price-preview={option.id === "auto" ? automatic : option.id} aria-hidden="true">
                <ChangePill percent={0.0126} size="sm" /><ChangePill percent={-0.0201} size="sm" />
              </span>
              <span className="theme-option-label"><span>
                <strong>{t(option.label)}</strong>{option.id === "auto" ? <small>{t("appearance.priceColors.autoDescription")}</small> : null}
              </span>{priceColors === option.id && <CheckIcon />}</span>
            </button>
          ))}
        </div>
        <DisplayCurrencySetting value={displayCurrency} onChange={onDisplayCurrencyChange} fx={displayFx} onConnect={onConnectMarketData} disabled={disabled} />
      </CardContent>
    </Card>
  );
}
