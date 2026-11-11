import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "@/i18n";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { GraphProcessingView, KeepAwakeStatus } from "../../types";
import { WorldProcessingSettings } from "./WorldProcessingSettings";

const autoProcess = () => screen.getByRole("checkbox", { name: "Automatically process saved sources" });
const setup = (bridge = new MockLoupistBridge({ delay_ms: 0 })) => ({ bridge, loupistId: "loupist-quality", onChanged: vi.fn(), onManageProviders: vi.fn() });
const selection = async (user: ReturnType<typeof userEvent.setup>) => {
  await user.click(await screen.findByRole("button", { name: /World processing model/ }));
  await user.type(screen.getByRole("searchbox", { name: "Search models" }), "ChatGPT");
  await user.click(screen.getByRole("menuitemradio", { name: "GPT-5.6 Luna" }));
  await user.click(screen.getByRole("menuitem", { name: "Advanced" }));
  await user.click(screen.getByRole("menuitemradio", { name: "High" }));
  await user.click(screen.getByRole("menuitemradio", { name: "Fast" }));
  await user.keyboard("{Escape}");
};

it("uses the shared model catalog and controls while leaving automatic processing opt-in", async () => {
  const props = setup();
  await props.bridge.modelVisibilityUpdate({ model_profile_id: "model-test", visible_in_chat: false });
  const save = vi.spyOn(props.bridge, "knowledgeGraphSettingsSet");
  render(<WorldProcessingSettings {...props} />);
  const user = userEvent.setup();
  expect(await screen.findByRole("checkbox", { name: "Automatically process saved sources" })).not.toBeChecked();
  await selection(user);
  expect(autoProcess()).not.toBeChecked();
  expect(save).not.toHaveBeenCalled();
  await user.click(autoProcess());
  await user.click(screen.getByText("Advanced limits"));
  await user.clear(screen.getByLabelText("Maximum model requests per source"));
  await user.type(screen.getByLabelText("Maximum model requests per source"), "6");
  await user.clear(screen.getByLabelText("Time limit per request (seconds)"));
  await user.type(screen.getByLabelText("Time limit per request (seconds)"), "45");
  await user.click(screen.getByRole("button", { name: "Save processing settings" }));
  await waitFor(() => expect(save).toHaveBeenCalledExactlyOnceWith({ loupistId: props.loupistId, settings: { enrichmentEnabled: true, paused: false, helperModelProfileId: "model-test", thinkingLevel: "high", runOptions: { performance: "fast" }, maxLlmCalls: 6, timeoutMs: 45000, researchFocus: "", relationHints: [], anchors: [], fastModel: null } }));
  expect(await screen.findByText("Ready to review sources")).toBeVisible();
  expect(screen.getByLabelText("World processing readiness")).toHaveTextContent("GPT-5.6 Luna");
});

it("keeps model preferences per Loupist and independent of the Chat picker", async () => {
  const props = setup();
  const { rerender } = render(<WorldProcessingSettings {...props} />);
  const user = userEvent.setup();
  await selection(user);
  await user.click(screen.getByRole("button", { name: "Save processing settings" }));
  await screen.findByText("World processing settings saved for this Loupist.");
  rerender(<WorldProcessingSettings {...props} loupistId="loupist-value" />);
  expect(await screen.findByRole("button", { name: /World processing model/ })).toHaveTextContent("Choose model");
  expect(autoProcess()).not.toBeChecked();
  rerender(<WorldProcessingSettings {...props} />);
  expect(await screen.findByRole("button", { name: /World processing model/ })).toHaveTextContent("GPT-5.6 Luna · High · Fast");
});

