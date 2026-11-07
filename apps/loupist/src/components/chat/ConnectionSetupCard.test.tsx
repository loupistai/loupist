import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { ConnectionSetupCard } from "./ConnectionSetupCard";

const user = () => userEvent.setup({ skipHover: true });

function renderCard(bridge: MockLoupistBridge, entryId: string, onAskAgain?: () => void) {
  return render(<ConnectionSetupCard bridge={bridge} loupistId="loupist-quality" entryId={entryId} onAskAgain={onAskAgain} />);
}

it("offers setup for a connection without its key, opens the Connections dialog, and refreshes when it closes", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const snapshot = vi.spyOn(bridge, "marketplaceSnapshot");
  renderCard(bridge, "fred.macro");
  const card = await screen.findByRole("region", { name: "Set up FRED" });
  expect(await within(card).findByText("FRED needs its setup before this Loupist can use it.")).toBeVisible();
  expect(card).toHaveAttribute("data-state", "needs_setup");
  await user().click(within(card).getByRole("button", { name: "Set up" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByLabelText("FRED API key")).toBeVisible();

  // The key is entered in the dialog, never in Chat; once verified, the card reads the new state.
  await user().type(within(dialog).getByLabelText("FRED API key"), "test-fred-key-0123");
  await user().click(within(dialog).getByRole("button", { name: "Save & verify" }));
  await within(dialog).findByText("Verification successful");
  const reads = snapshot.mock.calls.length;
  await user().click(within(dialog).getByRole("button", { name: "Done" }));
  expect(await within(card).findByText("FRED is ready. It works from your next message.")).toBeVisible();
  expect(snapshot.mock.calls.length).toBeGreaterThan(reads);
  expect(card).toHaveAttribute("data-state", "ready");
});

it("turns a set-up connection on for this Loupist and then offers to ask again", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await bridge.loupistCapabilityDisable({ loupist_id: "loupist-quality", entry_id: "world-bank.indicators" });
  const enable = vi.spyOn(bridge, "loupistCapabilityEnable");
  const onAskAgain = vi.fn();
  renderCard(bridge, "world-bank.indicators", onAskAgain);
  const card = await screen.findByRole("region", { name: "Set up World Bank Indicators" });
  expect(await within(card).findByText("World Bank Indicators is set up but off for this Loupist.")).toBeVisible();
  await user().click(within(card).getByRole("button", { name: "Use in this Loupist" }));
  expect(enable).toHaveBeenCalledWith({ loupist_id: "loupist-quality", entry_id: "world-bank.indicators" });
  expect(await within(card).findByText("World Bank Indicators is ready. It works from your next message.")).toBeVisible();
  await user().click(within(card).getByRole("button", { name: "Ask again" }));
  expect(onAskAgain).toHaveBeenCalledTimes(1);
});

it("says when the connection is gone or its runtime is missing, and reports a failed enable", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  renderCard(bridge, "gone.entry");
  const gone = await screen.findByRole("region", { name: "Set up gone.entry" });
  expect(await within(gone).findByText("gone.entry is not available in this app.")).toBeVisible();
  expect(gone).toHaveAttribute("data-state", "unavailable");

  await bridge.loupistCapabilityDisable({ loupist_id: "loupist-quality", entry_id: "world-bank.indicators" });
  vi.spyOn(bridge, "loupistCapabilityEnable").mockRejectedValue(new Error("Loupist is busy"));
  renderCard(bridge, "world-bank.indicators");
  const card = await screen.findByRole("region", { name: "Set up World Bank Indicators" });
  await user().click(await within(card).findByRole("button", { name: "Use in this Loupist" }));
  expect(await within(card).findByRole("alert")).toHaveTextContent("Could not change this connection. Try again.");
  expect(card).toHaveAttribute("data-state", "off_here");
});
