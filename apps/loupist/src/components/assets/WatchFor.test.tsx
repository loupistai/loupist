import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { mockAssetId } from "../../bridge/mock/assets";
import { previewWatchSettings } from "../../preview/finance";
import type { LoupistSummary } from "../../types";
import type { WatchAsset } from "../../contracts/watch";
import { WatchFor } from "./WatchFor";

async function section() {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList() as LoupistSummary[];
  bridge.watch.install(loupist!.id, previewWatchSettings());
  const onOpenConnections = vi.fn();
  render(<WatchFor bridge={bridge} loupistId={loupist!.id} loupistName="Quality" assetId={mockAssetId("HANUL", "KRX")} name="Hanul Precision" onOpenConnections={onOpenConnections} />);
  const region = await screen.findByRole("region", { name: "Watch for" });
  return { bridge, loupist: loupist!, region, onOpenConnections, user: userEvent.setup() };
}

it("one switch per kind that applies; a switch sets that kind with the revision Rust gave", async () => {
  const { bridge, loupist, region, user } = await section();
  const switches = within(region).getAllByRole("switch");
  expect(switches.map(item => item.getAttribute("aria-label"))).toEqual([
    "New filings for Hanul Precision", "Results within 7 days for Hanul Precision", "Insider buying by several people for Hanul Precision",
    "Big holder crossing 5% for Hanul Precision", "Foreign and institution buying turns for Hanul Precision", "Dividend record dates for Hanul Precision",
  ]);
  const insiders = within(region).getByRole("switch", { name: "Insider buying by several people for Hanul Precision" });
  expect(insiders).toHaveAttribute("aria-checked", "false");
  await user.click(insiders);
  await waitFor(() => expect(bridge.watch.setCalls).toEqual([{ loupistId: loupist.id, assetId: mockAssetId("HANUL", "KRX"), kind: "insiders", enabled: true, expectedRevision: 2 }]));
  await waitFor(() => expect(within(region).getByRole("switch", { name: "Insider buying by several people for Hanul Precision" })).toHaveAttribute("aria-checked", "true"));
  expect(within(region).getByText(/^Checked .* · hourly while Quality is open$/)).toBeVisible();
});

it("a kind whose connection is off stays focusable, says which connection it needs and never toggles", async () => {
  const { bridge, region, onOpenConnections, user } = await section();
  const flows = within(region).getByRole("switch", { name: "Foreign and institution buying turns for Hanul Precision" });
  expect(flows).toHaveAttribute("aria-disabled", "true");
  expect(flows).not.toBeDisabled();
  expect(flows).toHaveAccessibleDescription("Needs Korea Investment · Turn on");
  await user.click(flows);
  expect(bridge.watch.setCalls).toHaveLength(0);
  await user.click(within(region).getByRole("button", { name: "Turn on" }));
  expect(onOpenConnections).toHaveBeenCalled();
});

it("a kind whose connection macOS locked says which connection is locked and leads to Try again", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList() as LoupistSummary[];
  const settings = previewWatchSettings();
  const lock = (item: WatchAsset): WatchAsset => ({ ...item, kinds: item.kinds.map(kind => kind.needsConnection ? { ...kind, needsConnection: { ...kind.needsConnection, locked: true } } : kind) });
  bridge.watch.install(loupist!.id, { ...settings, watched: settings.watched?.map(lock) });
  const onOpenConnections = vi.fn();
  render(<WatchFor bridge={bridge} loupistId={loupist!.id} loupistName="Quality" assetId={mockAssetId("HANUL", "KRX")} name="Hanul Precision" onOpenConnections={onOpenConnections} />);
  const region = await screen.findByRole("region", { name: "Watch for" });
  const flows = within(region).getByRole("switch", { name: "Foreign and institution buying turns for Hanul Precision" });
  expect(flows).toHaveAccessibleDescription("Korea Investment is locked until you allow access in macOS · Try again in Connections");
  // The other kinds keep working.
  expect(within(region).getByRole("switch", { name: "New filings for Hanul Precision" })).not.toHaveAttribute("aria-disabled");
  await userEvent.setup().click(within(region).getByRole("button", { name: "Try again in Connections" }));
  expect(onOpenConnections).toHaveBeenCalled();
});

it("an asset that is not followed has no Watch for section", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList() as LoupistSummary[];
  bridge.watch.install(loupist!.id, previewWatchSettings());
  const view = render(<WatchFor bridge={bridge} loupistId={loupist!.id} loupistName="Quality" assetId={mockAssetId("NOPE", "NYSE")} name="Nope" onOpenConnections={vi.fn()} />);
  await new Promise(resolve => setTimeout(resolve, 10));
  expect(view.container).toBeEmptyDOMElement();
});
