import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { mockAssetId } from "../../bridge/mock/assets";
import { previewWatchSettings } from "../../preview/finance";
import type { LoupistSummary } from "../../types";
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

it("an asset that is not followed has no Watch for section", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList() as LoupistSummary[];
  bridge.watch.install(loupist!.id, previewWatchSettings());
  const view = render(<WatchFor bridge={bridge} loupistId={loupist!.id} loupistName="Quality" assetId={mockAssetId("NOPE", "NYSE")} name="Nope" onOpenConnections={vi.fn()} />);
  await new Promise(resolve => setTimeout(resolve, 10));
  expect(view.container).toBeEmptyDOMElement();
});
