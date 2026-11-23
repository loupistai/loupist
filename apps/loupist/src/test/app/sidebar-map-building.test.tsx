import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { tabShortcut } from "../../app/shortcuts";
import type { WorldReadinessStatus } from "../../knowledge/worldReadiness";
import type { GraphReadinessNote } from "../../types";
import { chooseLoupist } from "../renderApp";

const LOUPIST = "loupist-quality";

function mapJobs(bridge: MockLoupistBridge, initial: string[], initialReadiness: WorldReadinessStatus = "ready", note: GraphReadinessNote | null = null) {
  let statuses = initial;
  let readiness = initialReadiness;
  const catalog = bridge.knowledgeCatalog.bind(bridge);
  vi.spyOn(bridge, "knowledgeCatalog").mockImplementation(async request => {
    const view = await catalog(request);
    if (request.loupistId !== LOUPIST) return view;
    let index = 0;
    return { ...view, sources: view.sources.map(source => ({
      ...source, objects: source.objects.map(object => {
        const status = statuses[index++];
        return { ...object, processing: status ? { jobId: object.id, status, error: null } : undefined };
      }),
    })) };
  });
  const graph = bridge.knowledgeGraphSettingsGet.bind(bridge);
  vi.spyOn(bridge, "knowledgeGraphSettingsGet").mockImplementation(async request => {
    const view = await graph(request);
    return request.loupistId === LOUPIST
      ? { ...view, readiness: { ...view.readiness, status: readiness, note } }
      : view;
  });
  const listeners = new Set<Parameters<MockLoupistBridge["onWorldSchemaChanged"]>[0]>();
  vi.spyOn(bridge, "onWorldSchemaChanged").mockImplementation(listener => {
    listeners.add(listener);
    return () => { listeners.delete(listener); };
  });
  return {
    async change(next: string[], status: WorldReadinessStatus = "ready") {
      statuses = next;
      readiness = status;
      note = null;
      await act(async () => { for (const listener of listeners) listener({ loupistId: LOUPIST }); });
    },
    async connectProvider() {
      readiness = "ready";
      await act(async () => bridge.setProviderStatus("openai", { state: "connected" }));
    },
  };
}

const mapRow = (name: string) => screen.findByRole("button", { name }, { timeout: 5_000 });

it("shows queued World work on startup and shares the running Source count with the Library", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const jobs = mapJobs(bridge, ["queued"]);
  render(<App bridge={bridge} />);
  const queued = await mapRow("World · building from 1 source");
  expect(queued).toHaveAttribute("title", "World · building from 1 source");
  expect(queued.querySelector("[data-slot=spinner]")).toHaveClass("thread-running", "animate-spin");
  expect(queued.querySelector("[role=status]")).toBeNull();
  await jobs.change(["running", "queued"]);
  const running = await mapRow("World · building from 2 sources");
  await user.click(running);
  await user.click(await screen.findByRole("button", { name: "Library" }));
  await user.click(await screen.findByRole("tab", { name: "Sources" }));
  expect(await screen.findByRole("radio", { name: "Processing · 2" })).toBeVisible();
  await jobs.change(["paused", "failed_before_mutation"]);
  const stopped = await mapRow("World building failed");
  expect(stopped.querySelector("[data-slot=spinner]")).toBeNull();
});

it("keeps World spinner in the collapsed rail and switches to the next Loupist's state", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [first, second] = await bridge.loupistList();
  mapJobs(bridge, ["running"]);
  render(<App bridge={bridge} />);
  await mapRow("World · building from 1 source");
  act(() => bridge.emitMenuAction("toggle-sidebar"));
  const railRow = await mapRow("World · building from 1 source");
  expect(railRow.closest('[data-collapsible="icon"]')).toBeInTheDocument();
  expect(railRow.querySelector("[data-slot=spinner]")).toHaveClass("thread-running");
  await chooseLoupist(user, second!.name);
  expect((await mapRow("World")).querySelector("[data-slot=spinner]")).toBeNull();
  await chooseLoupist(user, first!.name);
  expect((await mapRow("World · building from 1 source")).querySelector("[data-slot=spinner]")).toBeVisible();
});

