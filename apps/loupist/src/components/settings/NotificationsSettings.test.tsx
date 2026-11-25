import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it } from "vitest";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { NotificationsSettings } from "./NotificationsSettings";

it("saves notification privacy and quiet hours separately from residence", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  render(<NotificationsSettings bridge={bridge} />);
  const details = screen.getByRole("switch", { name: "Show details in notifications" });
  await waitFor(() => expect(details).toBeEnabled());
  fireEvent.click(details);
  await waitFor(() => expect(details).toHaveAttribute("data-state", "unchecked"));
  const quiet = screen.getByRole("switch", { name: "Quiet hours" });
  fireEvent.click(quiet);
  await waitFor(() => expect(quiet).toHaveAttribute("data-state", "unchecked"));
  expect(await bridge.briefingResidentSettingsGet()).toMatchObject({
    keepRunning: true, notifications: { quietHours: null, dailyCap: 6, showDetails: false },
  });
});

it("inherits quiet notification defaults and saves changes by kind", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  render(<NotificationsSettings bridge={bridge} />);
  const briefing = screen.getByRole("combobox", { name: "Briefing" });
  await waitFor(() => expect(briefing).toBeEnabled());
  expect(briefing).toHaveValue("now");
  expect(screen.getByRole("combobox", { name: "Other scheduled work" })).toHaveValue("digest");
  const failures = screen.getByRole("combobox", { name: "Failed runs" });
  expect(failures).toHaveValue("digest");
  fireEvent.change(briefing, { target: { value: "off" } });
  await waitFor(() => expect(briefing).toHaveValue("off"));
  fireEvent.change(failures, { target: { value: "now" } });
  await waitFor(() => expect(failures).toHaveValue("now"));
  expect((await bridge.briefingResidentSettingsGet()).notifications?.defaults).toEqual({
    crossedLine: "now", filings: "now", briefing: "off", other: "digest", failures: "now",
  });
});

it("opens the operating system's notification settings only on request", async () => {
  const platform = vi.spyOn(navigator, "platform", "get").mockReturnValue("MacIntel");
  try {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const open = vi.spyOn(bridge, "notificationPermissionSettingsOpen");
    render(<NotificationsSettings bridge={bridge} />);
    expect(open).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Open System Settings" }));
    expect(open).toHaveBeenCalledOnce();
  } finally {
    platform.mockRestore();
  }
});
