import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import i18n from "../../i18n";
import { ApplicationSettings } from "./ApplicationSettings";

it("keeps Loupist in the menu bar through the host setting and lists what leaves this Mac", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const set = vi.spyOn(bridge, "briefingResidentSettingsSet");
  render(<ApplicationSettings bridge={bridge} />);
  const toggle = screen.getByRole("switch", { name: "Keep running in the menu bar" });
  await waitFor(() => expect(toggle).toBeEnabled());
  expect(screen.getByText("On because a briefing or Watching is active. You can turn it off here.")).toBeVisible();
  const before = toggle.getAttribute("aria-checked") === "true";
  await userEvent.setup().click(toggle);
  expect(set).toHaveBeenCalledWith({ keepRunning: !before });
  await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", String(!before)));
  // An explicit choice replaces the automatic note.
  expect(screen.queryByText("On because a briefing or Watching is active. You can turn it off here.")).toBeNull();
  expect(screen.getByRole("heading", { name: "About & privacy" })).toBeVisible();
  expect(screen.getByText("Beta. This build is signed by Loupist and is not notarized by Apple.")).toBeVisible();
  expect(screen.getByText("Loupist's makers receive nothing")).toBeVisible();
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

it("shows the Beta signing notice in Korean", async () => {
  await act(async () => { await i18n.changeLanguage("ko"); });
  try {
    render(<ApplicationSettings bridge={new MockLoupistBridge({ delay_ms: 0 })} />);
    expect(screen.getByText("베타 버전입니다. Loupist가 서명했으며 Apple 공증은 받지 않았습니다.")).toBeVisible();
    await waitFor(() => expect(screen.getByRole("switch")).toBeEnabled());
  } finally {
    await act(async () => { await i18n.changeLanguage("en"); });
  }
});
