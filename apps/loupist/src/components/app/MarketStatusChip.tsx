import i18n, { useTranslation } from "@/i18n";
import { formatNumber, formatTime } from "@/i18n/format";
import { useEffect, useState } from "react";
import { calendarCoverage, marketState, type MarketState } from "../../chat/marketStatus";

const t = i18n.getFixedT(null, "app");
/** Market ids and state keys are chosen at runtime. */
const tDynamic = t as unknown as (key: string, options?: Record<string, unknown>) => string;
const MAX_MARKETS = 2;

const duration = (minutes: number) => minutes < 60
  ? t("marketStatus.minutes", { count: minutes, formattedCount: formatNumber(minutes) })
  : t("marketStatus.hours", { count: Math.round(minutes / 60), formattedCount: formatNumber(Math.round(minutes / 60)) });

export function marketStateText(state: MarketState) {
  if (state.state === "always") return t("marketStatus.always");
  const market = tDynamic(`marketStatus.market.${state.market}`);
  // An early close is said in the reader's own clock, like every other time in the app.
  if (state.state === "open") return state.earlyCloseAtMs === undefined ? t("marketStatus.open", { market }) : t("marketStatus.closesEarly", { market, time: formatTime(state.earlyCloseAtMs) });
  if (state.state === "opensSoon") return t("marketStatus.opensIn", { market, time: duration(state.opensInMinutes) });
  return t(state.holiday ? "marketStatus.holiday" : "marketStatus.closed", { market });
}

/** What the chip knows: the published calendars and how far they reach, then regular hours. */
const description = () => t("marketStatus.description", {
  covered: calendarCoverage().map(({ market, through }) => t("marketStatus.through", { market: tDynamic(`marketStatus.market.${market}`), year: String(through) })).join(", "),
});

/**
 * The header's market status for the markets the user follows: open, opening
 * soon, or closed, by trading hours and the exchanges' published holidays and
 * early closes. Crypto reads "24h". It is a status line, not a control, and it
 * hides when the header is narrow.
 */
export function MarketStatusChip({ markets }: { markets: readonly string[] }) {
  useTranslation("app");
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNowMs(Date.now()), 60_000);
    return () => window.clearInterval(timer);
  }, []);
  const states = markets.slice(0, MAX_MARKETS).flatMap((market) => {
    const state = marketState(market, nowMs);
    return state ? [state] : [];
  });
  if (!states.length) return null;
  return <span className="market-status" role="group" aria-label={t("marketStatus.label")} title={description()}>
    {states.map((state) => <span key={state.market} data-state={state.state}>{marketStateText(state)}</span>)}
    {markets.length > MAX_MARKETS ? <span data-state="more">{t("marketStatus.more", { formattedCount: formatNumber(markets.length - MAX_MARKETS) })}</span> : null}
  </span>;
}
