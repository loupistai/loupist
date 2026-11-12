import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { ApplicationSettings } from "./ApplicationSettings";

it("keeps Loupist in the menu bar through the host setting and lists what leaves this Mac", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const set = vi.spyOn(bridge, "briefingResidentSettingsSet");
  render(<ApplicationSettings bridge={bridge} />);
  const toggle = screen.getByRole("switch", { name: "Keep Loupist running in the menu bar for briefings and alerts" });
  await waitFor(() => expect(toggle).toBeEnabled());
  expect(screen.getByText("Loupist stays in the menu bar so briefings and Watching can run.")).toBeVisible();
  const before = toggle.getAttribute("aria-checked") === "true";
  await userEvent.setup().click(toggle);
  expect(set).toHaveBeenCalledWith({ keepRunning: !before });
  await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", String(!before)));
  // An explicit choice replaces the automatic note.
  expect(screen.queryByText("Loupist stays in the menu bar so briefings and Watching can run.")).toBeNull();
  expect(screen.getByRole("heading", { name: "About & privacy" })).toBeVisible();
  expect(screen.getByText(/^Loupist's makers receive nothing/)).toBeVisible();
  expect(screen.getByRole("button", { name: "Read again" })).toBeVisible();
});

it("says when the menu bar setting cannot be saved", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "briefingResidentSettingsSet").mockRejectedValue(new Error("nope"));
  render(<ApplicationSettings bridge={bridge} />);
  const toggle = screen.getByRole("switch");
  await waitFor(() => expect(toggle).toBeEnabled());
  await userEvent.setup().click(toggle);
  expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't change this setting. Try again.");
});
