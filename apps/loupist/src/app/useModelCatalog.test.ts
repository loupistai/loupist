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
  it("makes a saved sign-in usable without visiting Settings", async () => {
    const { bridge, populated } = await emptyConnectedCatalog();
    const discover = vi.spyOn(bridge, "providerModels").mockResolvedValue(populated);
    const { result } = renderHook(() => useModelCatalog(bridge));
    await waitFor(() => expect(result.current.resolved).toBe(true));
    expect(discover).toHaveBeenCalledOnce();
    expect(result.current.catalog).toEqual(populated);
    expect(result.current.needsVisibleModels).toBe(false);
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
    expect(result.current.settingsError).toBe("Provider unavailable");
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
