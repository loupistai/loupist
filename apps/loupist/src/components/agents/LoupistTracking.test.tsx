import type { LoupistBridge } from "../../contracts/bridge";
import i18n, { resources } from "../../i18n";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { LoupistTracking } from "./LoupistTracking";
import type { ExtensionRelation, ExtensionType, SchemaChange, SchemaLabels, WorldSchemaView } from "../../contracts/knowledge";

type WorldSchemaApi = Pick<LoupistBridge, "worldSchemaGet" | "worldSchemaUndo" | "worldSchemaRemove" | "onWorldSchemaChanged">;

afterEach(async () => { await i18n.changeLanguage("en"); });

function trackedType(name: string, label: string, category: ExtensionType["category"], ko?: string, blurbs?: SchemaLabels): ExtensionType {
  return {
    name,
    description: `${label} is a kind of thing this Loupist keeps track of.`,
    category,
    labels: ko ? { en: label, ko } : { en: label },
    blurbs: blurbs ?? { en: `${label} in brief` },
    fields: [],
  };
}

function change(id: string, producer: SchemaChange["producer"], addedTypes: ExtensionType[], undoable = true, removedTypes: ExtensionType[] = []): SchemaChange {
  const labelled = [...addedTypes, ...removedTypes].map(type => ({ name: type.name, labels: type.labels }));
  return {
    changeId: id,
    producer,
    summary: "",
    addedTypes: addedTypes.map(type => type.name),
    addedRelations: [],
    removedTypes: removedTypes.map(type => type.name),
    removedRelations: [],
    kindLabels: labelled,
    createdAtMs: Date.UTC(2026, 9, 30),
    undoable,
  };
}

function schema(types: ExtensionType[] = [], relations: ExtensionRelation[] = [], changes: SchemaChange[] = []): WorldSchemaView {
  return { extension: { version: 1, types, relations, updatedAtMs: 1 }, changes };
}

function trackingApi(initial: WorldSchemaView, impl: Partial<WorldSchemaApi> = {}) {
  let current = structuredClone(initial);
  const bridge = {
    worldSchemaGet: vi.fn(async () => structuredClone(current)),
    worldSchemaUndo: vi.fn(async () => structuredClone(current)),
    worldSchemaRemove: vi.fn(async () => structuredClone(current)),
    onWorldSchemaChanged: vi.fn(() => () => {}),
    ...impl,
  } as WorldSchemaApi;
  return { bridge: bridge as unknown as LoupistBridge, replace: (next: WorldSchemaView) => { current = next; } };
}