it("shows recovery without a Source count and clears it when the host settles", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const jobs = mapJobs(bridge, []);
  render(<App bridge={bridge} />);
  await mapRow("World");
  await jobs.change([], "recovering");
  const recovery = await mapRow("World · building");
  expect(recovery).toHaveAttribute("title", "World · building");
  expect(recovery.querySelector("[data-slot=spinner]")).toHaveClass("thread-running");
  await jobs.change(["disabled", "cancelled_before_start"]);
  expect((await mapRow("World")).querySelector("[data-slot=spinner]")).toBeNull();
});

const providerMessage = "World is waiting for a model: connect the one chosen for World in Settings.";
const accountMessage = "World is waiting for an AI account. Connect one in Settings > AI accounts.";
// World's own lines count the Sources stopped at the limit; each Source's row keeps its own sentence.
const allowanceMessage = "1 Source paused at the request limit";

it.each([
  { readiness: "provider_disconnected", statuses: ["queued"], label: "World · waiting for a model", message: providerMessage },
  { readiness: "paused", statuses: ["paused"], label: "World · building paused", message: "World building paused" },
  { readiness: "disabled", statuses: ["queued"], label: "World · building is off", message: "World building is off" },
  { readiness: "ready", statuses: ["paused_budget"], label: allowanceMessage, message: allowanceMessage, continueAll: true, control: "World building settings" },
  { readiness: "ready", statuses: ["failed_before_mutation"], label: "World building failed", message: "World building failed" },
  // No AI account at all, and accounts whose models exclude World's.
  { readiness: "model_unavailable", note: "no_account", statuses: ["queued"], label: accountMessage, message: accountMessage, control: "Manage AI accounts" },
  { readiness: "model_unavailable", note: "model_unavailable", statuses: ["queued"], label: "Choose another AI model", message: "Choose another AI model" },
] as { readiness: WorldReadinessStatus; note?: GraphReadinessNote; statuses: string[]; label: string; message: string; continueAll?: boolean; control?: string }[])("keeps the sidebar and Library still when World waits ($label)", async ({ readiness, note, statuses, label, message, continueAll, control }) => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  mapJobs(bridge, [...statuses], readiness, note ?? null);
  render(<App bridge={bridge} />);
  const row = await mapRow(label);
  expect(row).toHaveAttribute("title", label);
  expect(row.querySelector("[data-slot=spinner]")).toBeNull();
  expect(row.querySelector("kbd")).toHaveTextContent(tabShortcut(3));

  await user.click(row);
  const stats = await screen.findByTestId("map-stats");
  expect(stats).toHaveTextContent(message);
  expect(stats.querySelector(".map-updating")).toBeNull();
  // A reason the user can act on carries the control that opens the right settings.
  expect(within(stats).queryAllByRole("button").map(button => button.textContent)).toEqual([...(continueAll ? ["Continue all"] : []), ...(control ? [control] : [])]);
  await user.click(await screen.findByRole("button", { name: "Library" }));
  await user.click(await screen.findByRole("tab", { name: "Sources" }));
  const library = within(screen.getByTestId("map-library"));
  expect(await library.findByRole("status")).toHaveTextContent(message);
  expect(library.queryByText("World is updating")).toBeNull();
  expect(library.getByRole("radio", { name: "Processing" })).toBeVisible();
  expect(library.queryByRole("button", { name: "Manage AI accounts" }) !== null).toBe(control === "Manage AI accounts");
  if (readiness === "provider_disconnected" || control === "World building settings") {
    await user.click(library.getByRole("button", { name: "World building settings" }));
    expect(await screen.findByRole("form", { name: "World building settings" })).toBeVisible();
  }
  if (control === "Manage AI accounts") {
    await user.click(library.getByRole("button", { name: "Manage AI accounts" }));
    expect(await screen.findByRole("heading", { name: "AI accounts", level: 1 })).toBeVisible();
  }
});

