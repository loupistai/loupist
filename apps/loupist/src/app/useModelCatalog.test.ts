import { act, renderHook, waitFor } from "@testing-library/react";
import { MockLoupistBridge } from "../bridge/mockBridge";
import type { ModelCatalog } from "../types";
import { useModelCatalog } from "./useModelCatalog";

async function emptyConnectedCatalog() {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const populated = await bridge.modelCatalogGet();
  const empty: ModelCatalog = { ...populated, models: [] };
  vi.spyOn(bridge, "modelCatalogGet").mockResolvedValue(empty);
  return { bridge, empty, populated };
}

describe("startup model discovery", () => {
  it("shows the saved catalog and reloads it when the background pass publishes", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const saved = await bridge.modelCatalogGet();
    const updated = { ...saved, models: saved.models.map(model => ({ ...model, name: "Updated model" })) };
    const read = vi.spyOn(bridge, "modelCatalogGet").mockResolvedValueOnce(saved).mockResolvedValue(updated);
    let publish!: () => void;
    const source = Object.assign(bridge, {
      onModelCatalogChanged(handler: () => void, onListening: () => void) {
        publish = handler;
        onListening();
        return () => {};
      },
    });
    const { result } = renderHook(() => useModelCatalog(source));
    await waitFor(() => expect(result.current.resolved).toBe(true));
    expect(result.current.catalog).toEqual(saved);
    expect(read).toHaveBeenCalledTimes(1);
    act(() => publish());
    await waitFor(() => expect(result.current.catalog).toEqual(updated));
  });

  it("makes a saved sign-in usable without visiting Settings", async () => {
    const { bridge, populated } = await emptyConnectedCatalog();
    const discover = vi.spyOn(bridge, "providerModels").mockResolvedValue(populated);
    const { result } = renderHook(() => useModelCatalog(bridge));
    await waitFor(() => expect(result.current.resolved).toBe(true));
    expect(discover).toHaveBeenCalledOnce();
    expect(result.current.catalog).toEqual(populated);
    expect(result.current.needsVisibleModels).toBe(false);
  });

  it("restores a second connected account even when the first already has models", async () => {
    const { bridge, populated } = await emptyConnectedCatalog();
    const first = populated.providers.find(provider => provider.credential_source !== "missing")!;
    const second = populated.providers.find(provider => provider.id !== first.id)!;
    const initial = {
      ...populated,
      providers: populated.providers.map(provider => provider.id === second.id ? { ...provider, credential_source: "saved" as const } : provider),
      models: populated.models.filter(model => model.provider !== second.id),
    };
    const grok = { ...populated.models[0]!, id: `${second.id}/restored`, provider: second.id, name: "Restored model" };
    vi.mocked(bridge.modelCatalogGet).mockResolvedValue(initial);
    const discover = vi.spyOn(bridge, "providerModels").mockResolvedValue({ ...initial, models: [...initial.models, grok] });
    const { result } = renderHook(() => useModelCatalog(bridge));
    await waitFor(() => expect(result.current.resolved).toBe(true));
    expect(discover).toHaveBeenCalledWith(second.id);
    expect(result.current.catalog?.models).toContainEqual(grok);
  });

  it("preserves deliberately hidden models without rediscovery", async () => {
    const { bridge, populated } = await emptyConnectedCatalog();
    vi.mocked(bridge.modelCatalogGet).mockResolvedValue({ ...populated, hidden_model_profile_ids: populated.models.map((model) => model.id) });
    const discover = vi.spyOn(bridge, "providerModels");
    const { result } = renderHook(() => useModelCatalog(bridge));
    await waitFor(() => expect(result.current.resolved).toBe(true));
    expect(discover).not.toHaveBeenCalled();
    expect(result.current.needsVisibleModels).toBe(true);
  });

  it("finishes startup with an actionable error when discovery fails", async () => {
    const { bridge } = await emptyConnectedCatalog();
    vi.spyOn(bridge, "providerModels").mockRejectedValue(new Error("Provider unavailable"));
    const { result } = renderHook(() => useModelCatalog(bridge));
    await waitFor(() => expect(result.current.resolved).toBe(true));
    expect(result.current.settingsError).toBe("Could not load available models.");
  });

  it("ignores an old discovery after switching bridges", async () => {
    const { bridge, populated } = await emptyConnectedCatalog();
    let finish!: (catalog: ModelCatalog) => void;
    const discover = vi.spyOn(bridge, "providerModels").mockReturnValue(new Promise((resolve) => { finish = resolve; }));
    const replacement = new MockLoupistBridge({ delay_ms: 0 });
    const next = await replacement.modelCatalogGet();
    next.models = next.models.map((model) => ({ ...model, name: "Replacement model" }));
    vi.spyOn(replacement, "modelCatalogGet").mockResolvedValue(next);
    const { result, rerender } = renderHook(({ source }) => useModelCatalog(source), { initialProps: { source: bridge } });
    await waitFor(() => expect(discover).toHaveBeenCalled());
    rerender({ source: replacement });
    await waitFor(() => expect(result.current.catalog).toEqual(next));
    await act(async () => finish(populated));
    expect(result.current.catalog).toEqual(next);
  });
});

