import userEvent from "@testing-library/user-event";
import { chooseSelectOption } from "@/test/selectOption";
import i18n, { resources } from "@/i18n";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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
  const user = userEvent.setup();
  const defaults = within(screen.getByRole("group", { name: "Defaults by kind" }));
  expect(defaults.getAllByRole("combobox")).toHaveLength(5);
  const briefing = defaults.getByRole("combobox", { name: "Briefing" });
  await waitFor(() => expect(briefing).toBeEnabled());
  expect(briefing).toHaveTextContent("Now");
  expect(screen.getByRole("combobox", { name: "Other scheduled work" })).toHaveTextContent("With the next update");
  const failures = screen.getByRole("combobox", { name: "Failed runs" });
  expect(failures).toHaveTextContent("With the next update");
  await chooseSelectOption(user, briefing, "In Bot only");
  await waitFor(() => expect(briefing).toHaveTextContent("In Bot only"));
  await chooseSelectOption(user, failures, "Now");
  await waitFor(() => expect(failures).toHaveTextContent("Now"));
  const cap = screen.getByRole("combobox", { name: "Daily cap" });
  await chooseSelectOption(user, cap, "4");
  await waitFor(() => expect(cap).toHaveTextContent("4"));
  expect((await bridge.briefingResidentSettingsGet()).notifications?.dailyCap).toBe(4);
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

it("edits quiet hours in the interface language and saves a 24-hour value", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const save = vi.spyOn(bridge, "briefingResidentSettingsSet");
  render(<NotificationsSettings bridge={bridge} />);
  const start = screen.getByRole("textbox", { name: "Start" });
  expect(start).toBeDisabled();
  await waitFor(() => expect(start).toBeEnabled());
  expect((start as HTMLInputElement).value).toMatch(/^10:00\sPM$/u);
  const user = userEvent.setup();
  fireEvent.change(start, { target: { value: "10:65 PM" } });
  start.focus();
  await user.keyboard("{Enter}");
  expect(save).not.toHaveBeenCalled();
  await act(async () => { await i18n.changeLanguage("ko"); });
  try {
    expect((start as HTMLInputElement).value).toMatch(/^오후\s10:00$/u);
    expect(start).toHaveAccessibleName(resources.ko.settings.notifications.start);
    expect(screen.getByRole("group", { name: resources.ko.settings.notifications.defaultTitle })).toBeVisible();
    fireEvent.change(start, { target: { value: "오후 9:25" } });
    await user.tab();
    await waitFor(() => expect((start as HTMLInputElement).value).toMatch(/^오후\s9:25$/u));
    expect(save).toHaveBeenCalledWith({ notifications: expect.objectContaining({ quietHours: { start: "21:25", end: "07:00" } }) });
  } finally {
    await act(async () => { await i18n.changeLanguage("en"); });
  }
});