it("offers provider management when there are no connected models without enabling processing", async () => {
  const props = setup();
  vi.spyOn(props.bridge, "modelCatalogGet").mockResolvedValue({ models: [], providers: [], custom_providers: [], hidden_model_profile_ids: [], model_notice: null });
  const save = vi.spyOn(props.bridge, "knowledgeGraphSettingsSet");
  render(<WorldProcessingSettings {...props} />);
  const user = userEvent.setup();
  expect(await screen.findByText(/No connected models/)).toBeVisible();
  expect(screen.queryByRole("button", { name: /World processing model/ })).toBeNull();
  expect(autoProcess()).not.toBeChecked();
  await user.click(screen.getByRole("button", { name: "Manage providers" }));
  expect(props.onManageProviders).toHaveBeenCalledOnce();
  await user.click(autoProcess());
  expect(screen.getByRole("button", { name: "Save processing settings" })).toBeDisabled();
  expect(save).not.toHaveBeenCalled();
});

it("shows host readiness for a disconnected provider", async () => {
  const [status, label] = ["provider_disconnected", "Reconnect your AI account"] as const;
  const props = setup();
  const view = await props.bridge.knowledgeGraphSettingsGet({ loupistId: props.loupistId });
  vi.spyOn(props.bridge, "knowledgeGraphSettingsGet").mockResolvedValue({ ...view, settings: { ...view.settings, enrichmentEnabled: true, helperModelProfileId: "removed-model" }, readiness: { status, note: "not_connected", modelName: "Saved model", providerName: "Saved provider", authKind: "oauth" } });
  render(<WorldProcessingSettings {...props} />);
  const readiness = await screen.findByLabelText("World processing readiness");
  expect(within(readiness).getByText(label)).toBeVisible();
  expect(readiness).toHaveTextContent("Saved model");
  expect(within(readiness).getByText("Connect the model provider in Settings.")).toBeVisible();
  expect(screen.getByText(/The saved model is unavailable/)).toBeVisible();
});

it("words the host's readiness note in the interface language", async () => {
  const props = setup();
  const view = await props.bridge.knowledgeGraphSettingsGet({ loupistId: props.loupistId });
  vi.spyOn(props.bridge, "knowledgeGraphSettingsGet").mockResolvedValue({ ...view, readiness: { status: "model_required", note: "awaiting_provider", modelName: null, providerName: null, authKind: null } });
  render(<WorldProcessingSettings {...props} />);
  const readiness = await screen.findByLabelText("World processing readiness");
  expect(readiness).toHaveTextContent("Waiting for a supported model provider. A recommended World model will be selected when one is connected.");
  await act(async () => { await i18n.changeLanguage("ko"); });
  try {
    expect(readiness).toHaveTextContent("지원되는 AI 서비스를 기다리는 중이에요. 연결되면 추천 World 모델을 골라 드려요.");
    expect(readiness).not.toHaveTextContent("Waiting for a supported model provider");
  } finally {
    await act(async () => { await i18n.changeLanguage("en"); });
  }
});

