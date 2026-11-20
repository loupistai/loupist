import { act, cleanup, render } from "@testing-library/react";
import i18n from "@/i18n";
import { formatDay } from "@/i18n/format";
import { FX_DISPLAY_FIXTURES as fixtures } from "../../bridge/mock/fxDisplay";
import type { DisplayFx } from "../../app/useDisplayFx";
import type { FxDisplay } from "../../types";
import { ConvertedAmount } from "./ConvertedAmount";

const SOURCE = fixtures.ready.source;
/** The rate's date as the line writes it: one unbreakable unit. */
const day = (date: string) => formatDay(date).replace(/ /gu, "\u00a0");
const table = (currency: string, rates: Record<string, number>, extra: Partial<Extract<FxDisplay, { state: "ready" }>> = {}): DisplayFx => ({
  status: "read", currency, display: { ...fixtures.ready, currency, rates: { ...rates, [currency]: 1 }, ...extra },
});
const won = table("KRW", fixtures.ready.rates);

function line(value: number, currency: string | undefined, fx: DisplayFx) {
  const view = render(<ConvertedAmount value={value} currency={currency} fx={fx} />);
  const element = view.container.querySelector<HTMLElement>("[data-converted-amount]");
  return { element, visible: element?.querySelector('[aria-hidden="true"]')?.textContent, spoken: element?.querySelector(".sr-only")?.textContent };
}

afterEach(async () => { cleanup(); await act(async () => { await i18n.changeLanguage("en"); }); });

it.each([
  ["CAD", table("CAD", { USD: 1.39452, EUR: 1.6231 }), "≈ CAD 227.10 · 1 USD = 1.39452 CAD", "≈ 227.10 CAD · 1 USD = 1.39452 CAD"],
  ["JPY", table("JPY", { USD: 154.321, EUR: 179.42 }), "≈ JPY 25,131 · 1 USD = 154.321 JPY", "≈ 25,131엔 · 1 USD = 154.321 JPY"],
  ["KRW", won, "≈ KRW 217,384 · 1 USD = 1,334.87 KRW", "≈ 217,384원 · 1 USD = 1,334.87 KRW"],
])("converts a US-dollar price into %s once, at the host's rate, by code or word and never by symbol", async (_code, fx, english, korean) => {
  const date = day("2026-11-16");
  const en = line(162.85, "USD", fx);
  expect(en.visible).toBe(`${english} · ${SOURCE}, ${date}`);
  expect(en.visible).not.toMatch(/[$¥₩]/u);
  expect(en.spoken).toBe(`About ${english.slice(2).split(" · ")[0]}, at ${english.split(" · ")[1]}, ${SOURCE}, ${date}`);
  cleanup();
  await act(async () => { await i18n.changeLanguage("ko"); });
  const ko = line(162.85, "USD", fx);
  expect(ko.visible).toBe(`${korean} · ${SOURCE}, ${day("2026-11-16")}`);
  expect(ko.visible).not.toMatch(/[$¥₩]/u);
  expect(ko.spoken).toMatch(/^약 .+ 기준$/u);
});

it("multiplies by target units per one listing unit, never the reciprocal, and keeps every digit of a small rate", () => {
  // A yen listing shown in won: 1 JPY = 9.0123 KRW, so 1,000 JPY is about 9,012 KRW.
  expect(line(1000, "JPY", table("KRW", { JPY: 9.0123 })).visible).toMatch(/^≈ KRW 9,012 · 1 JPY = 9.0123 KRW · /u);
  cleanup();
  // A won listing shown in dollars: the rate keeps its six significant digits.
  expect(line(39_920, "KRW", table("USD", { KRW: 0.000749138 })).visible).toMatch(/^≈ USD 29.91 · 1 KRW = 0.000749138 USD · /u);
  cleanup();
  // The unrounded price is multiplied once: 0.125 × 3 = 0.375 → 0.38, not 0.13 × 3.
  expect(line(0.125, "USD", table("CAD", { USD: 3 })).visible).toMatch(/^≈ CAD 0.38 · /u);
});

it.each([
  // Exact decimal products on a half unit, which binary multiplication puts just below it.
  ["a zero-decimal target", 0.7, "USD", table("KRW", { USD: 1355 }), /^≈ KRW 949 · /u],
  ["a two-decimal target", 1.15, "USD", table("CAD", { USD: 1.5 }), /^≈ CAD 1.73 · /u],
  ["a large amount", 1_234_567.89, "USD", table("KRW", { USD: 1334.87 }), /^≈ KRW 1,647,987,639 · /u],
  ["a small rate", 1_250_000, "IDR", table("USD", { IDR: 0.0000614 }), /^≈ USD 76.75 · /u],
])("rounds the exact decimal product once, half away from zero, for %s", (_name, value, currency, fx, expected) => {
  expect(line(value, currency, fx).visible).toMatch(expected);
});

it("shows the table's own date when it is behind", () => {
  const behind = table("KRW", fixtures.ready.rates, { behind: "offline", date: "2026-11-13" });
  expect(line(162.85, "USD", behind).visible).toMatch(new RegExp(`${SOURCE}, ${day("2026-11-13")}$`, "u"));
});

it.each<[string, number, string | undefined, DisplayFx]>([
  ["the setting is unset", 162.85, "USD", { status: "off" }],
  ["no Loupist is open", 162.85, "USD", { status: "no_loupist", currency: "KRW" }],
  ["the first read is in flight", 162.85, "USD", { status: "reading", currency: "KRW" }],
  ["the read failed", 162.85, "USD", { status: "failed", currency: "KRW" }],
  ["the host is waiting", 162.85, "USD", { status: "read", currency: "KRW", display: fixtures.waiting as Extract<FxDisplay, { state: "waiting" }> }],
  ["the table is too old", 162.85, "USD", { status: "read", currency: "KRW", display: fixtures.tooOld as Extract<FxDisplay, { state: "unavailable" }> }],
  ["market data is off with no table", 162.85, "USD", { status: "read", currency: "KRW", display: fixtures.connectionOff as Extract<FxDisplay, { state: "unavailable" }> }],
  ["the listing is already in the display currency", 112_400, "KRW", won],
  ["the listing currency is unknown", 162.85, undefined, won],
  ["the listing currency is not a code", 162.85, "usd", won],
  ["the table has no rate for it", 162.85, "TWD", won],
  ["the rate is zero", 162.85, "USD", table("KRW", { USD: 0 })],
  ["the rate is negative", 162.85, "USD", table("KRW", { USD: -1 })],
  ["the rate is not finite", 162.85, "USD", table("KRW", { USD: Number.NaN })],
  ["the price is not finite", Number.POSITIVE_INFINITY, "USD", won],
])("adds nothing when %s", (_name, value, currency, fx) => {
  expect(line(value, currency, fx).element).toBeNull();
});
