import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { LegacyHoldingsStatus } from "../../contracts/legacyHoldings";
import { mainTab, openApp } from "../renderApp";

const EXPORTED = "Holdings are no longer part of Loupist. What was stored has been saved to a file on this computer, and the assets are still in Following.";
const FAILED = "Holdings are no longer part of Loupist. The stored data has not been deleted, but the copy could not be saved yet.";
const exported: LegacyHoldingsStatus = { state: "exported", dismissed: false, exportedAtMs: 1_794_916_800_000, counts: { wallets: 2, snapshots: 3, positions: 14 } };
const failed: LegacyHoldingsStatus = { state: "failed", dismissed: false, counts: { wallets: 2, snapshots: 3, positions: 14 } };
const pending: LegacyHoldingsStatus = { state: "pending", dismissed: false, counts: { wallets: 2, snapshots: 3, positions: 14 } };

const notice = () => screen.queryByRole("status", { name: "Holdings notice" });
const deferred = <T,>() => {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
};
const start = async (status: LegacyHoldingsStatus, prepare?: (bridge: MockLoupistBridge) => void) => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  bridge.legacyHoldings.setStatus(status);
  const read = vi.spyOn(bridge, "legacyHoldingsStatus");
  prepare?.(bridge);
  await openApp(bridge);
  await waitFor(() => expect(read).toHaveBeenCalled());
  await act(async () => { await Promise.resolve(); });
  return bridge;
};
const buttons = (section: HTMLElement) => within(section).getAllByRole("button").map(button => button.textContent);

