import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { GraphProcessingView } from "../../types";
import { WorldProcessingSettings } from "./WorldProcessingSettings";

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
  expect(screen.getByRole("checkbox")).not.toBeChecked();
  expect(save).not.toHaveBeenCalled();
  await user.click(screen.getByRole("checkbox"));
  await user.click(screen.getByText("Advanced limits"));
  await user.clear(screen.getByLabelText("Maximum model requests per source"));
  await user.type(screen.getByLabelText("Maximum model requests per source"), "6");
  await user.clear(screen.getByLabelText("Time limit per request (seconds)"));
  await user.type(screen.getByLabelText("Time limit per request (seconds)"), "45");
  await user.click(screen.getByRole("button", { name: "Save processing settings" }));
  await waitFor(() => expect(save).toHaveBeenCalledExactlyOnceWith({ loupistId: props.loupistId, settings: { enrichmentEnabled: true, paused: false, helperModelProfileId: "model-test", thinkingLevel: "high", runOptions: { performance: "fast" }, maxLlmCalls: 6, timeoutMs: 45000, researchFocus: "", relationHints: [], anchors: [], fastModel: null } }));
  expect(await screen.findByText("Ready to process")).toBeVisible();
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
  expect(screen.getByRole("checkbox")).not.toBeChecked();
  rerender(<WorldProcessingSettings {...props} />);
  expect(await screen.findByRole("button", { name: /World processing model/ })).toHaveTextContent("GPT-5.6 Luna · High · Fast");
});

it("offers provider management when there are no connected models without enabling processing", async () => {
  const props = setup();
  vi.spyOn(props.bridge, "modelCatalogGet").mockResolvedValue({ models: [], providers: [], hidden_model_profile_ids: [] });
  const save = vi.spyOn(props.bridge, "knowledgeGraphSettingsSet");
  render(<WorldProcessingSettings {...props} />);
  const user = userEvent.setup();
  expect(await screen.findByText(/No connected models/)).toBeVisible();
  expect(screen.queryByRole("button", { name: /World processing model/ })).toBeNull();
  expect(screen.getByRole("checkbox")).not.toBeChecked();
  await user.click(screen.getByRole("button", { name: "Manage providers" }));
  expect(props.onManageProviders).toHaveBeenCalledOnce();
  await user.click(screen.getByRole("checkbox"));
  expect(screen.getByRole("button", { name: "Save processing settings" })).toBeDisabled();
  expect(save).not.toHaveBeenCalled();
});

it("shows host readiness for a disconnected provider", async () => {
  const [status, label] = ["provider_disconnected", "Reconnect provider"] as const;
  const props = setup();
  const view = await props.bridge.knowledgeGraphSettingsGet({ loupistId: props.loupistId });
  vi.spyOn(props.bridge, "knowledgeGraphSettingsGet").mockResolvedValue({ ...view, settings: { ...view.settings, enrichmentEnabled: true, helperModelProfileId: "removed-model" }, readiness: { status, message: "Reconnect or select an available model.", modelName: "Saved model", providerName: "Saved provider", authKind: "oauth" } });
  render(<WorldProcessingSettings {...props} />);
  const readiness = await screen.findByLabelText("World processing readiness");
  expect(within(readiness).getByText(label)).toBeVisible();
  expect(readiness).toHaveTextContent("Saved model");
  expect(screen.getByText(/The saved model is unavailable/)).toBeVisible();
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
  vi.spyOn(props.bridge, "modelCatalogGet").mockResolvedValue({ models: [], providers: [], hidden_model_profile_ids: [] });
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
  expect(screen.getByRole("checkbox")).not.toBeChecked();
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
  await user.click(screen.getByRole("button", { name: "Discard changes" }));
  expect(screen.getByLabelText("Key companies and topics")).toHaveValue("Samsung | 삼성 | 005930");
});
