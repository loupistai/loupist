import { act, fireEvent, render, screen } from "@testing-library/react";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import i18n from "../../i18n";
import type { UpdateState } from "../../types";
import { UpdateSettingsCard } from "./UpdateSettingsCard";
import { useAppUpdate } from "./useAppUpdate";

const idle: UpdateState = {
  supported: true, current_version: "0.1.0", phase: "idle", offer: null,
  downloaded_bytes: 0, total_bytes: null, last_checked_at_ms: null,
  next_auto_check_at_ms: null, error: null, blockers: [],
};

function Updates({ bridge }: { bridge: MockLoupistBridge }) {
  const update = useAppUpdate(bridge);
  return <UpdateSettingsCard {...update} onCheck={update.checkForUpdates} onInstall={update.installUpdate} />;
}

afterEach(async () => { await act(() => i18n.changeLanguage("en")); });

const copy = {
  en: {
    background: "Couldn't check for updates. Loupist will try again later.",
    manual: "Couldn't check for updates. Check your internet connection and try again.",
    download: "Couldn't download or verify the update. Try again.",
    install: "Couldn't install the update. Try again.",
    changed: "The available update changed. Check again before installing.",
    verify: "Couldn't verify the update. Check for updates and try again.",
    schedule: "Couldn't save the update schedule. Try again.",
    unavailable: "Automatic updates are unavailable in this build.",
    status: "Could not read update status.",
    busy: "Another update is in progress. Try again when it finishes.",
  },
  ko: {
    background: "업데이트를 확인하지 못했어요. 잠시 후 다시 시도할게요.",
    manual: "업데이트를 확인하지 못했어요. 인터넷 연결을 확인한 뒤 다시 시도해 주세요.",
    download: "업데이트를 내려받거나 서명을 확인하지 못했어요. 다시 시도해 주세요.",
    install: "업데이트를 설치하지 못했어요. 다시 시도해 주세요.",
    changed: "설치할 업데이트가 변경됐어요. 업데이트를 다시 확인해 주세요.",
    verify: "업데이트를 검증하지 못했어요. 업데이트를 다시 확인한 뒤 시도해 주세요.",
    schedule: "업데이트 일정을 저장하지 못했어요. 다시 시도해 주세요.",
    unavailable: "이 빌드에서는 자동 업데이트를 사용할 수 없어요.",
    status: "업데이트 상태를 읽지 못했어요. 다시 시도해 주세요.",
    busy: "다른 업데이트 작업이 진행 중이에요. 끝난 뒤 다시 시도해 주세요.",
  },
};

for (const language of ["en", "ko"] as const) {
  describe(`${language} update errors`, () => {
    const words = copy[language];
    beforeEach(async () => { await i18n.changeLanguage(language); });

    it.each([
      ["Could not check for updates. Loupist will retry.", words.background],
      ["Could not save update scheduling state.", words.schedule],
      ["Could not verify the update. Try again.", words.verify],
      ["Could not download the update. Try again.", words.download],
      ["The update download timed out. Try again.", words.download],
      ["Could not install the update. Try again.", words.install],
      ["The available update changed. Check again before installing.", words.changed],
      ["Automatic updates are unavailable in this build.", words.unavailable],
      ["unrecognized host diagnostics", words.status],
    ])("localizes background state: %s", async (hostError, expected) => {
      const bridge = new MockLoupistBridge({ delay_ms: 0 });
      vi.spyOn(bridge, "updateStatus").mockResolvedValue({ ...idle, error: hostError });
      render(<Updates bridge={bridge} />);
      expect(await screen.findByRole("alert")).toHaveTextContent(expected);
    });

    it("localizes manual failures even when the host has no failed status", async () => {
      const bridge = new MockLoupistBridge({ delay_ms: 0 });
      vi.spyOn(bridge, "updateStatus").mockResolvedValue(idle);
      vi.spyOn(bridge, "updateCheck").mockRejectedValue({ code: "update_check_failed", message: "update check failed" });
      render(<Updates bridge={bridge} />);
      await act(async () => {});
      fireEvent.click(screen.getByRole("button", { name: language === "ko" ? "업데이트 확인" : "Check for updates" }));
      expect(await screen.findByRole("alert")).toHaveTextContent(words.manual);
    });

    it.each([
      ["update_download_failed", "signature refused", words.download],
      ["update_install_failed", "update installation failed", words.install],
      ["conflict", "the available update changed; check again before installing", words.changed],
      ["update_check_failed", "update check timed out", words.verify],
      ["update_busy", "another update operation is already active", words.busy],
      ["updater_unavailable", "updater is not available in this Loupist build", words.unavailable],
      ["not_found", "update was not found", words.changed],
      ["internal", "private host diagnostics", words.install],
    ])("localizes install rejection: %s", async (code, message, expected) => {
      const bridge = new MockLoupistBridge({ delay_ms: 0 });
      vi.spyOn(bridge, "updateStatus").mockResolvedValue({ ...idle, offer: { offer_id: "offer", version: "0.1.1", notes: null, published_at: null } });
      vi.spyOn(bridge, "updateInstall").mockRejectedValue({ code, message });
      render(<Updates bridge={bridge} />);
      fireEvent.click(await screen.findByRole("button", { name: language === "ko" ? "설치 후 다시 시작" : "Install and restart" }));
      expect(await screen.findByRole("alert")).toHaveTextContent(expected);
    });

    it("never shows raw status-read errors", async () => {
      const bridge = new MockLoupistBridge({ delay_ms: 0 });
      vi.spyOn(bridge, "updateStatus").mockRejectedValue(new Error("host diagnostics"));
      render(<Updates bridge={bridge} />);
      expect(await screen.findByRole("alert")).toHaveTextContent(words.status);
    });

    it("localizes update blockers by kind, including unknown activities", () => {
      const kinds = ["chat", "chat_mutation", "memory_write", "memory_mutation", "loupist_mutation", "loupist_deletion", "marketplace_configuration", "marketplace_credential", "provider_inference", "provider_support", "graph", "update", "future_kind"];
      const expected = language === "ko" ? [
        "대화 진행 중", "대화 변경 중", "메모리 저장 중", "메모리 변경 중", "루피스트 변경 중", "루피스트 삭제 중", "마켓플레이스 설정 변경 중", "마켓플레이스 인증 정보 변경 중", "백그라운드 모델 작업 중", "모델 연결 확인 중", "지도 처리 중", "업데이트 설치 중", "작업 진행 중",
      ] : [
        "Chat in progress", "Changing a chat", "Saving Memory", "Changing Memory", "Changing a Loupist", "Deleting a Loupist", "Changing Marketplace settings", "Changing Marketplace credentials", "Background model work", "Checking a model connection", "Processing the map", "Installing an update", "Work in progress",
      ];
      render(<UpdateSettingsCard status={{ ...idle, blockers: kinds.map((kind) => ({ kind, id: kind, label: `raw host label: ${kind}` })) }} phase="idle" error={null} onCheck={vi.fn()} onInstall={vi.fn()} />);
      expect(screen.getAllByRole("listitem").map(item => item.textContent)).toEqual(expected);
    });
  });
}

it("rewords an existing error when the interface language changes", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "updateStatus").mockResolvedValue({ ...idle, error: "Could not check for updates. Loupist will retry." });
  render(<Updates bridge={bridge} />);
  expect(await screen.findByRole("alert")).toHaveTextContent(copy.en.background);
  await act(() => i18n.changeLanguage("ko"));
  expect(screen.getByRole("alert")).toHaveTextContent(copy.ko.background);
});
