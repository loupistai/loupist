import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { UpdateState } from "../../types";
import { mainTab, openApp } from "../renderApp";

const OFFER_ID = "d7fb1bc8-0678-42f5-a20c-2ec5dc953728";
const current: UpdateState = {
  supported: true, current_version: "0.0.1", phase: "idle", offer: null, downloaded_bytes: 0, total_bytes: null,
  last_checked_at_ms: 1_786_233_600_000, next_auto_check_at_ms: 1_786_320_000_000, error: null, blockers: [],
};
const available: UpdateState = {
  ...current,
  offer: { offer_id: OFFER_ID, version: "1.1.0", notes: "Adds signed updates and improves Memory history.", published_at: "2026-08-09T00:00:00Z" },
};
const openUpdates = async (status: UpdateState) => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const statusSpy = vi.spyOn(bridge, "updateStatus").mockResolvedValue(status);
  const check = vi.spyOn(bridge, "updateCheck").mockResolvedValue(current);
  await openApp(bridge);
  await waitFor(() => expect(statusSpy).toHaveBeenCalled());
  expect(check).not.toHaveBeenCalled();
  await user.click(await mainTab(/Settings/));
  await user.click(await screen.findByRole("button", { name: /^Updates/ }));
  return { user, bridge, check };
};

describe("Loupist · Updates", () => {
  it("loads native status and checks manually from Settings", async () => {
    const { user, check } = await openUpdates(current);
    await user.click(await screen.findByRole("button", { name: "Check for updates" }));
    await waitFor(() => expect(check).toHaveBeenCalledTimes(1));
    expect(await screen.findByText("Loupist is up to date")).toBeVisible();
  });

  it("shows availability without auto-installing and submits only the opaque offer on intent", async () => {
    const { user, bridge } = await openUpdates(available);
    const install = vi.spyOn(bridge, "updateInstall").mockResolvedValue({ outcome: "cancelled", blockers: [] });
    expect((await screen.findAllByLabelText("Update available"))[0]).toBeVisible();
    expect(await screen.findByText("Loupist 1.1.0 is available")).toBeVisible();
    expect(install).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Install and restart" }));
    await waitFor(() => expect(install).toHaveBeenCalledWith({ offer_id: OFFER_ID }));
  });

  it("shows active-work blockers without offering force quit", async () => {
    const { user, bridge } = await openUpdates(available);
    vi.spyOn(bridge, "updateInstall").mockResolvedValue({ outcome: "blocked", blockers: [{ id: "run-1", kind: "memory_mutation", label: "memory update for Loupist value (session-1)" }] });
    await user.click(await screen.findByRole("button", { name: "Install and restart" }));
    expect(await screen.findByText("Finish active work before updating")).toBeVisible();
    expect(screen.getByText("memory update for Loupist value (session-1)")).toBeVisible();
    expect(screen.queryByRole("button", { name: /force/i })).not.toBeInTheDocument();
  });

  it("renders native download progress", async () => {
    await openUpdates({ ...available, phase: "downloading", downloaded_bytes: 50, total_bytes: 100 });
    expect(await screen.findByText("Downloading and verifying the update… 50%")).toBeVisible();
  });
});