it("offers the recommended model where World says a chosen model is unavailable", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const { settings } = await bridge.knowledgeGraphSettingsGet({ loupistId: LOUPIST });
  await bridge.knowledgeGraphSettingsSet({ loupistId: LOUPIST, settings: { ...settings, enrichmentEnabled: true, worldModelChoice: "chosen", helperModelProfileId: "anthropic/claude-gone" } });
  mapJobs(bridge, ["queued"], "model_unavailable", "model_unavailable");
  const save = vi.spyOn(bridge, "knowledgeGraphSettingsSet");
  render(<App bridge={bridge} />);
  await user.click(await mapRow("Choose another AI model"));
  const stats = await screen.findByTestId("map-stats");
  expect(stats).toHaveTextContent("Choose another AI model");
  await user.click(within(stats).getByRole("button", { name: "Use the recommended model" }));
  await waitFor(() => expect(save).toHaveBeenCalledExactlyOnceWith({ loupistId: LOUPIST, settings: expect.objectContaining({ worldModelChoice: "recommended" }) }));
  await waitFor(() => expect(within(screen.getByTestId("map-stats")).queryByRole("button", { name: "Use the recommended model" })).toBeNull());
  expect((await bridge.knowledgeGraphSettingsGet({ loupistId: LOUPIST })).settings).toMatchObject({ worldModelChoice: "recommended", helperModelProfileId: "model-test" });
});

it("keeps a running World job visible even when the provider blocks the next Source", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  mapJobs(bridge, ["running", "queued"], "provider_disconnected");
  render(<App bridge={bridge} />);
  const row = await mapRow("World · building from 1 source");
  expect(row.querySelector("[data-slot=spinner]")).toBeVisible();
  await user.click(row);
  expect(await screen.findByText("Updating…")).toBeVisible();
  await user.click(await screen.findByRole("button", { name: "Library" }));
  await user.click(await screen.findByRole("tab", { name: "Sources" }));
  const library = within(screen.getByTestId("map-library"));
  const updating = await library.findAllByText("World is updating");
  expect(updating).toHaveLength(1);
  expect(updating[0]?.querySelector("[data-slot=spinner]")).toBeVisible();
  expect(library.getByRole("radio", { name: "Processing · 1" })).toBeVisible();
});

it("resumes the queued World status on a provider event with the World still closed", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const jobs = mapJobs(bridge, ["queued"], "provider_disconnected");
  render(<App bridge={bridge} />);
  await mapRow("World · waiting for a model");
  await jobs.connectProvider();
  expect((await mapRow("World · building from 1 source")).querySelector("[data-slot=spinner]")).toBeVisible();
  expect(screen.queryByTestId("map-view")).toBeNull();
});

it("continues every Source paused at the request limit in one confirmed step", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const jobs = mapJobs(bridge, ["paused_budget", "paused_budget"]);
  const stopped = (await bridge.knowledgeCatalog({ loupistId: LOUPIST })).sources.slice(0, 2).map(source => source.recordId);
  const retry = vi.spyOn(bridge, "knowledgeGraphRetrySources").mockResolvedValue({ continued: 2, skipped: 0 });
  render(<App bridge={bridge} />);
  await user.click(await mapRow("2 Sources paused at the request limit"));
  const stats = await screen.findByTestId("map-stats");
  expect(stats).toHaveTextContent("2 Sources paused at the request limit");

  // The confirmation names the count and the most requests it can spend; Cancel runs nothing.
  await user.click(within(stats).getByRole("button", { name: "Continue all" }));
  let dialog = await screen.findByRole("dialog", { name: "Continue reading 2 Sources?" });
  expect(dialog).toHaveTextContent("This can use up to 64 model requests.");
  await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(retry).not.toHaveBeenCalled();

  await user.click(within(stats).getByRole("button", { name: "Continue all" }));
  dialog = await screen.findByRole("dialog", { name: "Continue reading 2 Sources?" });
  await user.click(within(dialog).getByRole("button", { name: "Continue" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(retry).toHaveBeenCalledExactlyOnceWith({ loupistId: LOUPIST, recordIds: stopped, maxLlmCalls: 32 });
  await jobs.change(["queued", "queued"]);
  expect(await mapRow("World · building from 2 sources")).toBeVisible();
  expect(within(screen.getByTestId("map-stats")).queryByRole("button", { name: "Continue all" })).toBeNull();
});
