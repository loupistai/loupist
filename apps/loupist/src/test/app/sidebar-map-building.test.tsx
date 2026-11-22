import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { chooseLoupist } from "../renderApp";

const LOUPIST = "loupist-quality";

function mapJobs(bridge: MockLoupistBridge, initial: string[]) {
  let statuses = initial;
  let recovering = false;
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
    return recovering && request.loupistId === LOUPIST
      ? { ...view, readiness: { ...view.readiness, status: "recovering", note: "recovering" } }
      : view;
  });
  const listeners = new Set<Parameters<MockLoupistBridge["onWorldSchemaChanged"]>[0]>();
  vi.spyOn(bridge, "onWorldSchemaChanged").mockImplementation(listener => {
    listeners.add(listener);
    return () => { listeners.delete(listener); };
  });
  return async (next: string[], recovery = false) => {
    statuses = next;
    recovering = recovery;
    await act(async () => { for (const listener of listeners) listener({ loupistId: LOUPIST }); });
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
  await jobs(["running", "queued"]);
  const running = await mapRow("World · building from 2 sources");
  await user.click(running);
  await user.click(await screen.findByRole("button", { name: "Library" }));
  await user.click(await screen.findByRole("tab", { name: "Sources" }));
  expect(await screen.findByRole("radio", { name: "Processing · 2" })).toBeVisible();
  await jobs(["paused", "failed_before_mutation"]);
  const idle = await mapRow("World");
  expect(idle.querySelector("[data-slot=spinner]")).toBeNull();
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
+  await jobs([], true);
  const recovery = await mapRow("World · building");
  expect(recovery).toHaveAttribute("title", "World · building");
  expect(recovery.querySelector("[data-slot=spinner]")).toHaveClass("thread-running");
  await jobs(["disabled", "cancelled_before_start"]);
  expect((await mapRow("World")).querySelector("[data-slot=spinner]")).toBeNull();
});
