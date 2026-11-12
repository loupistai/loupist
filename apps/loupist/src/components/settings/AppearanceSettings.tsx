import { CheckIcon, LaptopIcon, LanguagesIcon, MoonIcon, SunIcon } from "lucide-react";
import { Card, CardContent } from "@/components/ui/card";
import type { ThemePreference } from "../../app/useTheme";
import type { LanguagePreference } from "../../i18n/useLanguage";
import { pseudoLocaleEnabled, useTranslation } from "../../i18n";

type Props = {
  theme: ThemePreference;
  onThemeChange: (theme: ThemePreference) => void;
  language: LanguagePreference | "en-XA";
  onLanguageChange: (language: LanguagePreference | "en-XA") => void;
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

export function AppearanceSettings({ theme, onThemeChange, language, onLanguageChange, disabled }: Props) {
  const { t } = useTranslation("settings");
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
      </CardContent>
    </Card>
  );
}
