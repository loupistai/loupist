/**
 * Trading hours for the markets people follow, used by the header's market
 * status chip. Holidays and early closes come from the maintained exchange
 * calendar (`market-calendar.json`, shared with Rust's `market_calendar.rs`;
 * sources: nyse.com/markets/hours-calendars, nasdaqtrader.com holiday
 * calendar, global.krx.co.kr market holidays). Only years an exchange has
 * published are listed; any other date counts as a regular weekday session.
 */
import calendar from "../../market-calendar.json";

type Session = readonly [openMinute: number, closeMinute: number];
type MarketHours = { id: string; timeZone: string; sessions: readonly Session[] };
type MarketCalendar = { years: readonly number[]; closed: Readonly<Record<string, string>>; earlyClose: Readonly<Record<string, string>> };

const at = (hour: number, minute = 0) => hour * 60 + minute;

/** Exchanges by the display id the chip shows. Lunch breaks are separate sessions. */
const MARKETS: Record<string, MarketHours> = {
  KRX: { id: "KRX", timeZone: "Asia/Seoul", sessions: [[at(9), at(15, 30)]] },
  US: { id: "US", timeZone: "America/New_York", sessions: [[at(9, 30), at(16)]] },
  TSE: { id: "TSE", timeZone: "Asia/Tokyo", sessions: [[at(9), at(11, 30)], [at(12, 30), at(15, 30)]] },
  HKEX: { id: "HKEX", timeZone: "Asia/Hong_Kong", sessions: [[at(9, 30), at(12)], [at(13), at(16)]] },
  SSE: { id: "SSE", timeZone: "Asia/Shanghai", sessions: [[at(9, 30), at(11, 30)], [at(13), at(15)]] },
  LSE: { id: "LSE", timeZone: "Europe/London", sessions: [[at(8), at(16, 30)]] },
  XETRA: { id: "XETRA", timeZone: "Europe/Berlin", sessions: [[at(9), at(17, 30)]] },
  EURONEXT: { id: "EURONEXT", timeZone: "Europe/Paris", sessions: [[at(9), at(17, 30)]] },
  TSX: { id: "TSX", timeZone: "America/Toronto", sessions: [[at(9, 30), at(16)]] },
  ASX: { id: "ASX", timeZone: "Australia/Sydney", sessions: [[at(10), at(16)]] },
};

/** Published holidays and early closes by chip id (US: NYSE and Nasdaq; KRX). */
const CALENDARS: Record<string, MarketCalendar> = calendar.markets;

/** Codes people type after a symbol ("005930:KRX", "AAPL:NASDAQ") mapped to a market above, or to crypto. */
const ALIASES: Record<string, string> = {
  KRX: "KRX", KOSPI: "KRX", KOSDAQ: "KRX", KONEX: "KRX", KS: "KRX", KQ: "KRX",
  US: "US", NYSE: "US", NASDAQ: "US", AMEX: "US", NYSEARCA: "US", ARCA: "US", BATS: "US", CBOE: "US", NMS: "US",
  TSE: "TSE", JPX: "TSE", TYO: "TSE", T: "TSE",
  HKEX: "HKEX", HKG: "HKEX", HK: "HKEX", SEHK: "HKEX",
  SSE: "SSE", SHA: "SSE", SS: "SSE", SZSE: "SSE", SHE: "SSE", SZ: "SSE",
  LSE: "LSE", LON: "LSE", L: "LSE",
  XETRA: "XETRA", ETR: "XETRA", FRA: "XETRA", DE: "XETRA",
  EURONEXT: "EURONEXT", EPA: "EURONEXT", AMS: "EURONEXT", PA: "EURONEXT", AS: "EURONEXT",
  TSX: "TSX", TO: "TSX",
  ASX: "ASX", AX: "ASX",
  CRYPTO: "CRYPTO", UPBIT: "CRYPTO", BITHUMB: "CRYPTO", BINANCE: "CRYPTO", COINBASE: "CRYPTO", KRAKEN: "CRYPTO", BYBIT: "CRYPTO", OKX: "CRYPTO",
};

