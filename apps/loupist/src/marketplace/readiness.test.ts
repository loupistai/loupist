import type { MarketplaceConnectorStatus, MarketplaceEntry } from "./types";
import { CONNECTION_STATES, connectionState, sortConnectionEntries } from "./readiness";

const connector = (patch: Partial<MarketplaceConnectorStatus> = {}): MarketplaceConnectorStatus => ({
  entry_id: "x", config: {}, config_state: "not_required", credentials: [], readiness: "needs_configuration", ...patch,
});
const credential = (patch: Partial<MarketplaceConnectorStatus["credentials"][number]>): MarketplaceConnectorStatus["credentials"][number] => ({
  entry_id: "x", credential_id: "api_key", stored: false, active: false, pending: false, verification: "never", verified_at: null, last_error: null, locked: false, ...patch,
});

describe("connection states", () => {
  it("treats an entry nobody set up as neutral, not as a problem", () => {
    for (const status of [undefined, connector(), connector({ config_state: "missing", credentials: [credential({})] })]) {
      expect(connectionState(status)).toBe("not_connected");
    }
    expect(CONNECTION_STATES.not_connected).toMatchObject({ label: "Not connected", tone: "neutral", needsAttention: false });
  });

  it("flags only a stored failing key, an unverified candidate or an invalid saved setting", () => {
    const stored = (patch: Partial<MarketplaceConnectorStatus["credentials"][number]>) => connector({ credentials: [credential({ stored: true, ...patch })] });
    expect(connectionState(stored({ verification: "rejected", last_error: "no" }))).toBe("needs_attention");
    expect(connectionState(stored({ pending: true }))).toBe("needs_attention");
    expect(connectionState(connector({ config_state: "missing", config: { contact_email: "not-an-email" } }))).toBe("needs_attention");
    expect(CONNECTION_STATES.needs_attention.needsAttention).toBe(true);
  });

  it("keeps ready and unavailable apart from attention", () => {
    expect(connectionState(connector({ readiness: "ready" }))).toBe("ready");
    expect(connectionState(connector({ readiness: "runtime_unavailable", credentials: [credential({ stored: true })] }))).toBe("unavailable");
    expect(Object.values(CONNECTION_STATES).filter((state) => state.needsAttention)).toHaveLength(2);
  });

  it("marks a key macOS refused as locked, on its own connection only", () => {
    const locked = connector({ credentials: [credential({ stored: true, locked: true })] });
    expect(connectionState(locked)).toBe("locked");
    expect(CONNECTION_STATES.locked).toMatchObject({ label: "Locked until you allow access in macOS", tone: "warning", needsAttention: true });
    expect(connectionState(connector({ credentials: [credential({ stored: true, active: true })], readiness: "ready" }))).toBe("ready");
  });
});

describe("connection ordering", () => {
  const entry = (id: string, name: string, setup = true) => ({ id, name, ...(setup ? { setup: { config_fields: [], credential_fields: [] } } : {}) }) as unknown as MarketplaceEntry;
  it("puts ready entries first, setup-free entries ahead of the rest, then sorts by display name", () => {
    const entries = [entry("z", "Zeta"), entry("b", "bravo"), entry("a", "Alpha"), entry("p", "Market data platform", false), entry("r", "Ready one")];
    const connectors = new Map(["r", "p"].map((id) => [id, connector({ entry_id: id, readiness: "ready" })]));
    expect(sortConnectionEntries(entries, connectors, (item) => item.name).map((item) => item.name))
      .toEqual(["Market data platform", "Ready one", "Alpha", "bravo", "Zeta"]);
  });
});