describe("Loupist · holdings export notice", () => {
  it.each([
    ["nothing to export", { state: "none", dismissed: false }],
    ["an export still pending", pending],
    ["a dismissed export", { ...exported, dismissed: true }],
    ["a dismissed failure", { ...failed, dismissed: true }],
  ] as [string, LegacyHoldingsStatus][])("shows no line for %s", async (_name, status) => {
    await start(status);
    expect(notice()).toBeNull();
    expect(screen.queryByText(/Holdings are no longer part of Loupist/)).toBeNull();
  });

  it("says once that holdings were saved to a file, without a path, a name or a count", async () => {
    await start(exported);
    const line = await screen.findByRole("status", { name: "Holdings notice" });
    expect(within(line).getByText(EXPORTED)).toBeVisible();
    expect(buttons(line)).toEqual(["Show file", "Save a copy…", "Dismiss"]);
    expect(line.textContent).not.toMatch(/\d|\/|\\|\.json/);
  });

  it("shows the file and keeps Show file usable when it cannot", async () => {
    const bridge = await start(exported);
    const user = userEvent.setup();
    const reveal = vi.spyOn(bridge, "legacyHoldingsReveal");
    const line = await screen.findByRole("status", { name: "Holdings notice" });
    await user.click(within(line).getByRole("button", { name: "Show file" }));
    expect(reveal).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("alert")).toBeNull();
    reveal.mockRejectedValueOnce(new Error("/Users/someone/Library/holdings.json"));
    await user.click(within(line).getByRole("button", { name: "Show file" }));
    expect(await within(line).findByRole("alert")).toHaveTextContent("Couldn’t show the file.");
    expect(line.textContent).not.toMatch(/\/Users/);
    expect(within(line).getByRole("button", { name: "Show file" })).toBeEnabled();
    await user.click(within(line).getByRole("button", { name: "Show file" }));
    expect(reveal).toHaveBeenCalledTimes(3);
    await waitFor(() => expect(within(line).queryByRole("alert")).toBeNull());
  });

  it("saves a copy, says nothing when the panel is cancelled, and retries a failed save", async () => {
    const bridge = await start(exported);
    const user = userEvent.setup();
    const save = vi.spyOn(bridge, "legacyHoldingsSaveCopy");
    const line = await screen.findByRole("status", { name: "Holdings notice" });
    await user.click(within(line).getByRole("button", { name: "Save a copy…" }));
    expect(save).toHaveBeenCalledTimes(1);
    bridge.legacyHoldings.saveCopyResult = false;
    await user.click(within(line).getByRole("button", { name: "Save a copy…" }));
    expect(save).toHaveBeenCalledTimes(2);
    expect(within(line).queryByRole("alert")).toBeNull();
    save.mockRejectedValueOnce(new Error("disk full"));
    await user.click(within(line).getByRole("button", { name: "Save a copy…" }));
    const alert = await within(line).findByRole("alert");
    expect(alert).toHaveTextContent("Couldn’t save the copy.");
    await user.click(within(alert).getByRole("button", { name: "Try again" }));
    expect(save).toHaveBeenCalledTimes(4);
    await waitFor(() => expect(within(line).queryByRole("alert")).toBeNull());
  });

  it("does not start a second save while the panel is open", async () => {
    const bridge = await start(exported);
    const user = userEvent.setup();
    const panel = deferred<boolean>();
    const save = vi.spyOn(bridge, "legacyHoldingsSaveCopy").mockReturnValueOnce(panel.promise);
    const line = await screen.findByRole("status", { name: "Holdings notice" });
    await user.click(within(line).getByRole("button", { name: "Save a copy…" }));
    await user.click(within(line).getByRole("button", { name: "Save a copy…" }));
    expect(save).toHaveBeenCalledTimes(1);
    await act(async () => { panel.resolve(false); await panel.promise; });
  });

  it("runs one file action at a time across the shell line and Settings › General", async () => {
    const bridge = await start(exported);
    const user = userEvent.setup();
    await user.click(await mainTab(/Settings/));
    await user.click(await screen.findByRole("button", { name: /^General/ }));
    const row = await waitFor(() => { const found = document.querySelector<HTMLElement>("[data-legacy-holdings-settings]"); expect(found).not.toBeNull(); return found!; });
    const line = screen.getByRole("status", { name: "Holdings notice" });
    const fileButtons = () => [line, row].flatMap(surface => ["Show file", "Save a copy…"].map(name => within(surface).getByRole("button", { name })));
    const panel = deferred<boolean>();
    const save = vi.spyOn(bridge, "legacyHoldingsSaveCopy").mockReturnValueOnce(panel.promise).mockRejectedValueOnce(new Error("disk full"));
    const reveal = vi.spyOn(bridge, "legacyHoldingsReveal");
    await user.click(within(line).getByRole("button", { name: "Save a copy…" }));
    await user.click(within(row).getByRole("button", { name: "Save a copy…" }));
    await user.click(within(row).getByRole("button", { name: "Show file" }));
    expect(save).toHaveBeenCalledTimes(1);
    expect(reveal).not.toHaveBeenCalled();
    for (const button of fileButtons()) expect(button).toHaveAttribute("aria-disabled", "true");
    await act(async () => { panel.resolve(true); await panel.promise; });
    for (const button of fileButtons()) expect(button).not.toHaveAttribute("aria-disabled", "true");
    // A failed copy releases the guard too.
    await user.click(within(row).getByRole("button", { name: "Save a copy…" }));
    expect(await screen.findByText("Couldn’t save the copy.")).toBeVisible();
    expect(save).toHaveBeenCalledTimes(2);
    await user.click(within(line).getByRole("button", { name: "Show file" }));
    expect(reveal).toHaveBeenCalledTimes(1);
  });

  it("dismisses through the host at once and keeps focus in the page", async () => {
    const bridge = await start(exported);
    const user = userEvent.setup();
    const dismiss = vi.spyOn(bridge, "legacyHoldingsDismiss");
    const line = await screen.findByRole("status", { name: "Holdings notice" });
    within(line).getByRole("button", { name: "Dismiss" }).focus();
    await user.keyboard("{Enter}");
    expect(notice()).toBeNull();
    expect(dismiss).toHaveBeenCalledTimes(1);
    expect(document.activeElement).not.toBe(document.body);
    expect(document.activeElement).toBe(document.getElementById("current-location"));
    await act(async () => { await Promise.resolve(); });
    expect(notice()).toBeNull();
  });

  it("keeps a dismissed export in Settings › General", async () => {
    await start({ ...exported, dismissed: true });
    const user = userEvent.setup();
    await user.click(await mainTab(/Settings/));
    await user.click(await screen.findByRole("button", { name: /^General/ }));
    expect(await screen.findByText(/The stored holdings were saved to a file on/)).toBeVisible();
    expect(notice()).toBeNull();
  });

  it("says when the export has not succeeded and offers Try again and Dismiss", async () => {
    await start(failed);
    const line = await screen.findByRole("status", { name: "Holdings notice" });
    expect(within(line).getByText(FAILED)).toBeVisible();
    expect(buttons(line)).toEqual(["Try again", "Dismiss"]);
    expect(line.textContent).not.toMatch(/\d|\//);
  });

  it("keeps Try again busy until the run ends, then shows the exported line", async () => {
    const bridge = await start(failed);
    const user = userEvent.setup();
    const run = deferred<LegacyHoldingsStatus>();
    const retry = vi.spyOn(bridge, "legacyHoldingsRetry").mockReturnValueOnce(run.promise);
    const line = await screen.findByRole("status", { name: "Holdings notice" });
    const button = within(line).getByRole("button", { name: /Try again/ });
    button.focus();
    await user.keyboard(" ");
    expect(button).toBeDisabled();
    expect(within(button).getByRole("status")).toBeInTheDocument();
    await user.click(button);
    expect(retry).toHaveBeenCalledTimes(1);
    await act(async () => { run.resolve(exported); await run.promise; });
    const next = await screen.findByRole("status", { name: "Holdings notice" });
    expect(within(next).getByText(EXPORTED)).toBeVisible();
    await waitFor(() => expect(document.activeElement).toBe(within(next).getByRole("button", { name: "Show file" })));
  });

  it("keeps the failed line when Try again fails again", async () => {
    const bridge = await start(failed, mock => { mock.legacyHoldings.retryResult = { ...failed }; });
    const user = userEvent.setup();
    const retry = vi.spyOn(bridge, "legacyHoldingsRetry");
    const line = await screen.findByRole("status", { name: "Holdings notice" });
    await user.click(within(line).getByRole("button", { name: "Try again" }));
    expect(retry).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(within(line).getByRole("button", { name: "Try again" })).toBeEnabled());
    expect(within(line).getByText(FAILED)).toBeVisible();
  });

  it("dismisses a failure for the rest of this run", async () => {
    const bridge = await start(failed);
    const user = userEvent.setup();
    const dismiss = vi.spyOn(bridge, "legacyHoldingsDismiss");
    const line = await screen.findByRole("status", { name: "Holdings notice" });
    await user.click(within(line).getByRole("button", { name: "Dismiss" }));
    expect(dismiss).toHaveBeenCalledTimes(1);
    expect(notice()).toBeNull();
    expect(document.activeElement).not.toBe(document.body);
  });

  it.each([["exported", exported, EXPORTED], ["failed", failed, FAILED]] as const)("shows the %s line when a pending export ends after launch", async (_name, status, text) => {
    const bridge = await start(pending);
    expect(notice()).toBeNull();
    act(() => bridge.legacyHoldings.setStatus(status));
    const line = await screen.findByRole("status", { name: "Holdings notice" });
    expect(within(line).getByText(text)).toBeVisible();
  });

  it("lets a change during the first read win over that read", async () => {
    const first = deferred<LegacyHoldingsStatus>();
    const bridge = await start(pending, mock => { vi.spyOn(mock, "legacyHoldingsStatus").mockReturnValueOnce(first.promise); });
    expect(notice()).toBeNull();
    act(() => bridge.legacyHoldings.setStatus(exported));
    await act(async () => { first.resolve(pending); await first.promise; });
    expect(within(await screen.findByRole("status", { name: "Holdings notice" })).getByText(EXPORTED)).toBeVisible();
  });

  it("shows nothing when the status cannot be read, then follows the next change", async () => {
    const bridge = await start(pending, mock => { vi.spyOn(mock, "legacyHoldingsStatus").mockRejectedValue(new Error("no store")); });
    expect(notice()).toBeNull();
    act(() => bridge.legacyHoldings.setStatus(failed));
    expect(within(await screen.findByRole("status", { name: "Holdings notice" })).getByText(FAILED)).toBeVisible();
  });

  it("shows no line while Following is open", async () => {
    await start(exported);
    const user = userEvent.setup();
    await screen.findByRole("status", { name: "Holdings notice" });
    await user.click(within(document.querySelector<HTMLElement>('[data-sidebar-place="following"]')!).getByRole("button"));
    await screen.findByRole("heading", { name: "Following", level: 1 });
    expect(notice()).toBeNull();
    await user.click(screen.getByRole("button", { name: "Bot" }));
    expect(await screen.findByRole("status", { name: "Holdings notice" })).toBeVisible();
  });
});
