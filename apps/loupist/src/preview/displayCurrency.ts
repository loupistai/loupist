/**
 * Display-currency preview fixtures (currency design §5, slice 2): reference tables in
 * the host's shape (target units per one source unit, six significant digits). Everything
 * goes through the mock host's own controls: the global configuration and `fxDisplayControl.publish`.
 */
import { FX_DISPLAY_FIXTURES } from "../bridge/mock/fxDisplay";
import type { FxDisplay } from "../contracts/config";
import type { PreviewBridge } from "./fixtures";

type Table = Exclude<FxDisplay, { state: "unset" }>;
const DAY = 86_400_000;

/** The last weekday before today, as an ECB publication day. */
function lastPublication() {
  for (let back = 1; ; back += 1) {
    const at = new Date(Date.now() - back * DAY);
    if (at.getUTCDay() !== 0 && at.getUTCDay() !== 6) return at.toISOString().slice(0, 10);
  }
}

const RATES: Record<string, Record<string, number>> = {
  KRW: FX_DISPLAY_FIXTURES.ready.rates,
  CAD: { EUR: 1.6231, USD: 1.39452, CAD: 1 },
  JPY: { EUR: 179.42, USD: 154.321, JPY: 1 },
  USD: { EUR: 1.10141, KRW: 0.000749138, USD: 1 },
};

/** A ready table for one display currency, dated the last publication day unless given. */
export function previewRates(currency: keyof typeof RATES, extra: Partial<Extract<Table, { state: "ready" }>> = {}): Table {
  const date = extra.date ?? lastPublication();
  return { ...FX_DISPLAY_FIXTURES.ready, currency, rates: RATES[currency]!, date, revision: `ecb:${date}`, fetchedAt: `${date}T15:00:00+00:00`, ...extra };
}

/** Sets the global display currency to the table's and publishes the table, as a completed refresh would. */
export async function showInDisplayCurrency(bridge: PreviewBridge, table: FxDisplay | null, currency = table && table.state !== "unset" ? table.currency : undefined) {
  if (!currency) return;
  const current = await bridge.configRead({ scope: { kind: "global" } });
  await bridge.configUpdate({ scope: current.scope, expectedRevision: current.revision, set: { appearance: { display_currency: currency } }, unset: [] });
  if (table) bridge.fxDisplayControl.publish(table);
}
