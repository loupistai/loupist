import { useTranslation } from "@/i18n";
import { LOUPIST_TONES, type LoupistTone } from "@/contracts/loupist";
import "@/styles/onboarding.css";

/**
 * How a Loupist talks: one choice from three rows. Presentation only; the tone
 * never changes what evidence an answer needs.
 */
export function LoupistToneField({ value, onChange, legend, name, busy = false }: {
  value: LoupistTone; onChange: (tone: LoupistTone) => void; legend: string; name: string; busy?: boolean;
}) {
  const { t } = useTranslation("onboarding");
  return <fieldset className="loupist-tone-field" disabled={busy} data-loupist-tone>
    <legend>{legend}</legend>
    <div className="loupist-tone-options">
      {LOUPIST_TONES.map(tone => <label key={tone} className="loupist-tone-option" data-selected={value === tone || undefined}>
        <input type="radio" name={name} value={tone} checked={value === tone} onChange={() => onChange(tone)} />
        <span><strong>{t(`tone.${tone}.label`)}</strong><span>{t(`tone.${tone}.description`)}</span></span>
      </label>)}
      <p className="loupist-tone-note">{t("tone.note")}</p>
    </div>
  </fieldset>;
}
