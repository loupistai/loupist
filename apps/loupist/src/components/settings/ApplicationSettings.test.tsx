import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { LegacyHoldingsStatus } from "../../contracts/legacyHoldings";
import i18n from "../../i18n";
import { ApplicationSettings } from "./ApplicationSettings";

it("keeps Loupist in the menu bar through the host setting and lists what leaves this Mac", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const set = vi.spyOn(bridge, "briefingResidentSettingsSet");
  render(<ApplicationSettings bridge={bridge} />);
  const toggle = screen.getByRole("switch", { name: "Keep running in the menu bar" });
  await waitFor(() => expect(toggle).toBeEnabled());
  expect(screen.getByText("On because a briefing is active. You can turn it off here.")).toBeVisible();
  const before = toggle.getAttribute("aria-checked") === "true";
  await userEvent.setup().click(toggle);
  expect(set).toHaveBeenCalledWith({ keepRunning: !before });
  await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", String(!before)));
  // An explicit choice replaces the automatic note.
  expect(screen.queryByText("On because a briefing is active. You can turn it off here.")).toBeNull();
  expect(screen.getByRole("heading", { name: "About & privacy" })).toBeVisible();
  expect(screen.getByText("Beta. This build is signed by Loupist and is not notarized by Apple.")).toBeVisible();
  // One sentence says what matters; where each kind of data goes opens on request.
  expect(screen.getByText(/^Your research stays on this Mac\./u)).toBeVisible();
  expect(screen.getByText("Loupist's makers receive nothing")).not.toBeVisible();
  await userEvent.setup().click(screen.getByText("Where each kind of data goes"));
  expect(screen.getByText("Loupist's makers receive nothing")).toBeVisible();
  // The app keeps no holdings, so no line says it keeps them or sends wallet addresses.
  expect(screen.getByText("names and tickers you follow or ask about.")).toBeVisible();
  expect(screen.queryByText(/holdings|wallet/i)).toBeNull();
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

describe("the holdings export row", () => {
  const exported: LegacyHoldingsStatus = { state: "exported", dismissed: false, exportedAtMs: 1_794_916_800_000, counts: { wallets: 2, snapshots: 3, positions: 14 } };
  const renderRow = (status: LegacyHoldingsStatus) => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    bridge.legacyHoldings.setStatus(status);
    const read = vi.spyOn(bridge, "legacyHoldingsStatus");
    render(<ApplicationSettings bridge={bridge} />);
    return { bridge, read };
  };
  const row = () => document.querySelector<HTMLElement>("[data-legacy-holdings-settings]");
  const settle = async (read: { mock: { calls: unknown[] } }) => {
    await waitFor(() => expect(read.mock.calls.length).toBeGreaterThan(0));
    await act(async () => { await Promise.resolve(); });
  };

  it.each([
    ["none", { state: "none", dismissed: false }],
    ["pending", { state: "pending", dismissed: false }],
    ["failed", { state: "failed", dismissed: false }],
    ["failed and dismissed", { state: "failed", dismissed: true }],
  ] as [string, LegacyHoldingsStatus][])("has no row while the export is %s", async (_name, status) => {
    const { read } = renderRow(status);
    await settle(read);
    expect(row()).toBeNull();
    expect(screen.queryByRole("button", { name: "Show file" })).toBeNull();
  });

  it("has no row when the status cannot be read", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    bridge.legacyHoldings.setStatus(exported);
    const read = vi.spyOn(bridge, "legacyHoldingsStatus").mockRejectedValue(new Error("no store"));
    render(<ApplicationSettings bridge={bridge} />);
    await settle(read);
    expect(row()).toBeNull();
  });

  it.each([false, true])("keeps the export date, Show file and Save a copy (dismissed: %s)", async (dismissed) => {
    const { bridge } = renderRow({ ...exported, dismissed });
    const user = userEvent.setup();
    const reveal = vi.spyOn(bridge, "legacyHoldingsReveal");
    const save = vi.spyOn(bridge, "legacyHoldingsSaveCopy");
    await waitFor(() => expect(row()).not.toBeNull());
    const date = new Intl.DateTimeFormat("en", { year: "numeric", month: "short", day: "numeric" }).format(exported.exportedAtMs);
    expect(row()).toHaveTextContent(`The stored holdings were saved to a file on ${date}.`);
    // The date only: no path, file name or count.
    expect(row()!.textContent!.replace(date, "")).not.toMatch(/\d|\/|\.json/);
    expect([...row()!.querySelectorAll("button")].map(button => button.textContent)).toEqual(["Show file", "Save a copy…"]);
    await user.click(screen.getByRole("button", { name: "Show file" }));
    expect(reveal).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Save a copy…" }));
    expect(save).toHaveBeenCalledTimes(1);
    bridge.legacyHoldings.saveCopyResult = false;
    await user.click(screen.getByRole("button", { name: "Save a copy…" }));
    expect(save).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("says when the file cannot be shown or the copy cannot be saved, and retries the copy", async () => {
    const { bridge } = renderRow(exported);
    const user = userEvent.setup();
    const reveal = vi.spyOn(bridge, "legacyHoldingsReveal").mockRejectedValueOnce(new Error("gone"));
    const save = vi.spyOn(bridge, "legacyHoldingsSaveCopy").mockRejectedValueOnce(new Error("disk full"));
    await user.click(await screen.findByRole("button", { name: "Show file" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn’t show the file.");
    expect(screen.getByRole("button", { name: "Show file" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Save a copy…" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Couldn’t save the copy."));
    await user.click(within(screen.getByRole("alert")).getByRole("button", { name: "Try again" }));
    expect(save).toHaveBeenCalledTimes(2);
    expect(reveal).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  });

  it("appears when a pending export ends while Settings is open", async () => {
    const { bridge, read } = renderRow({ state: "pending", dismissed: false });
    await settle(read);
    expect(row()).toBeNull();
    act(() => bridge.legacyHoldings.setStatus(exported));
    await waitFor(() => expect(row()).not.toBeNull());
  });

  it("dates the row in Korean", async () => {
    await act(async () => { await i18n.changeLanguage("ko"); });
    try {
      renderRow(exported);
      await waitFor(() => expect(row()).not.toBeNull());
      const date = new Intl.DateTimeFormat("ko", { year: "numeric", month: "short", day: "numeric" }).format(exported.exportedAtMs);
      expect(row()).toHaveTextContent(`저장되어 있던 보유 자산을 ${date}에 파일로 저장했습니다.`);
      expect([...row()!.querySelectorAll("button")].map(button => button.textContent)).toEqual(["파일 보기", "사본 저장…"]);
    } finally {
      await act(async () => { await i18n.changeLanguage("en"); });
    }
  });
});
