import { renderHook } from "@testing-library/react";
import { useChatViewModel } from "./useChatViewModel";
import type { ModelRunSelection } from "../types";

type Input = Parameters<typeof useChatViewModel>[0];
function input(home: boolean, configuredModel: string | null = "xai/grok-4.7", explicit?: ModelRunSelection): Input {
  const model = { id: "xai/grok-4.7", provider: "xai", model: "grok-4.7", name: "Grok", thinking_levels: ["low", "medium", "high"],
    input: ["text"], reasoning: true, context_window: 100_000, max_tokens: 20_000, run_controls: [], thinking_level_map: {}, credential_source: "saved" };
  const snapshot = { modelSelection: { model, is_default: !configuredModel, thinking_level: "high", run_controls: [], run_options: {} }, diagnostics: [], effective: { chat: { model_profile_id: configuredModel, thinking_level: "high", run_options: {} }, models: { hidden_model_profile_ids: [] }, skills: {} } };
  // Only the returned view-model props execute here; bridge/run callbacks stay idle.
  return {
    selectedLoupist: { id: "loupist-a", last_model_profile_id: model.id }, activeChat: {},
    threads: [{ id: "thread-a", home }], effectiveThreadId: "thread-a", effectiveChatKey: null,
    model: { catalog: { models: [model], providers: [], custom_providers: [], hidden_model_profile_ids: [] }, selections: explicit ? { "loupist-a": explicit } : {}, changeSelection: vi.fn() },
    configuration: { snapshot, reload: async () => snapshot }, bridge: {}, chat: {}, workspace: {},
    loupist: {}, drafts: { getSession: () => [] }, threadActions: {},
  } as unknown as Input;
}

it("shows and submits the same saved reasoning default on both surfaces, preserving an explicit composer choice", async () => {
  const { result, rerender } = renderHook((props: Input) => useChatViewModel(props), { initialProps: input(true) });
  expect(result.current!.modelSelection.thinking_level).toBe("high");
  expect(await result.current!.onResolveModelSelection!()).toMatchObject({ thinking_level: "high" });
  rerender(input(false));
  expect(result.current!.modelSelection.thinking_level).toBe("high");
  expect(await result.current!.onResolveModelSelection!()).toMatchObject({ thinking_level: "high" });
  rerender(input(true, "xai/grok-4.7", { model_profile_id: "xai/grok-4.7", thinking_level: "medium", run_options: {} }));
  expect(await result.current!.onResolveModelSelection!()).toMatchObject({ thinking_level: "medium" });
});

it("names the host-selected model and submits the saved controls without pinning it", async () => {
  const { result } = renderHook(() => useChatViewModel(input(true, null)));
  expect(result.current!.modelSelection.thinking_level).toBe("high");
  expect(await result.current!.onResolveModelSelection!()).toMatchObject({ model_profile_id: "", thinking_level: "high", auto: true });
});

it("never sends a composer pick the latest configuration hides: the Loupist's default answers and the pick is dropped", async () => {
  const pick = { model_profile_id: "xai/grok-4.7", thinking_level: "medium", run_options: {} };
  const props = input(false, null, pick);
  const resetSelection = vi.fn();
  const snapshot = await props.configuration.reload();
  const hidden = { ...snapshot!, effective: { ...snapshot!.effective, models: { hidden_model_profile_ids: [pick.model_profile_id] } } };
  const { result } = renderHook(() => useChatViewModel({ ...props, model: { ...props.model, resetSelection }, configuration: { ...props.configuration, reload: async () => hidden } }));
  expect(result.current!.modelSelection).toMatchObject({ thinking_level: "medium" });
  expect(await result.current!.onResolveModelSelection!()).toMatchObject({ model_profile_id: "", thinking_level: "high", auto: true });
  expect(resetSelection).toHaveBeenCalledExactlyOnceWith("loupist-a");
});
