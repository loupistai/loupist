import i18n, { useTranslation } from "@/i18n";
import { Textarea } from "@/components/ui/textarea";
import { Label } from "@/components/ui/label";
import type { LoupistAvatarId, LoupistTemplateId, LoupistTone } from "@/contracts/loupist";
import type { LoupistSummary } from "@/types";
import { loupistPortraitId } from "@/lib/loupistAvatar";
import { workspaceFolderName } from "@/lib/loupistFolder";
import { LoupistToneField } from "./LoupistToneField";
import { LoupistIdentityFields } from "./LoupistIdentityFields";
import "@/styles/onboarding.css";

const loupistTemplates = [
  { id: "portfolio", avatar: "loupist-01" },
  { id: "semiconductors", avatar: "loupist-02" },
  { id: "dividends", avatar: "loupist-03" },
  { id: "technology", avatar: "loupist-04" },
  { id: "crypto", avatar: "loupist-05" },
  { id: "macro", avatar: "loupist-06" },
  { id: "commodities", avatar: "loupist-07" },
  { id: "custom", avatar: "loupist-10" },
] as const satisfies readonly { id: LoupistTemplateId; avatar: LoupistAvatarId }[];
/** `starters` names the template starters to follow once the Loupist exists. */
export type LoupistDraft = { template: LoupistTemplateId; name: string; focus: string; avatar: LoupistAvatarId; tone: LoupistTone; starters: readonly string[] };
type TemplateId = (typeof loupistTemplates)[number]["id"];

/**
 * What a class template suggests following (finance-crypto-and-templates.md §3.4).
 * A tapped chip is picked, and tapped again is dropped; whoever creates the Loupist
 * follows the picked ones in it, each by its English name alone, and the Loupist
 * finds how to read its price. Choosing another template drops the picks.
 */
export const TEMPLATE_STARTERS = {
  crypto: [
    { key: "BTC", name: "Bitcoin" },
    { key: "ETH", name: "Ethereum" },
    { key: "SOL", name: "Solana" },
    { key: "USDC", name: "USD Coin" },
  ],
  macro: [
    { key: "US10Y", name: "US 10-year Treasury" },
    { key: "US2Y", name: "US 2-year Treasury" },
    { key: "EURUSD", name: "EUR/USD" },
    { key: "USDKRW", name: "USD/KRW" },
  ],
  commodities: [
    { key: "GC", name: "Gold" },
    { key: "CL", name: "WTI crude" },
    { key: "HG", name: "Copper" },
  ],
} as const;

/**
 * A template fills the form with a persona name ("Chip Loupist"), not its card title,
 * so the Loupist reads as a colleague. Custom suggests no name; the user writes one.
 */
export function templateDraft(id: TemplateId, tone: LoupistTone = "balanced"): LoupistDraft {
  const avatar = loupistTemplates.find(template => template.id === id)!.avatar;
  return {
    template: id, avatar, tone, starters: [],
    name: id === "custom" ? "" : i18n.t(`onboarding:wizard.templates.${id}.suggestedName`),
    focus: id === "custom" ? "" : i18n.t(`onboarding:wizard.templates.${id}.focus`),
  };
}

/**
 * The name step for a Loupist that existed before onboarding (a development seed, or a
 * folder registered before the notice was accepted). Its stored name is kept unless the
 * host only copied the folder's name, which the interface never shows as a name; then
 * its template (or My portfolio) suggests one.
 */
export function existingDraft(loupist: LoupistSummary): LoupistDraft {
  const template = loupistTemplates.find(item => item.id === loupist.template)?.id ?? "portfolio";
  const suggested = templateDraft(template, loupist.tone);
  const name = loupist.name.trim();
  const folderOnly = !name || name === workspaceFolderName(loupist.workspace_path);
  return { ...suggested, name: folderOnly ? suggested.name : name, focus: loupist.philosophy, avatar: loupistPortraitId(loupist) };
}

export function LoupistTemplateFields({ draft, onChange, busy = false, idPrefix = "new-loupist", essentials = false }: {
  draft: LoupistDraft; onChange: (draft: LoupistDraft) => void; busy?: boolean; idPrefix?: string;
  /**
   * Onboarding's short form: templates and starters, plus Focus for Custom. The name and
   * face follow on their own step (Onboarding); the template sets the tone.
   */
  essentials?: boolean;
}) {
  const { t } = useTranslation("onboarding");
  const starters = draft.template in TEMPLATE_STARTERS ? TEMPLATE_STARTERS[draft.template as keyof typeof TEMPLATE_STARTERS] : null;
  return <div className="loupist-template-fields">
    <div className="loupist-template-grid" role="group" aria-label={t("wizard.templatesLabel")}>
      {loupistTemplates.map(({ id }) => <button type="button" key={id} className="loupist-template-card" data-testid="loupist-template-card"
        aria-pressed={draft.template === id} disabled={busy} onClick={() => onChange(templateDraft(id, draft.tone))}>
        <strong>{t(`wizard.templates.${id}.title`)}</strong><span title={t(`wizard.templates.${id}.focus`)}>{t(`wizard.templates.${id}.focus`)}</span>
      </button>)}
    </div>
    {starters ? <div className="loupist-template-starters" data-template-starters={draft.template}>
      <p>{t("wizard.starters")}</p>
      <div className="onboarding-chips">{starters.map(entry => {
        const on = draft.starters.includes(entry.name);
        const name = t(`wizard.starter.${entry.key}`);
        return <button type="button" key={entry.key} className="loupist-starter" aria-pressed={on} disabled={busy}
          aria-label={t("wizard.followStarter", { name })}
          onClick={() => onChange({ ...draft, starters: on ? draft.starters.filter(item => item !== entry.name) : [...draft.starters, entry.name] })}>{name}</button>;
      })}</div>
    </div> : null}
    {essentials ? <p className="loupist-template-later">{t("wizard.changeLater")}</p>
      : <LoupistIdentityFields name={draft.name} avatar={draft.avatar} focus={draft.focus} busy={busy} idPrefix={idPrefix}
        onChange={identity => onChange({ ...draft, ...identity })} />}
    {!essentials || draft.template === "custom" ? <div className="form-field"><Label htmlFor={`${idPrefix}-focus`}>{t("wizard.focus")}</Label>
      <Textarea id={`${idPrefix}-focus`} rows={2} value={draft.focus} maxLength={280} disabled={busy}
        placeholder={t("wizard.focusPlaceholder")} onChange={event => onChange({ ...draft, focus: event.target.value })} /></div> : null}
    {essentials ? null : <LoupistToneField name={`${idPrefix}-tone`} value={draft.tone} busy={busy} onChange={tone => onChange({ ...draft, tone })}
      legend={draft.name.trim() ? t("tone.title", { name: draft.name.trim() }) : t("tone.titleDefault")} />}
  </div>;
}
