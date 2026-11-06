import type { ReactNode } from "react";
import { ChevronRightIcon } from "lucide-react";
import { useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
import { Disclosure } from "@/components/ui/disclosure";
import { providerKeyName } from "@/chat/providerNames";
import type { ModelCatalog, ModelProviderOption, ProviderStatusView } from "@/types";
import "@/styles/connect.css";

/** Sign-in rows, in the order the spec lists them (providers §2.1). */
const SIGN_IN = ["openai", "anthropic", "xai"] as const;
export const SIGN_IN_LABEL = { openai: "wizard.chatgpt", anthropic: "wizard.claude", xai: "wizard.xai" } as const;
const SIGN_IN_NOTE = { openai: "wizard.chatgptNote", anthropic: "wizard.claudeNote", xai: null } as const;
/** One API-key row each; Upstage Solar is a preset endpoint Rust saves, not a Pi provider. */
export const KEY_ROWS = ["anthropic", "openai", "google", "xai", "upstage", "openrouter"] as const;
const KEY_NOTE = { anthropic: null, openai: null, google: null, xai: null, upstage: "wizard.upstageNote", openrouter: "wizard.openrouterNote" } as const;
export const UPSTAGE_BASE_URL = "https://api.upstage.ai/v1";

export type ProviderWaysActions = {
  onSignIn: (provider: string) => void;
  onKey: (provider: string) => void;
  onUpstage: () => void;
  onLocal: () => void;
  onCustom: () => void;
};

const connectedStatus = (option: ModelProviderOption | undefined): ProviderStatusView | null =>
  option && (option.credential_source !== "missing" || option.status.state === "needs_reconnect") ? option.status : null;

/** Text plus an 8 px dot, never color alone; the dot is decorative for screen readers. */
export function ProviderStateLabel({ status }: { status: ProviderStatusView }) {
  const { t } = useTranslation("settings");
  const key = status.state === "needs_reconnect" ? "providers.needsReconnect" : status.state === "disconnected" ? "providers.disconnected" : "providers.connected";
  return <span className="connect-state" data-state={status.state}><span className="connect-state-dot" aria-hidden="true" />{t(key)}</span>;
}

function Row({ id, label, note, trailing, disabled, onClick }: { id: string; label: ReactNode; note?: ReactNode; trailing?: ReactNode; disabled?: boolean; onClick: () => void }) {
  return <button type="button" className="connect-row" data-connect-row={id} disabled={disabled} onClick={onClick}>
    <span className="connect-row-copy"><span className="connect-row-label">{label}</span>{note ? <span className="connect-row-note">{note}</span> : null}</span>
    {trailing}
    <ChevronRightIcon className="connect-row-chevron" aria-hidden="true" />
  </button>;
}

function GroupLabel({ children }: { children: ReactNode }) {
  return <p className="connect-group-label">{children}</p>;
}

/**
 * Every other way to connect, shared by onboarding's "Other ways to connect"
 * and Settings › Add a provider: sign-in, API key, this Mac and a custom
 * endpoint, then (Settings only) the rest of Pi's key providers collapsed.
 */
export function ProviderWays({ catalog, omitSignIn = [], includeMore, disabled, actions }: {
  catalog: ModelCatalog | null;
  /** Sign-in rows to leave out; onboarding shows them as its own buttons. */
  omitSignIn?: readonly string[];
  includeMore: boolean;
  disabled?: boolean;
  actions: ProviderWaysActions;
}) {
  const { t } = useTranslation("onboarding");
  const { t: ts } = useTranslation("settings");
  const providers = catalog?.providers ?? [];
  const find = (id: string) => providers.find(option => option.id === id);
  /** A connected row shows its state instead of "Recommended", only for the credential that row adds. */
  const trailing = (option: ModelProviderOption | undefined, signInRow: boolean) => {
    const status = connectedStatus(option);
    if (status && (status.credential_kind === "plan") === signInRow) return <ProviderStateLabel status={status} />;
    return !status && signInRow && option?.recommended ? <span className="connect-recommended">{ts("providers.connectedRecommended")}</span> : null;
  };
  const signIn = SIGN_IN.filter(id => !omitSignIn.includes(id) && find(id)?.sign_in_flow);
  const keys = KEY_ROWS.filter(id => id === "upstage" || find(id)?.api_key);
  const upstage = (catalog?.custom_providers ?? []).find(item => item.base_url.startsWith(UPSTAGE_BASE_URL));
  const more = includeMore ? providers.filter(option => option.group === "more" && option.api_key && !(KEY_ROWS as readonly string[]).includes(option.id)) : [];
  return <div className="connect-ways">
    {signIn.length ? <><GroupLabel>{t("wizard.groupSignIn")}</GroupLabel>
      {signIn.map(id => { const note = SIGN_IN_NOTE[id]; return <Row key={id} id={`sign-in-${id}`} label={t(SIGN_IN_LABEL[id])} note={note ? t(note) : undefined}
        trailing={trailing(find(id), true)} disabled={disabled} onClick={() => actions.onSignIn(id)} />; })}</> : null}
    <GroupLabel>{t("wizard.groupKeys")}</GroupLabel>
    {keys.map(id => { const note = KEY_NOTE[id]; return <Row key={id} id={`key-${id}`} label={providerKeyName(id, find(id)?.label ?? id)} note={note ? t(note) : undefined}
      trailing={id === "upstage" ? (upstage?.status ? <ProviderStateLabel status={upstage.status} /> : null) : trailing(find(id), false)}
      disabled={disabled} onClick={() => (id === "upstage" ? actions.onUpstage() : actions.onKey(id))} />; })}
    <GroupLabel>{t("wizard.groupMac")}</GroupLabel>
    <Row id="local" label={t("wizard.local")} note={t("wizard.localNote")} disabled={disabled} onClick={actions.onLocal} />
    <GroupLabel>{t("wizard.groupAdvanced")}</GroupLabel>
    <Row id="custom" label={t("wizard.custom")} disabled={disabled} onClick={actions.onCustom} />
    {more.length ? <Disclosure className="connect-more" title={ts("providers.more", { formattedCount: formatNumber(more.length) })}>
      {more.map(option => <Row key={option.id} id={`key-${option.id}`} label={option.label} trailing={trailing(option, false)} disabled={disabled} onClick={() => actions.onKey(option.id)} />)}
    </Disclosure> : null}
  </div>;
}
