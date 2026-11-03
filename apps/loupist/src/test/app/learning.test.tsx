import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mainTab } from "../renderApp";
import { App } from "../../App";
import { PreviewBridge, prepareLearningFixture } from "../../preview/fixtures";
import { useLearningState } from "../../components/map/learningState";
vi.mock("../../components/map/MapCanvas", () => ({ default: ({ selectedId, learningGlowIds }: { selectedId?: string; learningGlowIds?: Set<string> }) => <div data-testid="learning-canvas" data-selected={selectedId} data-glow={[...(learningGlowIds ?? [])].join(",")} /> }));
async function fixture() {
  const bridge = new PreviewBridge({ delay_ms: 0 });
  const review = await prepareLearningFixture(bridge);
  const map = await bridge.knowledgeMap({ loupistId: "loupist-quality" });
  const node = map.nodes.find(node => node.recordId === review.lensRecordId)!;
  const appliedIds = map.edges.filter(edge => edge.kind === "applies_to" && edge.source === node.id).map(edge => edge.target);
  return { bridge, review, node, appliedIds };
}

it.each(["created", "refined"] as const)("announces a %s event, highlights its World entities, and opens the learned document in Lens", async status => {
  const { bridge, review, node, appliedIds } = await fixture();
  render(<App bridge={bridge} />);
  await screen.findByRole("button", { name: "Panel kind" });
  await userEvent.click(await mainTab(/Map/));
  await screen.findByTestId("learning-canvas");
  act(() => bridge.emitLearningReview({ ...review, status }));
  await screen.findByText(`${status === "created" ? "Your Loupist learned" : "Your Loupist refined"} a way of working: ${node.label}`);
  expect(appliedIds.length).toBeGreaterThan(0);
  expect(screen.getByTestId("learning-canvas")).toHaveAttribute("data-glow", appliedIds.join(","));
  await userEvent.click(screen.getByRole("button", { name: "View" }));
  expect(await screen.findByRole("heading", { name: node.label })).toBeVisible();
  expect(await screen.findByRole("region", { name: "Research principles" })).toBeVisible();
  expect(await screen.findByRole("complementary", { name: node.label })).toBeVisible();
  const provenance = await screen.findByRole("region", { name: "How your Loupist learned this" });
  expect(provenance).toHaveTextContent(review.rationale!);
  await userEvent.click(await within(provenance).findByRole("button", { name: "Show all 6" }));
  expect(within(provenance).getAllByRole("listitem")).toHaveLength(6);
  expect(await screen.findByRole("region", { name: "Then vs now" })).toBeVisible();
});

it("announces learning away from Map without starting a hidden glow", async () => {
  const { bridge, review, node } = await fixture();
  function State() { const state = useLearningState(bridge, "loupist-quality"); return <output data-testid="glow-state">{state.glow?.recordId ?? "none"}</output>; }
  render(<><App bridge={bridge} /><State /></>);
  await screen.findByRole("button", { name: "Panel kind" });
  act(() => bridge.emitLearningReview(review));
  await screen.findByText(`Your Loupist learned a way of working: ${node.label}`);
  expect(screen.getByTestId("glow-state")).toHaveTextContent("none");
  await userEvent.click(screen.getByRole("button", { name: "View" }));
  expect(await screen.findByRole("heading", { name: node.label })).toBeVisible();
});