it("shows only the basics once nothing extra is tracked", async () => {
  let resolveGet: (view: WorldSchemaView) => void = () => {};
  const { bridge } = trackingApi(schema(), { worldSchemaGet: vi.fn(() => new Promise<WorldSchemaView>(resolve => { resolveGet = resolve; })) });
  render(<LoupistTracking bridge={bridge} loupistId="loupist-quality" />);
  expect(screen.queryByText(/Only the basics/)).toBeNull();
  await act(async () => { resolveGet(schema()); });
  expect(screen.getByRole("heading", { name: "What the map tracks" })).toBeVisible();
  expect(screen.getByText(/Only the basics so far/)).toBeVisible();
  expect(screen.queryByRole("button", { name: /Undo/ })).toBeNull();
});
it("renders Korean labels and only a Korean blurb in Korean", async () => {
  const process = trackedType("ProcessNode", "Process node", "product", "공정 노드", { en: "Process generations such as N3 or 18A", ko: "N3, 18A 같은 반도체 공정 세대" });
  const bare = trackedType("BareType", "Bare type", "market", "기본", { en: "English only blurb" });
  const relations: ExtensionRelation[] = [
    { name: "QUALIFIED_BY", description: "Links a company to the process node it has qualified.", signatures: [["Organization", "ProcessNode"]], labels: { en: "qualified by", ko: "인증" } },
    { name: "MADE_ON", description: "Links a product to the process node it was manufactured on.", signatures: [["Product", "ProcessNode"]], labels: { en: "made on" } },
  ];
  await act(async () => { await i18n.changeLanguage("ko"); });
  render(<LoupistTracking bridge={trackingApi(schema([process, bare], relations, [change("c1", "preset", [process, bare])])).bridge} loupistId="loupist-quality" />);
  expect(await screen.findAllByText("공정 노드")).not.toHaveLength(0);
  expect(screen.getByText("N3, 18A 같은 반도체 공정 세대")).toBeVisible();
  expect(screen.queryByText("English only blurb")).toBeNull();
  expect(screen.getByText("연결: 인증 · made on")).toBeVisible();
  expect(screen.getByText("공정 노드, 기본 추가")).toBeVisible();
});
it("moves focus to the next stop control, then to the heading", async () => {
  const alpha = trackedType("AlphaType", "Alpha", "company");
  const beta = trackedType("BetaType", "Beta", "event");
  let resolveBeta: (view: WorldSchemaView) => void = () => {};
  const remove = vi.fn(async ({ name }: { name: string }) => {
    if (name === "BetaType") return new Promise<WorldSchemaView>(resolve => { resolveBeta = resolve; });
    return schema([beta]);
  });
  const { bridge } = trackingApi(schema([alpha, beta]), { worldSchemaRemove: remove });
  render(<LoupistTracking bridge={bridge} loupistId="loupist-quality" />);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "Stop tracking Alpha" }));
  const betaButton = screen.getByRole("button", { name: "Stop tracking Beta" });
  expect(betaButton).toHaveFocus();
  await user.click(betaButton);
  expect(betaButton).toBeDisabled();
  expect(remove).toHaveBeenLastCalledWith({ loupistId: "loupist-quality", kind: "type", name: "BetaType" });
  await act(async () => { resolveBeta(schema()); });
  expect(screen.getByRole("heading", { name: "What the map tracks" })).toHaveFocus();
});
it("lists recent changes and undoes one while the others stay", async () => {
  const process = trackedType("ProcessNode", "Process node", "product");
  const capacity = trackedType("CapacityPlan", "Capacity plan", "event", "증설 계획");
  const view = schema([process, capacity], [], [change("c2", "review", [capacity]), change("c1", "preset", [process]), change("c0", "chat", [], false, [trackedType("Gone", "Old kind", "other")])]);
  let resolveUndo: (next: WorldSchemaView) => void = () => {};
  const undo = vi.fn(() => new Promise<WorldSchemaView>(resolve => { resolveUndo = resolve; }));
  const { bridge } = trackingApi(view, { worldSchemaUndo: undo });
  render(<LoupistTracking bridge={bridge} loupistId="loupist-quality" />);
  const user = userEvent.setup();
  expect(await screen.findByText("Added Capacity plan")).toBeVisible();
  expect(screen.getByText("Removed Old kind")).toBeVisible();
  expect(screen.getByText(/Noticed in your sources/)).toBeVisible();
  expect(screen.getAllByRole("button", { name: /^Undo:/ })).toHaveLength(2);
  await user.click(screen.getByRole("button", { name: "Undo: Added Capacity plan" }));
  expect(undo).toHaveBeenCalledWith({ loupistId: "loupist-quality", changeId: "c2" });
  expect(screen.getByRole("button", { name: "Undo: Added Process node" })).toBeDisabled();
  await act(async () => { resolveUndo(schema([process], [], [change("c3", "undo", [], true, [capacity]), change("c2", "review", [capacity], false), change("c1", "preset", [process])])); });
  expect(screen.getByRole("status")).toHaveTextContent("Undone.");
  expect(screen.getByRole("heading", { name: "What the map tracks" })).toHaveFocus();
  expect(screen.queryByRole("button", { name: "Undo: Added Capacity plan" })).toBeNull();
  expect(screen.getByRole("button", { name: "Undo: Removed Capacity plan" })).toBeVisible();
});
it("offers no Apply or Dismiss for a suggestion", async () => {
  render(<LoupistTracking bridge={trackingApi(schema([], [], [])).bridge} loupistId="loupist-quality" />);
  await screen.findByText(/Only the basics/);
  expect(screen.queryByRole("button", { name: /Track these|Not now|Apply|Dismiss/ })).toBeNull();
});
it("shows a load error and retries", async () => {
  const loaded = schema([trackedType("ProcessNode", "Process node", "product")]);
  const get = vi.fn().mockRejectedValueOnce(new Error("offline")).mockResolvedValueOnce(loaded);
  const { bridge } = trackingApi(schema(), { worldSchemaGet: get });
  render(<LoupistTracking bridge={bridge} loupistId="loupist-quality" />);
  const user = userEvent.setup();
  expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load what the map tracks.");
  expect(screen.queryByText(/Only the basics/)).toBeNull();
  await user.click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByText("Process node")).toBeVisible();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(get).toHaveBeenCalledTimes(2);
});
it("keeps the list when a change cannot be saved", async () => {
  const alpha = trackedType("AlphaType", "Alpha", "market");
  const remove = vi.fn(async () => { throw new Error("nope"); });
  const { bridge } = trackingApi(schema([alpha]), { worldSchemaRemove: remove });
  render(<LoupistTracking bridge={bridge} loupistId="loupist-quality" />);
  const user = userEvent.setup();
  const button = await screen.findByRole("button", { name: "Stop tracking Alpha" });
  await user.click(button);
  expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't save that change.");
  expect(screen.getByText("Alpha")).toBeVisible();
  expect(button).toBeEnabled();
  expect(screen.queryByRole("status")).toBeNull();
});
it("reloads when this Loupist's vocabulary changes and unsubscribes on unmount", async () => {
  let current = schema();
  const handlers = new Set<(event: { loupistId: string }) => void>();
  const unsubscribe = vi.fn();
  const get = vi.fn(async () => structuredClone(current));
  const bridge = {
    worldSchemaGet: get,
    worldSchemaUndo: vi.fn(),
    worldSchemaRemove: vi.fn(),
    onWorldSchemaChanged: (handler: (event: { loupistId: string }) => void) => {
      handlers.add(handler);
      return () => { unsubscribe(); handlers.delete(handler); };
    },
  } as unknown as LoupistBridge;
  const view = render(<LoupistTracking bridge={bridge} loupistId="loupist-quality" />);
  expect(await screen.findByText(/Only the basics so far/)).toBeVisible();
  current = schema([trackedType("ProcessNode", "Process node", "product")]);
  await act(async () => { handlers.forEach(handler => handler({ loupistId: "someone-else" })); });
  expect(get).toHaveBeenCalledTimes(1);
  await act(async () => { handlers.forEach(handler => handler({ loupistId: "loupist-quality" })); });
  expect(await screen.findByText("Process node")).toBeVisible();
  expect(screen.queryByText(/Only the basics/)).toBeNull();
  expect(get).toHaveBeenCalledTimes(2);
  view.unmount();
  expect(unsubscribe).toHaveBeenCalledOnce();
  current = schema([trackedType("OtherType", "Other", "other")]);
  await act(async () => { handlers.forEach(handler => handler({ loupistId: "loupist-quality" })); });
  expect(get).toHaveBeenCalledTimes(2);
});
it("never uses technical vocabulary words in what users read", () => {
  const strings: string[] = [];
  const walk = (value: unknown) => { if (typeof value === "string") strings.push(value); else if (value && typeof value === "object") Object.values(value).forEach(walk); };
  for (const language of ["en", "ko"] as const) {
    walk(resources[language].app.tracking);
    walk(resources[language].chat.vocabulary);
  }
  expect(strings.filter(text => /ontology|schema|entity type|온톨로지|스키마/i.test(text))).toEqual([]);
});
