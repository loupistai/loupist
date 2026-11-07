import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { MarketplaceView } from "./MarketplaceView";

it("filters connection readiness using the host’s verified state and combines it with search", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const snapshot = await bridge.marketplaceSnapshot();
  const entries = snapshot.catalog.entries;
  const ready = entries.find((entry) => snapshot.connectors.some((connector) => connector.entry_id === entry.id))!;
  const unavailable = entries.find((entry) => entry.id !== ready.id)!;
  snapshot.connectors = snapshot.connectors.map((connector) => ({ ...connector, readiness: connector.entry_id === ready.id ? "ready" : "needs_configuration" }));
  vi.spyOn(bridge, "marketplaceSnapshot").mockResolvedValue(snapshot);
  render(<MarketplaceView bridge={bridge} />);
  const user = userEvent.setup();
  expect(await screen.findByRole("heading", { name: ready.name, level: 4 })).toBeVisible();
  await user.click(screen.getByRole("radio", { name: "Ready to use" }));
  expect(screen.getByRole("heading", { name: ready.name, level: 4 })).toBeVisible();
  expect(screen.queryByRole("heading", { name: unavailable.name, level: 4 })).toBeNull();
  await user.type(screen.getByRole("searchbox", { name: "Search connections" }), "no-matching-connection");
  expect(screen.getByRole("heading", { name: "No matching connections" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Clear filters" }));
  expect(screen.getByRole("heading", { name: unavailable.name, level: 4 })).toBeVisible();
});

async function renderView(patch?: (snapshot: Awaited<ReturnType<MockLoupistBridge["marketplaceSnapshot"]>>) => void) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const snapshot = await bridge.marketplaceSnapshot();
  patch?.(snapshot);
  vi.spyOn(bridge, "marketplaceSnapshot").mockResolvedValue(snapshot);
  render(<MarketplaceView bridge={bridge} loupists={await bridge.loupistList()} currentLoupistId="loupist-quality" />);
  await screen.findByRole("heading", { name: "FRED", level: 4 });
  return { bridge, user: userEvent.setup() };
}
const cardOf = (name: string) => within(screen.getByRole("heading", { name, level: 4 }).closest("article")!);

describe("connection states", () => {
  it("calls an unconfigured optional source Not connected and counts only real problems under Needs attention", async () => {
    const { user } = await renderView((snapshot) => {
      const alpha = snapshot.connectors.find((connector) => connector.entry_id === "alpha-vantage.market-data")!;
      alpha.credentials = alpha.credentials.map((credential) => ({ ...credential, stored: true, pending: true, verification: "rejected", last_error: "The provider rejected these credentials." }));
    });
    expect(cardOf("FRED").getByText("Not connected")).toBeVisible();
    expect(cardOf("FRED").queryByText("Needs setup")).toBeNull();
    expect(cardOf("Alpha Vantage").getByText("Needs attention")).toBeVisible();
    await user.click(screen.getByRole("radio", { name: "Needs attention 1" }));
    expect(screen.getByRole("heading", { name: "Alpha Vantage", level: 4 })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "FRED", level: 4 })).toBeNull();
    expect(screen.getByText("1 of 24 connections")).toBeVisible();
  });

  it("shows no Needs attention count when nothing is wrong", async () => {
    await renderView();
    expect(screen.getByRole("radio", { name: "Needs attention" })).toBeVisible();
  });

  it("labels SEC EDGAR by what it asks for instead of No API key required", async () => {
    await renderView();
    expect(cardOf("SEC EDGAR").getByText("Contact email only")).toBeVisible();
    expect(cardOf("SEC EDGAR").queryByText("No API key required")).toBeNull();
    expect(cardOf("SEC EDGAR").getByText("Not connected")).toBeVisible();
  });
});

describe("connection list", () => {
  it("leads Market data with the keyless platform, ready entries next, then display names", async () => {
    await renderView();
    const group = screen.getByRole("heading", { name: /^Market data/, level: 3 }).closest("section")!;
    const names = within(group).getAllByRole("heading", { level: 4 }).map((heading) => heading.textContent!);
    expect(names[0]).toBe("Market data platform");
    const rest = names.slice(1);
    expect(rest).toEqual([...rest].sort((left, right) => left.localeCompare(right, "en", { sensitivity: "base" })));
    expect(rest.indexOf("U.S. Bureau of Labor Statistics")).toBeGreaterThan(rest.indexOf("Tiingo"));
  });

  it("searches only the text a card shows", async () => {
    const { user } = await renderView();
    const search = screen.getByRole("searchbox", { name: "Search connections" });
    await user.type(search, "openbb");
    expect(screen.getByRole("heading", { name: "No matching connections" })).toBeVisible();
    await user.clear(search);
    await user.type(search, "contact email");
    expect(screen.getByRole("heading", { name: "SEC EDGAR", level: 4 })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "FRED", level: 4 })).toBeNull();
  });

  it("puts Available connections above Your connections and hides the Loupist picker until a connection exists", async () => {
    await renderView();
    const available = screen.getByRole("heading", { name: "Available connections", level: 2 });
    const custom = screen.getByRole("heading", { name: "Your connections", level: 2 });
    expect(available.compareDocumentPosition(custom) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(screen.getByRole("button", { name: "+ Add a connection" })).toBeVisible();
    expect(screen.queryByRole("combobox", { name: "Loupist" })).toBeNull();
  });

  it("marks the loading state as a status", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "marketplaceSnapshot").mockReturnValue(new Promise(() => {}));
    render(<MarketplaceView bridge={bridge} />);
    expect(screen.getByRole("status", { name: "Loading connections" })).toBeVisible();
  });
});