export type MarketState =
  | { market: "CRYPTO"; state: "always" }
  /** `earlyCloseAtMs`: today is a published early close and this session ends then. */
  | { market: string; state: "open"; closesInMinutes: number; earlyCloseAtMs?: number }
  | { market: string; state: "opensSoon"; opensInMinutes: number }
  /** `holiday`: today is a published exchange holiday. */
  | { market: string; state: "closed"; holiday?: true };

/** The market a followed asset trades on, or undefined for codes this chip does not know. */
export function marketFor(code: string): string | undefined {
  return ALIASES[code.trim().toUpperCase()];
}

/** Followed assets' markets, most followed first; unknown codes are left out. */
export function followedMarkets(entries: readonly { market: string }[]): string[] {
  const counts = new Map<string, number>();
  for (const entry of entries) {
    const market = marketFor(entry.market);
    if (market) counts.set(market, (counts.get(market) ?? 0) + 1);
  }
  return [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map(([market]) => market);
}

/** Markets with a published calendar and the last year it covers, for the chip's description. */
export function calendarCoverage(): { market: string; through: number }[] {
  return Object.entries(CALENDARS).map(([market, entry]) => ({ market, through: Math.max(...entry.years) }));
}

const DAY_MS = 86_400_000;

/** The local calendar day (as UTC midnight) and minute of the day at `nowMs` in `timeZone`. */
function localClock(nowMs: number, timeZone: string) {
  const parts = new Intl.DateTimeFormat("en-US", { timeZone, year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", hourCycle: "h23" }).formatToParts(nowMs);
  const part = (type: string) => Number(parts.find((item) => item.type === type)?.value ?? "");
  return { dayMs: Date.UTC(part("year"), part("month") - 1, part("day")), minute: part("hour") * 60 + part("minute") };
}

type Day = { sessions: Session[]; holiday: boolean; early: boolean };

/** A day's sessions: none on weekends and published holidays; an early close cuts the day short. */
function daySessions(hours: MarketHours, dayMs: number): Day {
  const day = new Date(dayMs);
  if (day.getUTCDay() === 0 || day.getUTCDay() === 6) return { sessions: [], holiday: false, early: false };
  const regular: Day = { sessions: [...hours.sessions], holiday: false, early: false };
  const entry = CALENDARS[hours.id];
  // A year the exchange has not published is a regular year: never a holiday.
  if (!entry || !entry.years.includes(day.getUTCFullYear())) return regular;
  const date = day.toISOString().slice(0, 10);
  if (entry.closed[date]) return { sessions: [], holiday: true, early: false };
  const early = entry.earlyClose[date];
  if (!early) return regular;
  const [hour, minute] = early.split(":").map(Number);
  const close = at(hour, minute);
  return {
    sessions: hours.sessions.filter(([open]) => open < close).map(([open, end]) => [open, Math.min(end, close)] as const),
    holiday: false,
    early: true,
  };
}

/** "Opens in" only within a few hours; overnight and weekends read as closed. */
const SOON_MINUTES = 3 * 60;

export function marketState(market: string, nowMs: number): MarketState | undefined {
  if (market === "CRYPTO") return { market, state: "always" };
  const hours = MARKETS[market];
  if (!hours) return undefined;
  const { dayMs, minute } = localClock(nowMs, hours.timeZone);
  const today = daySessions(hours, dayMs);
  const session = today.sessions.find(([open, close]) => minute >= open && minute < close);
  if (session) {
    const closesInMinutes = session[1] - minute;
    // The last session of a shortened day ends at the early close.
    return today.early && session === today.sessions[today.sessions.length - 1]
      ? { market, state: "open", closesInMinutes, earlyCloseAtMs: nowMs - (nowMs % 60_000) + closesInMinutes * 60_000 }
      : { market, state: "open", closesInMinutes };
  }
  if (today.holiday) return { market, state: "closed", holiday: true };
  // The next session start, walking forward over up to two weeks of local days.
  for (let offset = 0; offset < 15; offset += 1) {
    const day = offset === 0 ? today : daySessions(hours, dayMs + offset * DAY_MS);
    const next = day.sessions.find(([open]) => offset > 0 || open > minute);
    if (!next) continue;
    const minutes = offset * 24 * 60 + next[0] - minute;
    return minutes <= SOON_MINUTES ? { market, state: "opensSoon", opensInMinutes: minutes } : { market, state: "closed" };
  }
  return { market, state: "closed" };
}
