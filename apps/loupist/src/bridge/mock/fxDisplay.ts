import type { FxChanged, FxDisplay, FxUnavailableReason } from "../../contracts/config";

const currency = "KRW";
const date = "2026-11-16";
const ready = {
  state: "ready",
  currency,
  revision: `ecb:${date}`,
  date,
  source: "European Central Bank euro foreign exchange reference rates",
  fetchedAt: "2026-11-17T16:00:00+00:00",
  rates: { EUR: 1470.23, KRW: 1, USD: 1334.87 },
} satisfies FxDisplay;

function unavailable(reason: FxUnavailableReason, lastDate?: string): FxDisplay {
  return { state: "unavailable", currency, reason, ...(lastDate ? { lastDate } : {}) };
}

export const FX_DISPLAY_FIXTURES = {
  unset: { state: "unset" },
  waiting: { state: "waiting", currency },
  ready,
  behindOffline: { ...ready, behind: "offline" },
  behindProviderFailed: { ...ready, behind: "provider_failed" },
  behindConnectionOff: { ...ready, behind: "connection_off" },
  connectionOff: unavailable("connection_off"),
  offline: unavailable("offline"),
  providerFailed: unavailable("provider_failed"),
  noRate: unavailable("no_rate_for_currency", date),
  tooOld: unavailable("too_old", "2026-11-08"),
} satisfies Record<string, FxDisplay>;

export class MockFxDisplay {
  private readonly fixtures = new Map<string, Exclude<FxDisplay, { state: "unset" }>>();
  private readonly handlers = new Set<(event: FxChanged) => void>();
  private reads = 0;

  get rateReads() { return this.reads; }

  read(currency: string, connected: boolean): FxDisplay {
    if (connected) this.reads++;
    const fixture = this.fixtures.get(currency);
    if (fixture?.state === "ready" && fixture.usable_until && Date.now() >= Date.parse(fixture.usable_until)) {
      return { state: "unavailable", currency, reason: "too_old", lastDate: fixture.date };
    }
    if (!connected) {
      if (fixture?.state === "ready") return { ...structuredClone(fixture), behind: "connection_off" };
      if (fixture?.state === "unavailable" && (fixture.reason === "too_old" || fixture.reason === "no_rate_for_currency")) {
        return structuredClone(fixture);
      }
      return { state: "unavailable", currency, reason: "connection_off" };
    }
    return fixture ? structuredClone(fixture) : { state: "waiting", currency };
  }

  publish(fixture: FxDisplay) {
    if (fixture.state === "unset") {
      this.fixtures.clear();
    } else {
      this.fixtures.set(fixture.currency, structuredClone(fixture));
    }
    const event: FxChanged = fixture.state === "ready"
      ? { state: fixture.state, revision: fixture.revision }
      : { state: fixture.state };
    this.handlers.forEach(handler => handler(event));
  }

  invalidate() {
    this.handlers.forEach(handler => handler({ state: "invalidated" }));
  }

  subscribe(handler: (event: FxChanged) => void, onListening?: () => void): () => void {
    this.handlers.add(handler);
    onListening?.();
    return () => { this.handlers.delete(handler); };
  }
}
