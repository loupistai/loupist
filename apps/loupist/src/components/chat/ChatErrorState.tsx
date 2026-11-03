import i18n, { useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
import { ExternalLinkIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { ChatFailureKind } from "../../types";

const t = i18n.getFixedT(null, "chat");

/** Seconds before the automatic retry after the first and second busy answer when the provider gave no hint; the third asks to try later. */
export const BUSY_RETRY_SECONDS = [20, 40] as const;

/** What an answer's error can ask the app to do about its account (providers spec §2.4). */
export type ProviderErrorAction = "reconnect" | "use_api_key" | "use_another" | "manage_usage" | "open_claude";

type Props = {
  kind: ChatFailureKind;
  /** The provider's display name, or a generic "your AI account". */
  provider: string;
  /** How many busy (rate-limited) answers in a row this Chat has had, including this one. */
  attempt: number;
  /** The provider's own wait, read and bounded by Rust; it replaces the fixed one. */
  retryAfterSeconds?: number;
  /** The fallback sentence for kinds without a shape of their own. */
  fallback: string;
  onRetry?: () => void;
  onProviderAction?: (action: ProviderErrorAction) => void;
};

/**
 * The run state of a failed answer (chat spec X11, providers §2.4). Rust sends
 * a typed kind, never provider text. Busy key or local accounts count down and
 * retry on their own; a plan at its limit never does, and each account problem
 * names one next step. The partial answer and its timeline stay above.
 */
export function ChatErrorState({ kind, provider, attempt, retryAfterSeconds, fallback, onRetry, onProviderAction }: Props) {
  useTranslation("chat");
  if (kind === "rate_limited" && onRetry && attempt <= BUSY_RETRY_SECONDS.length) {
    return <BusyCountdown seconds={retryAfterSeconds ?? BUSY_RETRY_SECONDS[attempt - 1] ?? BUSY_RETRY_SECONDS[0]} onRetry={onRetry} />;
  }
  const action = (name: ProviderErrorAction, label: string, primary = false, external = false) => onProviderAction
    ? <button type="button" data-primary={primary || undefined} data-error-action={name} onClick={() => onProviderAction(name)}>{label}{external ? <ExternalLinkIcon aria-hidden="true" /> : null}</button>
    : null;
  const retry = onRetry ? <button type="button" onClick={onRetry}>{t("chatMessageCard.tryAgain")}</button> : null;
  switch (kind) {
    case "plan_limit":
      return <>
        <p>{t("chatError.planLimit")}</p>
        <p className="message-run-hint">{t("chatError.planLimitHint")}</p>
        {action("manage_usage", t("chatError.manageUsage"), true, true)}{retry}{action("use_another", t("chatError.useAnother"))}
      </>;
    case "plan_not_eligible":
      return <><p>{t("chatError.notEligible")}</p>{action("use_api_key", t("chatError.useApiKey"), true)}</>;
    case "needs_reconnect":
      return <><p>{t("chatError.signInExpired", { provider })}</p>{action("reconnect", t("chatError.reconnect"), true)}</>;
    case "extra_usage_off":
      return <><p>{t("chatError.extraUsageOff")}</p>{action("open_claude", t("chatError.openClaude"), true, true)}{action("use_api_key", t("chatError.useApiKey"))}{retry}</>;
    case "out_of_credit":
      return <><p>{t("chatError.outOfCredit", { provider })}</p>{retry}</>;
    default:
      break;
  }
  const text = kind === "rate_limited" ? (onRetry ? t("chatError.tryLater", { provider }) : fallback)
    : kind === "offline" ? t("chatError.couldNotReach", { provider })
      : kind === "provider_unavailable" ? t("chatError.providerUnavailable", { provider })
        : kind === "context_too_long" ? t("chatError.contextTooLong")
          : fallback;
  // Trying the same long Chat again cannot fit it into the model.
  return <>
    <p>{text}</p>
    {kind === "context_too_long" ? null : retry}
  </>;
}

function BusyCountdown({ seconds, onRetry }: { seconds: number; onRetry: () => void }) {
  const [left, setLeft] = useState(seconds);
  const fired = useRef(false);
  useEffect(() => {
    const timer = window.setInterval(() => setLeft((value) => Math.max(0, value - 1)), 1000);
    return () => window.clearInterval(timer);
  }, []);
  useEffect(() => {
    if (left > 0 || fired.current) return;
    fired.current = true;
    onRetry();
  }, [left, onRetry]);
  const retryNow = () => {
    if (fired.current) return;
    fired.current = true;
    onRetry();
  };
  return <>
    <p aria-live="off">{t("chatError.busy", { seconds: formatNumber(left) })}</p>
    <button type="button" onClick={retryNow}>{t("chatError.retryNow")}</button>
  </>;
}
