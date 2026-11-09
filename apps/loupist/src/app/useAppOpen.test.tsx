import { act, renderHook } from "@testing-library/react";
import i18n from "@/i18n";
import { evidenceFixture } from "../test/evidenceFixtures";
import { useAppOpen, type AppOpenShell } from "./useAppOpen";

const shell = (): AppOpenShell => ({
  openExternalUrl: vi.fn(async () => undefined),
  readArtifact: vi.fn(),
  loupistId: "loupist-a",
  threadId: "chat-a",
  panel: { openEvidence: vi.fn(), openBrowser: vi.fn(), openMemory: vi.fn(), openArtifact: vi.fn(), openFiles: vi.fn() },
  showThread: vi.fn(),
  openKnowledgePage: vi.fn(),
  openAssetPage: vi.fn(),
  openScheduledTask: vi.fn(),
  showBriefing: vi.fn(),
  openSettings: vi.fn(),
});

function router(targets = shell()) {
  const hook = renderHook(() => useAppOpen(targets));
  return { targets, hook, announced: () => hook.result.current.announcement?.text };
}

it("opens a file beside the visible chat, and beside a briefing's own chat after showing it", async () => {
  const { targets, hook, announced } = router();
  await act(() => hook.result.current.open({ kind: "file", path: "reports/summary.md" }));
  expect(targets.panel.openFiles).toHaveBeenLastCalledWith("loupist-a", "chat-a", "reports/summary.md");
  expect(targets.showThread).not.toHaveBeenCalled();
  expect(announced()).toBe("Opened summary.md in the panel");
  await act(() => hook.result.current.openIn("briefing-chat", { kind: "file", path: "reports/summary.md" }));
  expect(targets.showThread).toHaveBeenCalledExactlyOnceWith("loupist-a", "briefing-chat");
  expect(targets.panel.openFiles).toHaveBeenLastCalledWith("loupist-a", "briefing-chat", "reports/summary.md");
});

it("opens a Lens record in the Evidence reader under the interface language's name", async () => {
  await i18n.changeLanguage("ko");
  try {
    const { targets, hook, announced } = router();
    await act(() => hook.result.current.open({ kind: "lens", recordId: "rec-1" }));
    expect(targets.panel.openMemory).toHaveBeenCalledExactlyOnceWith("loupist-a", "chat-a", expect.objectContaining({ surface: "lens", recordId: "rec-1", selection: { kind: "lens", recordId: "rec-1" } }), i18n.t("app:shell.tabs.lens"));
    expect(announced()).toBe(i18n.t("chat:navigation.openedInPanel", { title: i18n.t("app:shell.tabs.lens") }));
    expect(announced()).toMatch(/패널에서 열었어요$/u);
  } finally {
    await i18n.changeLanguage("en");
  }
});

it("opens browser and reader evidence in the panel with its step, and follows the fallback for the rest", async () => {
  const { targets, hook } = router();
  const page = evidenceFixture({ url: "https://example.com/report" });
  const step = { label: "Read report", itemId: "read" };
  await act(() => hook.result.current.open({ kind: "evidence", evidence: page, step }));
  expect(targets.panel.openEvidence).toHaveBeenCalledExactlyOnceWith("loupist-a", "chat-a", page, step);
  const quote = evidenceFixture({ id: "ev_quote", tool_name: "mcp__openbb__equity_price_quote", url: undefined, excerpt: JSON.stringify({ results: [{ symbol: "AAPL", currency: "USD", last_price: 201.5 }] }) });
  const asset = { loupistId: "loupist-a", assetId: "NASDAQ:AAPL" };
  await act(() => hook.result.current.open({ kind: "evidence", evidence: quote, fallback: { kind: "asset", target: asset } }));
  expect(targets.openAssetPage).toHaveBeenCalledExactlyOnceWith(asset);
  expect(targets.panel.openEvidence).toHaveBeenCalledOnce();
});