describe("composer picks for this app session, by chat", () => {
  const claude = { model_profile_id: "anthropic/claude-sonnet-4-5", thinking_level: "medium", run_options: {} };
  const luna = { model_profile_id: "model-test", thinking_level: "high", run_options: { performance: "standard" } };
  // `chatSessionKey`: a chat of each Loupist.
  const quality = "loupist-quality:thread-margin", cycle = "loupist-cycle:thread-inventory";
  async function twoAccounts() {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    await bridge.providerConfigure({ provider: "anthropic", api_key: "test-key" });
    const view = renderHook(() => useModelCatalog(bridge));
    await waitFor(() => expect(view.result.current.resolved).toBe(true));
    act(() => {
      view.result.current.changeSelection(quality, claude);
      view.result.current.changeSelection(cycle, luna);
    });
    return { bridge, ...view };
  }

  it("turns a pick into its chat's Default row once its account is disconnected, and keeps it there when the account returns", async () => {
    const { bridge, result } = await twoAccounts();
    await act(() => result.current.saveSent(quality, claude, claude));
    expect(result.current.selections).toEqual({ [quality]: claude, [cycle]: luna });
    expect(result.current.lastSent).toEqual(claude);
    await act(() => result.current.providerActions.onDisconnect("anthropic"));
    expect(result.current.selections).toEqual({ [quality]: null, [cycle]: luna });
    expect(result.current.lastSent).toBeUndefined();
    await act(async () => { result.current.setCatalog(await bridge.providerConfigure({ provider: "anthropic", api_key: "test-key" })); });
    expect(result.current.catalog?.models.some((model) => model.id === claude.model_profile_id)).toBe(true);
    expect(result.current.selections).toEqual({ [quality]: null, [cycle]: luna });
  });

  it("drops a pick whose model is hidden from Chat or no longer offered", async () => {
    const { bridge, result } = await twoAccounts();
    await act(() => result.current.providerActions.onUpdateModelVisibility({ model_profile_id: claude.model_profile_id, visible_in_chat: false }));
    expect(result.current.selections).toEqual({ [quality]: null, [cycle]: luna });
    const catalog = await bridge.modelCatalogGet();
    act(() => result.current.setCatalog({ ...catalog, models: catalog.models.filter((model) => model.id !== luna.model_profile_id) }));
    expect(result.current.selections).toEqual({ [quality]: null, [cycle]: null });
  });

  it("yields a Loupist's chats to its saved Chat default, every chat to the app's, and new chats to the default", async () => {
    const { result } = await twoAccounts();
    await act(() => result.current.saveSent(quality, claude, claude));
    act(() => result.current.yieldToChatDefault({ kind: "project", loupistId: "loupist-quality" }));
    expect(result.current.selections).toEqual({ [cycle]: luna });
    expect(result.current.lastSent).toBeUndefined();
    act(() => result.current.changeSelection(quality, claude));
    act(() => result.current.yieldToChatDefault({ kind: "global" }));
    expect(result.current.selections).toEqual({});
  });

  it("hands a new chat's choice to the chat its first message creates", async () => {
    const { result } = await twoAccounts();
    const draft = "loupist-quality:__empty-chat__", created = "loupist-quality:thread-new";
    act(() => result.current.changeSelection(draft, luna));
    act(() => result.current.moveSelection(draft, created, luna));
    expect(result.current.selections).toEqual({ [quality]: claude, [cycle]: luna, [created]: luna });
    // Its Default row moves too; nothing chosen leaves the next new chat to start afresh.
    act(() => result.current.resetSelection(draft));
    act(() => result.current.moveSelection(draft, "loupist-quality:thread-next", null));
    act(() => result.current.moveSelection(draft, "loupist-quality:thread-last", undefined));
    expect(result.current.selections).toMatchObject({ "loupist-quality:thread-next": null });
    expect(Object.keys(result.current.selections)).not.toContain(draft);
    expect(Object.keys(result.current.selections)).not.toContain("loupist-quality:thread-last");
  });
});
