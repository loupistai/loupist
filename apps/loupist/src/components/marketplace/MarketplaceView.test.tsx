import { render, screen } from "@testing-library/react";
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
  expect(await screen.findByRole("heading", { name: ready.name, level: 3 })).toBeVisible();
  await user.click(screen.getByRole("radio", { name: "Ready to use" }));
  expect(screen.getByRole("heading", { name: ready.name, level: 3 })).toBeVisible();
  expect(screen.queryByRole("heading", { name: unavailable.name, level: 3 })).toBeNull();
  await user.type(screen.getByRole("searchbox", { name: "Search connections" }), "no-matching-connection");
  expect(screen.getByRole("heading", { name: "No matching connections" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Clear filters" }));
  expect(screen.getByRole("heading", { name: unavailable.name, level: 3 })).toBeVisible();
});