it("hands a system-browser open to the host and nothing else", async () => {
  const { targets, hook, announced } = router();
  await act(() => hook.result.current.open({ kind: "link", url: "https://www.example.com/a" }, { external: true }));
  expect(targets.openExternalUrl).toHaveBeenCalledExactlyOnceWith("https://www.example.com/a");
  expect(targets.panel.openBrowser).not.toHaveBeenCalled();
  expect(announced()).toBeUndefined();
  await act(() => hook.result.current.open({ kind: "link", url: "https://www.example.com/a" }));
  expect(targets.panel.openBrowser).toHaveBeenCalledExactlyOnceWith("loupist-a", "chat-a", "https://www.example.com/a");
  expect(announced()).toBe("Opened example.com in the panel");
});

it("keeps one opener identity while the shell it routes to changes", async () => {
  const first = shell();
  const hook = renderHook((targets: AppOpenShell) => useAppOpen(targets), { initialProps: first });
  const { open } = hook.result.current;
  const second = { ...shell(), threadId: "chat-b" };
  hook.rerender(second);
  expect(hook.result.current.open).toBe(open);
  await act(() => open({ kind: "file" }));
  expect(second.panel.openFiles).toHaveBeenCalledExactlyOnceWith("loupist-a", "chat-b", undefined);
  expect(first.panel.openFiles).not.toHaveBeenCalled();
});

it("never opens a hidden panel for a briefing that has no chat of its own", async () => {
  const { targets, hook, announced } = router();
  await act(() => hook.result.current.openIn(null, { kind: "file", path: "reports/summary.md" }));
  await act(() => hook.result.current.openIn(null, { kind: "lens", recordId: "rec-1" }));
  expect(targets.panel.openFiles).not.toHaveBeenCalled();
  expect(targets.panel.openMemory).not.toHaveBeenCalled();
  expect(announced()).toBeUndefined();
  await act(() => hook.result.current.openIn(null, { kind: "link", url: "https://example.com/a" }));
  expect(targets.openExternalUrl).toHaveBeenCalledExactlyOnceWith("https://example.com/a");
  expect(targets.panel.openBrowser).not.toHaveBeenCalled();
  const asset = { loupistId: "loupist-a", assetId: "NASDAQ:AAPL" };
  await act(() => hook.result.current.openIn(null, { kind: "asset", target: asset }));
  expect(targets.openAssetPage).toHaveBeenCalledExactlyOnceWith(asset);
});

it("resolves a retained artifact link in its chat to the latest saved revision", async () => {
  const { targets, hook, announced } = router();
  vi.mocked(targets.readArtifact).mockResolvedValue({
    artifact: { id: "artifact-1", chat_session_id: "chat-a", title: "Updated analysis", kind: "markdown", current_revision: 2, created_at_ms: 1, updated_at_ms: 2 },
    revision: { artifact_id: "artifact-1", revision: 2, digest: "latest", source_message_id: "answer", created_at_ms: 2, payload: { kind: "markdown", schema: "loupist-markdown/1", markdown: "Updated" } },
  });
  await act(() => hook.result.current.open({ kind: "artifact-link", artifactId: "artifact-1" }));
  expect(targets.readArtifact).toHaveBeenCalledExactlyOnceWith("loupist-a", "chat-a", "artifact-1");
  expect(targets.panel.openArtifact).toHaveBeenCalledExactlyOnceWith("loupist-a", "chat-a", { artifact_id: "artifact-1", revision: 2, digest: "latest", kind: "markdown", title: "Updated analysis" });
  expect(announced()).toBe("Opened Updated analysis in the panel");
  vi.mocked(targets.readArtifact).mockRejectedValueOnce(new Error("outside this chat"));
  await expect(hook.result.current.open({ kind: "artifact-link", artifactId: "outside" })).rejects.toThrow("outside this chat");
  expect(targets.panel.openArtifact).toHaveBeenCalledOnce();
});
