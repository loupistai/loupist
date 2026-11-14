import { act, render, renderHook, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "@/i18n";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { UpdateSettingsCard } from "../../components/settings/UpdateSettingsCard";
import { useAppUpdate } from "../../components/settings/useAppUpdate";
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
  it("does not call the app up to date before a successful check", async () => {
    await openUpdates({ ...current, last_checked_at_ms: null });
    await screen.findByRole("button", { name: "Check for updates" });
    expect(screen.queryByText("Loupist is up to date")).not.toBeInTheDocument();
    expect(screen.getByText("Check for updates to see the installed and latest versions.")).toBeVisible();
  });

  it("does not call a failed check up to date", async () => {
    await openUpdates({ ...current, error: "Could not check for updates." });
    await screen.findByRole("button", { name: "Check for updates" });
    expect(screen.queryByText("Loupist is up to date")).not.toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("Could not check for updates.");
  });

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
    await waitFor(() => expect(install).toHaveBeenCalledWith({
      offer_id: OFFER_ID,
      confirmation: {
        title: "Install Loupist update",
        message: "Update Loupist from 0.0.1 to 1.1.0? Active work has been checked. Loupist will download the signed update, install it and restart right away.",
        install: "Install and restart",
        cancel: "Not now",
      },
    }));
  });

  it("words the native confirmation in Korean", async () => {
    await act(async () => { await i18n.changeLanguage("ko"); });
    try {
      const bridge = new MockLoupistBridge({ delay_ms: 0 });
      vi.spyOn(bridge, "updateStatus").mockResolvedValue(available);
      const install = vi.spyOn(bridge, "updateInstall").mockResolvedValue({ outcome: "cancelled", blockers: [] });
      const { result } = renderHook(() => useAppUpdate(bridge));
      await waitFor(() => expect(result.current.status?.offer?.offer_id).toBe(OFFER_ID));
      await act(() => result.current.installUpdate(OFFER_ID));
      expect(install).toHaveBeenCalledWith({
        offer_id: OFFER_ID,
        confirmation: {
          title: "루피스트 업데이트 설치",
          message: "루피스트를 업데이트할까요? 현재 버전 0.0.1, 새 버전 1.1.0. 진행 중인 작업을 확인했어요. 서명된 업데이트를 내려받아 설치한 뒤 바로 다시 시작해요.",
          install: "설치 후 다시 시작",
          cancel: "나중에",
        },
      });
      render(<UpdateSettingsCard status={available} phase="confirming" error={null} onCheck={vi.fn()} onInstall={vi.fn()} />);
      expect(screen.getByText("방금 열린 확인 창에서 선택해 주세요.")).toBeVisible();
    } finally { await act(async () => { await i18n.changeLanguage("en"); }); }
  });

  it("says where to confirm while the native dialog is open", () => {
    render(<UpdateSettingsCard status={available} phase="confirming" error={null} onCheck={vi.fn()} onInstall={vi.fn()} />);
    expect(screen.getByText("Confirm in the dialog that just opened.")).toBeVisible();
  });

  it("dates a release the updater sent in its own form, never the raw text", async () => {
    await act(async () => { await i18n.changeLanguage("ko"); });
    try {
      const raw = { ...available, offer: { ...available.offer!, published_at: "2026-11-14 8:50:41.0 +00:00:00" } };
      render(<UpdateSettingsCard status={raw} phase="idle" error={null} onCheck={vi.fn()} onInstall={vi.fn()} />);
      expect(screen.getByText("2026년 11월 14일")).toBeVisible();
      expect(screen.queryByText(/8:50:41/)).not.toBeInTheDocument();
    } finally { await act(async () => { await i18n.changeLanguage("en"); }); }
  });

  it("leaves out a release date it cannot read", () => {
    const unreadable = { ...available, offer: { ...available.offer!, published_at: "soon" } };
    render(<UpdateSettingsCard status={unreadable} phase="idle" error={null} onCheck={vi.fn()} onInstall={vi.fn()} />);
    expect(screen.queryByText(/Released/)).not.toBeInTheDocument();
    expect(screen.queryByText("soon")).not.toBeInTheDocument();
  });

  it("dates a Korean release in the long form", async () => {
    await act(async () => { await i18n.changeLanguage("ko"); });
    try {
      render(<UpdateSettingsCard status={available} phase="idle" error={null} onCheck={vi.fn()} onInstall={vi.fn()} />);
      expect(screen.getByText("2026년 8월 9일")).toBeVisible();
    } finally { await act(async () => { await i18n.changeLanguage("en"); }); }
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
