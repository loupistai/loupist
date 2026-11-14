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
  it("leaves an unconfigured optional source unmarked and counts only real problems under Needs attention", async () => {
    const { user } = await renderView((snapshot) => {
      const alpha = snapshot.connectors.find((connector) => connector.entry_id === "alpha-vantage.market-data")!;
      alpha.credentials = alpha.credentials.map((credential) => ({ ...credential, stored: true, pending: true, verification: "rejected", last_error: "The provider rejected these credentials." }));
    });
    expect(cardOf("FRED").queryByText("Not connected")).toBeNull();
    expect(cardOf("FRED").queryByText("Needs setup")).toBeNull();
    expect(cardOf("Alpha Vantage").getByText("Needs attention")).toBeVisible();
    await user.click(screen.getByRole("radio", { name: "Needs attention 1" }));
    expect(screen.getByRole("heading", { name: "Alpha Vantage", level: 4 })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "FRED", level: 4 })).toBeNull();
    expect(screen.getByText("1 of 24 connections")).toBeVisible();
  });

  it("shows a key macOS refused as locked on its own card, explains the prompt before Try again, and asks again", async () => {
    const lock = (snapshot: Awaited<ReturnType<MockLoupistBridge["marketplaceSnapshot"]>>) => {
      const fred = snapshot.connectors.find((connector) => connector.entry_id === "fred.macro")!;
      fred.credentials = fred.credentials.map((credential) => ({ ...credential, stored: true, active: false, locked: true }));
      fred.readiness = "needs_configuration";
    };
    const { bridge, user } = await renderView(lock);
    expect(cardOf("FRED").getByText("Locked until you allow access in macOS")).toBeVisible();
    expect(cardOf("FRED").getByText(/macOS asks once for your login password .* not signed with an Apple Developer ID\. Choose “Always Allow”/)).toBeVisible();
    // Every other connection still lists and none of them is locked.
    expect(cardOf("SEC EDGAR").queryByText("Locked until you allow access in macOS")).toBeNull();
    expect(screen.getAllByText("Locked until you allow access in macOS")).toHaveLength(1);

    // Denied again: still locked, and it says so.
    const unlock = vi.spyOn(bridge, "marketplaceCredentialUnlock").mockImplementationOnce(async () => {
      const snapshot = await bridge.marketplaceSnapshot();
      return snapshot.connectors.find((connector) => connector.entry_id === "fred.macro")!.credentials;
    });
    await user.click(cardOf("FRED").getByRole("button", { name: "Try again FRED" }));
    expect(unlock).toHaveBeenCalledWith({ entry_id: "fred.macro" });
    expect(await cardOf("FRED").findByText("macOS did not allow access. Try again and enter your login password.")).toBeVisible();

    // Allowed: the card is no longer locked.
    unlock.mockResolvedValueOnce([]);
    vi.mocked(bridge.marketplaceSnapshot).mockImplementation(async () => {
      const snapshot = await new MockLoupistBridge({ delay_ms: 0 }).marketplaceSnapshot();
      return snapshot;
    });
    await user.click(cardOf("FRED").getByRole("button", { name: "Try again FRED" }));
    await vi.waitFor(() => expect(screen.queryByText("Locked until you allow access in macOS")).toBeNull());
  });

  it("shows no Needs attention count when nothing is wrong", async () => {
    await renderView();
    expect(screen.getByRole("radio", { name: "Needs attention" })).toBeVisible();
  });

  it("labels SEC EDGAR by what it asks for instead of No API key required", async () => {
    await renderView();
    expect(cardOf("SEC EDGAR").getByText("Contact email only")).toBeVisible();
    expect(cardOf("SEC EDGAR").queryByText("No API key required")).toBeNull();
    expect(cardOf("SEC EDGAR").queryByText("Not connected")).toBeNull();
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

  it("keeps your own connections in a closed advanced disclosure below Available connections, without a Loupist picker until one exists", async () => {
    await renderView();
    const available = screen.getByRole("heading", { name: "Available connections", level: 2 });
    const custom = document.querySelector<HTMLDetailsElement>("[data-custom-connections]")!;
    expect(custom.querySelector("summary")).toHaveTextContent("Add your own connection (advanced)");
    expect(custom.open).toBe(false);
    expect(available.compareDocumentPosition(custom) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    custom.querySelector("summary")!.click();
    expect(await screen.findByRole("button", { name: "Add a connection" })).toBeVisible();
    expect(screen.queryByRole("combobox", { name: "Loupist" })).toBeNull();
  });

  it("marks the loading state as a status", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "marketplaceSnapshot").mockReturnValue(new Promise(() => {}));
    render(<MarketplaceView bridge={bridge} />);
    expect(screen.getByRole("status", { name: "Loading connections" })).toBeVisible();
  });
});