it("preserves edits after save or pause errors and retries settings reads", async () => {
  const props = setup();
  const get = props.bridge.knowledgeGraphSettingsGet.bind(props.bridge);
  vi.spyOn(props.bridge, "knowledgeGraphSettingsGet").mockRejectedValueOnce(new Error("Settings temporarily unavailable")).mockImplementation(get);
  vi.spyOn(props.bridge, "knowledgeGraphSettingsSet").mockRejectedValueOnce(new Error("Settings could not save"));
  vi.spyOn(props.bridge, "knowledgeGraphPause").mockRejectedValueOnce(new Error("Processing could not pause"));
  render(<WorldProcessingSettings {...props} />);
  const user = userEvent.setup();
  expect(await screen.findByRole("alert")).toHaveTextContent("Settings temporarily unavailable");
  await user.click(screen.getByRole("button", { name: "Try again" }));
  await selection(user);
  await user.click(screen.getByRole("button", { name: "Save processing settings" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Settings could not save");
  expect(screen.getByRole("button", { name: /World processing model/ })).toHaveTextContent("High · Fast");
  await user.click(screen.getByRole("button", { name: "Pause processing" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Processing could not pause");
  expect(screen.getByRole("button", { name: /World processing model/ })).toHaveTextContent("High · Fast");
  await user.click(screen.getByRole("button", { name: "Save processing settings" }));
  await screen.findByText("World processing settings saved for this Loupist.");
});

it("ignores an old Loupist save response after navigation", async () => {
  const props = setup();
  const old = await props.bridge.knowledgeGraphSettingsGet({ loupistId: props.loupistId });
  let resolveSave!: (view: GraphProcessingView) => void;
  vi.spyOn(props.bridge, "knowledgeGraphSettingsSet").mockImplementationOnce(() => new Promise((resolve) => { resolveSave = resolve; }));
  const { rerender } = render(<WorldProcessingSettings {...props} />);
  const user = userEvent.setup();
  await selection(user);
  await user.click(screen.getByRole("button", { name: "Save processing settings" }));
  rerender(<WorldProcessingSettings {...props} loupistId="loupist-value" />);
  expect(await screen.findByRole("button", { name: /World processing model/ })).toHaveTextContent("Choose model");
  await act(async () => resolveSave({ ...old, settings: { ...old.settings, helperModelProfileId: "model-test" } }));
  expect(screen.getByRole("button", { name: /World processing model/ })).toHaveTextContent("Choose model");
  expect(props.onChanged).not.toHaveBeenCalled();
});

it("recovers explicitly without a connected model, enabling processing, or saving the draft", async () => {
  const props = setup();
  const saved = await props.bridge.knowledgeGraphSettingsGet({ loupistId: props.loupistId });
  vi.spyOn(props.bridge, "knowledgeGraphSettingsGet").mockResolvedValue({ ...saved, readiness: { status: "reconciliation_required" } });
  vi.spyOn(props.bridge, "modelCatalogGet").mockResolvedValue({ models: [], providers: [], custom_providers: [], hidden_model_profile_ids: [], model_notice: null });
  const save = vi.spyOn(props.bridge, "knowledgeGraphSettingsSet");
  let resolve!: (view: GraphProcessingView) => void;
  const recover = vi.spyOn(props.bridge, "knowledgeGraphRecover").mockReturnValue(new Promise((done) => { resolve = done; }));
  render(<WorldProcessingSettings {...props} />);
  const user = userEvent.setup();
  await screen.findByRole("button", { name: "Recover World processing" });
  await user.click(screen.getByText("Advanced limits"));
  fireEvent.change(screen.getByLabelText("Maximum model requests per source"), { target: { value: "8" } });
  await user.dblClick(screen.getByRole("button", { name: "Recover World processing" }));
  expect(recover).toHaveBeenCalledExactlyOnceWith({ loupistId: props.loupistId });
  expect(screen.getByRole("button", { name: "Starting recovery…" })).toBeDisabled();
  expect(screen.getByText(/Recovery does not use a model/)).toBeVisible();
  await act(async () => resolve(saved));
  expect(screen.getByLabelText("Maximum model requests per source")).toHaveValue(8);
  expect(autoProcess()).not.toBeChecked();
  expect(screen.getByRole("status")).toHaveTextContent("Interrupted processing was not retried");
  expect(screen.queryByRole("button", { name: "Recover World processing" })).toBeNull();
  expect(save).not.toHaveBeenCalled();
  expect(props.onChanged).toHaveBeenCalledOnce();
});

it("resumes read-only recovery polling when opening an in-progress recovery", async () => {
  const props = setup();
  const saved = await props.bridge.knowledgeGraphSettingsGet({ loupistId: props.loupistId });
  const inProgress: GraphProcessingView = { ...saved, readiness: { status: "recovering" } };
  const get = vi.spyOn(props.bridge, "knowledgeGraphSettingsGet").mockResolvedValueOnce(inProgress).mockResolvedValueOnce(inProgress).mockResolvedValue(saved);
  const recover = vi.spyOn(props.bridge, "knowledgeGraphRecover");
  vi.useFakeTimers();
  try {
    render(<WorldProcessingSettings {...props} />);
    await act(async () => {});
    expect(screen.getByRole("status")).toHaveTextContent("Restoring confirmed World data");
    expect(screen.getByRole("button", { name: "Save processing settings" })).toBeDisabled();
    await act(async () => vi.advanceTimersByTimeAsync(2_000));
    expect(screen.getByRole("status")).toHaveTextContent("Restoring confirmed World data");
    await act(async () => vi.advanceTimersByTimeAsync(2_000));
    expect(screen.getByRole("status")).toHaveTextContent("World processing recovered");
    expect(props.onChanged).toHaveBeenCalledOnce();
    await act(async () => vi.advanceTimersByTimeAsync(4_000));
    expect(get).toHaveBeenCalledTimes(3);
    expect(recover).not.toHaveBeenCalled();
  } finally { vi.useRealTimers(); }
});

it("shows a background processing failure without refreshing or losing unsaved guidance", async () => {
  const props = setup();
  const saved = await props.bridge.knowledgeGraphSettingsGet({ loupistId: props.loupistId });
  const ready: GraphProcessingView = { ...saved, readiness: { status: "ready" } };
  const get = vi.spyOn(props.bridge, "knowledgeGraphSettingsGet").mockResolvedValueOnce(ready).mockResolvedValue({
    ...ready, readiness: { status: "reconciliation_required" },
  });
  const save = vi.spyOn(props.bridge, "knowledgeGraphSettingsSet");
  vi.useFakeTimers();
  try {
    render(<WorldProcessingSettings {...props} />);
    await act(async () => {});
    fireEvent.click(screen.getByText("Research guidance"));
    fireEvent.change(screen.getByLabelText("Research focus"), { target: { value: "Keep this unsaved research focus" } });
    await act(async () => vi.advanceTimersByTimeAsync(5_000));
    expect(screen.getByRole("button", { name: "Recover World processing" })).toBeEnabled();
    expect(screen.getByLabelText("Research focus")).toHaveValue("Keep this unsaved research focus");
    expect(screen.getByText(/Unsaved changes/)).toBeVisible();
    await act(async () => vi.advanceTimersByTimeAsync(15_000));
    expect(get).toHaveBeenCalledTimes(2);
    expect(save).not.toHaveBeenCalled();
    expect(props.onChanged).not.toHaveBeenCalled();
  } finally { vi.useRealTimers(); }
});

it("saves guidance-only edits, preserves anchor identifiers, and discards all guidance drafts", async () => {
  const props = setup();
  const saved = await props.bridge.knowledgeGraphSettingsGet({ loupistId: props.loupistId });
  const anchors = [{ name: "Samsung", aliases: ["삼성"], identifiers: ["005930"] }];
  await props.bridge.knowledgeGraphSettingsSet({ loupistId: props.loupistId, settings: { ...saved.settings, anchors } });
  const save = vi.spyOn(props.bridge, "knowledgeGraphSettingsSet");
  render(<WorldProcessingSettings {...props} />);
  const user = userEvent.setup();
  await user.click(await screen.findByText("Research guidance"));
  await user.type(screen.getByLabelText("Preferred relationship names"), "supplies, owns");
  expect(screen.getByRole("button", { name: "Save processing settings" })).toBeEnabled();
  await user.click(screen.getByRole("button", { name: "Save processing settings" }));
  await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ settings: expect.objectContaining({ relationHints: ["supplies", "owns"], anchors }) })));
  await screen.findByText("World processing settings saved for this Loupist.");
  await user.type(screen.getByLabelText("Key companies and topics"), "; Acme");
  await user.click(screen.getByRole("button", { name: "Save processing settings" }));
  await waitFor(() => expect(save).toHaveBeenLastCalledWith(expect.objectContaining({ settings: expect.objectContaining({ anchors: [...anchors, { name: "Acme", aliases: [], identifiers: [] }] }) })));
  await screen.findByText("World processing settings saved for this Loupist.");
  await user.type(screen.getByLabelText("Key companies and topics"), " | Another alias");
  await user.click(screen.getByRole("button", { name: "Discard changes" }));
  expect(screen.getByLabelText("Key companies and topics")).toHaveValue("Samsung | 삼성 | 005930; Acme");
});

const keepAwakeLabel = "Keep this Mac awake while the World is processing";
const keepAwake = (overrides: Partial<KeepAwakeStatus> = {}): KeepAwakeStatus => ({ supported: true, enabled: false, active: false, onBattery: false, remaining: 0, ...overrides });

it("hides keep-awake where the host cannot prevent sleep", async () => {
  const props = setup();
  vi.spyOn(props.bridge, "worldKeepAwakeGet").mockResolvedValue(keepAwake({ supported: false, enabled: true }));
  render(<WorldProcessingSettings {...props} />);
  await screen.findByRole("form", { name: "World processing settings" });
  await waitFor(() => expect(props.bridge.worldKeepAwakeGet).toHaveBeenCalled());
  expect(screen.queryByRole("checkbox", { name: keepAwakeLabel })).toBeNull();
  expect(screen.queryByText(/closing the lid/)).toBeNull();
});

it("saves keep-awake at once for every Loupist without touching the processing form", async () => {
  const props = setup();
  const save = vi.spyOn(props.bridge, "knowledgeGraphSettingsSet");
  let resolveSet!: (status: KeepAwakeStatus) => void;
  const set = vi.spyOn(props.bridge, "worldKeepAwakeSet").mockImplementationOnce(() => new Promise((resolve) => { resolveSet = resolve; }));
  render(<WorldProcessingSettings {...props} />);
  const user = userEvent.setup();
  const toggle = await screen.findByRole("checkbox", { name: keepAwakeLabel });
  expect(toggle).not.toBeChecked();
  expect(screen.getByText(/Applies to every Loupist, and only while plugged in/)).toHaveTextContent("closing the lid still puts the Mac to sleep");
  await user.click(toggle);
  expect(set).toHaveBeenCalledExactlyOnceWith({ enabled: true });
  expect(toggle).toBeDisabled();
  await act(async () => resolveSet(keepAwake({ enabled: true, active: true, remaining: 1 })));
  expect(toggle).toBeChecked();
  expect(toggle).toBeEnabled();
  expect(screen.getByRole("status")).toHaveTextContent("Keeping this Mac awake · 1 source left");
  expect(screen.queryByText(/Unsaved changes/)).toBeNull();
  expect(screen.getByRole("button", { name: "Save processing settings" })).toBeDisabled();
  expect(save).not.toHaveBeenCalled();
  set.mockRejectedValueOnce(new Error("Settings are read-only"));
  await user.click(toggle);
  expect(await screen.findByRole("alert")).toHaveTextContent("Settings are read-only");
  expect(autoProcess()).not.toBeChecked();
});

it("shows how many sources keep the Mac awake and when battery power lets it sleep", async () => {
  const props = setup();
  const get = vi.spyOn(props.bridge, "worldKeepAwakeGet").mockResolvedValueOnce(keepAwake({ enabled: true, active: true, remaining: 1234 })).mockResolvedValue(keepAwake({ enabled: true, onBattery: true, remaining: 1234 }));
  vi.useFakeTimers();
  try {
    render(<WorldProcessingSettings {...props} />);
    // Settings load first; the keep-awake read follows.
    await act(async () => {});
    await act(async () => {});
    expect(get).toHaveBeenCalledOnce();
    expect(screen.getByRole("status")).toHaveTextContent("Keeping this Mac awake · 1,234 sources left");
    await act(async () => vi.advanceTimersByTimeAsync(5_000));
    expect(get).toHaveBeenCalledTimes(2);
    expect(screen.queryByText(/Keeping this Mac awake/)).toBeNull();
    expect(screen.getByText("On battery power, so this Mac can sleep as usual.")).toBeVisible();
  } finally { vi.useRealTimers(); }
});
