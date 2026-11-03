import type { LoupistBridge } from "../../contracts/bridge";
import i18n from "../../i18n";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { LoupistTracking } from "./LoupistTracking";
import type { ExtensionRelation, ExtensionType, SchemaLabels, SchemaProposal, WorldSchemaView } from "../../contracts/knowledge";

type WorldSchemaApi = Pick<LoupistBridge, "worldSchemaGet" | "worldSchemaApply" | "worldSchemaDismiss" | "worldSchemaRemove" | "onWorldSchemaChanged">;

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

function schema(types: ExtensionType[] = [], relations: ExtensionRelation[] = [], proposal: SchemaProposal | null = null): WorldSchemaView {
  return { extension: { version: 1, types, relations, updatedAtMs: 1 }, proposal };
}

function trackingApi(initial: WorldSchemaView, impl: Partial<WorldSchemaApi> = {}) {
  let current = structuredClone(initial);
  const bridge = {
    worldSchemaGet: vi.fn(async () => structuredClone(current)),
    worldSchemaApply: vi.fn(async () => structuredClone(current)),
    worldSchemaDismiss: vi.fn(async () => structuredClone(current)),
    worldSchemaRemove: vi.fn(async () => structuredClone(current)),
    onWorldSchemaChanged: vi.fn(() => () => {}),
    ...impl,
  } as WorldSchemaApi;
  return { bridge: bridge as unknown as LoupistBridge, replace: (next: WorldSchemaView) => { current = next; } };
}

const suggestedRelation: ExtensionRelation = {
  name: "EXPANDS_CAPACITY",
  description: "Links a capacity plan to the process node or site whose output it expands.",
  signatures: [["CapacityPlan", "ProcessNode"]],
  labels: { en: "expands capacity", ko: "증설" },
};
it("shows only the basics once nothing extra is tracked", async () => {
  let resolveGet: (view: WorldSchemaView) => void = () => {};
  const { bridge } = trackingApi(schema(), { worldSchemaGet: vi.fn(() => new Promise<WorldSchemaView>(resolve => { resolveGet = resolve; })) });
  render(<LoupistTracking bridge={bridge} loupistId="loupist-quality" />);
  expect(screen.queryByText(/Only the basics/)).toBeNull();
  await act(async () => { resolveGet(schema()); });
  expect(screen.getByText("Only the basics so far. Your Loupist suggests more after it reads about 10 sources.")).toBeVisible();
  expect(screen.queryByRole("button", { name: "Track these" })).toBeNull();
});
it("renders Korean labels and only a Korean blurb in Korean", async () => {
  const process = trackedType("ProcessNode", "Process node", "product", "공정 노드", { en: "Process generations such as N3 or 18A", ko: "N3, 18A 같은 반도체 공정 세대" });
  const bare = trackedType("BareType", "Bare type", "market", "기본", { en: "English only blurb" });
  const relations: ExtensionRelation[] = [
    { name: "QUALIFIED_BY", description: "Links a company to the process node it has qualified.", signatures: [["Organization", "ProcessNode"]], labels: { en: "qualified by", ko: "인증" } },
    { name: "MADE_ON", description: "Links a product to the process node it was manufactured on.", signatures: [["Product", "ProcessNode"]], labels: { en: "made on" } },
  ];
  await act(async () => { await i18n.changeLanguage("ko"); });
  render(<LoupistTracking bridge={trackingApi(schema([process, bare], relations)).bridge} loupistId="loupist-quality" />);
  expect(await screen.findByText("공정 노드")).toBeVisible();
  expect(screen.getByText("N3, 18A 같은 반도체 공정 세대")).toBeVisible();
  expect(screen.queryByText("English only blurb")).toBeNull();
  expect(screen.queryByText(process.description)).toBeNull();
  expect(screen.getByText("관계: 인증 · made on")).toBeVisible();
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
  expect(screen.getByRole("heading", { name: "What this Loupist tracks" })).toHaveFocus();
});
it("accepts and dismisses a suggestion", async () => {
  const existing = trackedType("ProcessNode", "Process node", "product");
  const suggested = trackedType("CapacityPlan", "Capacity plan", "event", "증설 계획", {
    en: "Announced plans to add production capacity",
    ko: "생산능력을 늘리는 증설 계획",
  });
  const proposal: SchemaProposal = { proposalId: "proposal-1", kind: "review", status: "pending", createdAtMs: 1, types: [suggested], relations: [suggestedRelation] };
  let resolveApply: (view: WorldSchemaView) => void = () => {};
  const apply = vi.fn(() => new Promise<WorldSchemaView>(resolve => { resolveApply = resolve; }));
  const dismiss = vi.fn(async () => schema([existing], [], { ...proposal, status: "dismissed" }));
  const { bridge } = trackingApi(schema([existing], [], proposal), { worldSchemaApply: apply, worldSchemaDismiss: dismiss });
  const user = userEvent.setup();
  const first = render(<LoupistTracking bridge={bridge} loupistId="loupist-quality" />);
  expect(await screen.findByText("Suggested from your sources")).toBeVisible();
  expect(screen.getByText("Announced plans to add production capacity")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Not now" }));
  expect(dismiss).toHaveBeenCalledWith({ loupistId: "loupist-quality", proposalId: "proposal-1" });
  expect(screen.queryByRole("button", { name: "Track these" })).toBeNull();
  expect(screen.getByText("Process node")).toBeVisible();
  expect(screen.getByRole("heading", { name: "What this Loupist tracks" })).toHaveFocus();
  first.unmount();

  render(<LoupistTracking bridge={bridge} loupistId="loupist-quality" />);
  await user.click(await screen.findByRole("button", { name: "Track these" }));
  expect(screen.getByRole("button", { name: "Track these" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Not now" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Stop tracking Process node" })).toBeDisabled();
  await act(async () => { resolveApply(schema([existing, suggested], [suggestedRelation], null)); });
  expect(apply).toHaveBeenCalledWith({ loupistId: "loupist-quality", proposalId: "proposal-1" });
  expect(screen.getByRole("status")).toHaveTextContent("Now tracking Capacity plan, expands capacity.");
  expect(screen.getByRole("heading", { name: "What this Loupist tracks" })).toHaveFocus();
  expect(screen.queryByRole("button", { name: "Track these" })).toBeNull();
  expect(screen.getByRole("button", { name: "Stop tracking Capacity plan" })).toBeVisible();
});
it("shows a load error and retries", async () => {
  const loaded = schema([trackedType("ProcessNode", "Process node", "product")]);
  const get = vi.fn().mockRejectedValueOnce(new Error("offline")).mockResolvedValueOnce(loaded);
  const { bridge } = trackingApi(schema(), { worldSchemaGet: get });
  render(<LoupistTracking bridge={bridge} loupistId="loupist-quality" />);
  const user = userEvent.setup();
  expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load what this Loupist tracks.");
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
it("reloads when this Loupist's schema changes and unsubscribes on unmount", async () => {
  let current = schema();
  const handlers = new Set<(event: { loupistId: string }) => void>();
  const unsubscribe = vi.fn();
  const get = vi.fn(async () => structuredClone(current));
  const bridge = {
    worldSchemaGet: get,
    worldSchemaApply: vi.fn(),
    worldSchemaDismiss: vi.fn(),
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
