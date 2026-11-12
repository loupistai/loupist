import i18n, { useTranslation } from "@/i18n";
import { useState } from "react";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Label } from "@/components/ui/label";
import type { LoupistAvatarId, LoupistTemplateId, LoupistTone } from "@/contracts/loupist";
import type { WatchlistAddRequest } from "@/contracts/userContext";
import { LoupistToneField } from "./LoupistToneField";
import { LoupistAvatarPicker } from "../app/LoupistAvatar";
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
export type LoupistDraft = { template: LoupistTemplateId; name: string; focus: string; avatar: LoupistAvatarId; tone: LoupistTone };
type TemplateId = (typeof loupistTemplates)[number]["id"];

/**
 * What a class template suggests following (finance-crypto-and-templates.md §3.4).
 * Nothing is added until the user taps a chip; names are stored in English.
 */
export const TEMPLATE_STARTERS = {
  crypto: [
    { key: "BTC", symbol: "BTC", market: "CRYPTO", name: "Bitcoin" },
    { key: "ETH", symbol: "ETH", market: "CRYPTO", name: "Ethereum" },
    { key: "SOL", symbol: "SOL", market: "CRYPTO", name: "Solana" },
    { key: "USDC", symbol: "USDC", market: "CRYPTO", name: "USD Coin" },
  ],
  macro: [
    { key: "US10Y", symbol: "US10Y", market: "RATES", name: "US 10-year Treasury" },
    { key: "US2Y", symbol: "US2Y", market: "RATES", name: "US 2-year Treasury" },
    { key: "EURUSD", symbol: "EURUSD", market: "FX", name: "EUR/USD" },
    { key: "USDKRW", symbol: "USDKRW", market: "FX", name: "USD/KRW" },
  ],
  commodities: [
    { key: "GC", symbol: "GC", market: "COMEX", name: "Gold" },
    { key: "CL", symbol: "CL", market: "NYMEX", name: "WTI crude" },
    { key: "HG", symbol: "HG", market: "COMEX", name: "Copper" },
  ],
} as const;

/**
 * A template fills the form with a persona name ("Chip Loupist"), not its card title,
 * so the Loupist reads as a colleague. Custom suggests no name; the user writes one.
 */
export function templateDraft(id: TemplateId, tone: LoupistTone = "balanced"): LoupistDraft {
  const avatar = loupistTemplates.find(template => template.id === id)!.avatar;
  return {
    template: id, avatar, tone,
    name: id === "custom" ? "" : i18n.t(`onboarding:wizard.templates.${id}.suggestedName`),
    focus: id === "custom" ? "" : i18n.t(`onboarding:wizard.templates.${id}.focus`),
  };
}

export function LoupistTemplateFields({ draft, onChange, busy = false, idPrefix = "new-loupist", onFollow, essentials = false }: {
  draft: LoupistDraft; onChange: (draft: LoupistDraft) => void; busy?: boolean; idPrefix?: string;
  /**
   * Onboarding's short form: templates, starters and Name, plus Focus for Custom. The
   * template sets the avatar and tone; Loupist settings changes them later.
   */
  essentials?: boolean;
  /** Adds a starter to the watchlist, which every Loupist shares; without it no starters are offered. */
  onFollow?: (entry: WatchlistAddRequest) => Promise<void>;
}) {
  const { t } = useTranslation("onboarding");
  const [adding, setAdding] = useState<string | null>(null);
  const [followed, setFollowed] = useState<readonly string[]>([]);
  const [followError, setFollowError] = useState(false);
  const starters = draft.template in TEMPLATE_STARTERS ? TEMPLATE_STARTERS[draft.template as keyof typeof TEMPLATE_STARTERS] : null;
  const follow = async (entry: (typeof TEMPLATE_STARTERS)[keyof typeof TEMPLATE_STARTERS][number]) => {
    if (!onFollow || adding) return;
    setAdding(entry.key);
    setFollowError(false);
    try {
      await onFollow({ symbol: entry.symbol, market: entry.market, displayName: entry.name });
      setFollowed(current => [...current, entry.key]);
    } catch { setFollowError(true); }
    finally { setAdding(null); }
  };
  return <div className="loupist-template-fields">
    <div className="loupist-template-grid" role="group" aria-label={t("wizard.templatesLabel")}>
      {loupistTemplates.map(({ id }) => <button type="button" key={id} className="loupist-template-card" data-testid="loupist-template-card"
        aria-pressed={draft.template === id} disabled={busy} onClick={() => onChange(templateDraft(id, draft.tone))}>
        <strong>{t(`wizard.templates.${id}.title`)}</strong><span title={t(`wizard.templates.${id}.focus`)}>{t(`wizard.templates.${id}.focus`)}</span>
      </button>)}
    </div>
    {onFollow && starters ? <div className="loupist-template-starters" data-template-starters={draft.template}>
      <p>{t("wizard.starters")}</p>
      <div className="onboarding-chips">{starters.map(entry => {
        const on = followed.includes(entry.key);
        const name = t(`wizard.starter.${entry.key}`);
        return <button type="button" key={entry.key} className="loupist-starter" aria-pressed={on} disabled={busy || adding !== null}
          aria-label={t("wizard.followStarter", { name })} onClick={() => { if (!on) void follow(entry); }}>{name}</button>;
      })}</div>
      {followError ? <p className="inline-error" role="alert">{t("wizard.followFailed")}</p> : null}
    </div> : null}
    <div className="form-field"><Label htmlFor={`${idPrefix}-name`}>{t("wizard.name")}</Label>
      <Input id={`${idPrefix}-name`} value={draft.name} maxLength={60} disabled={busy}
        onChange={event => onChange({ ...draft, name: event.target.value })} /></div>
    {essentials ? <p className="loupist-template-later">{t("wizard.changeLater")}</p>
      : <LoupistAvatarPicker loupist={draft} disabled={busy} onChange={avatar => onChange({ ...draft, avatar })} />}
    {!essentials || draft.template === "custom" ? <div className="form-field"><Label htmlFor={`${idPrefix}-focus`}>{t("wizard.focus")}</Label>
      <Textarea id={`${idPrefix}-focus`} rows={2} value={draft.focus} maxLength={280} disabled={busy}
        placeholder={t("wizard.focusPlaceholder")} onChange={event => onChange({ ...draft, focus: event.target.value })} /></div> : null}
    {essentials ? null : <LoupistToneField name={`${idPrefix}-tone`} value={draft.tone} busy={busy} onChange={tone => onChange({ ...draft, tone })}
      legend={draft.name.trim() ? t("tone.title", { name: draft.name.trim() }) : t("tone.titleDefault")} />}
  </div>;
}
