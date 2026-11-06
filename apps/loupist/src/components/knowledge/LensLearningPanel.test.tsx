import { act, render, screen } from "@testing-library/react";
import { PreviewBridge, prepareLearningFixture } from "../../preview/fixtures";
import { LensLearningPanel } from "./LensLearningPanel";

async function fixture() {
  const bridge = new PreviewBridge({ delay_ms: 0 });
  const review = await prepareLearningFixture(bridge);
  const node = { kind: "lens_method", recordId: review.lensRecordId! } satisfies Parameters<typeof LensLearningPanel>[0]["node"];
  const props = { bridge, loupistId: "loupist-quality", node, onSelect: vi.fn(), onArchived: vi.fn() };
  return { bridge, review, props };
}

it("uses the review named by the saved change rather than a later unbound review", async () => {
  const { bridge, review, props } = await fixture();
  vi.spyOn(bridge, "learningReviewList").mockResolvedValue([
    { ...review, reviewId: "unbound-review", createdAtMs: review.createdAtMs + 1000, rationale: "An unrelated later rationale." },
    review,
  ]);
  render(<LensLearningPanel {...props} />);
  expect(await screen.findByRole("region", { name: "Why this candidate was saved" })).toHaveTextContent(review.rationale!);
  expect(screen.queryByText("An unrelated later rationale.")).toBeNull();
  expect(screen.queryByText(/new facts/)).toBeNull();
});

it("keeps the committed rationale when its named review is outside the recent review window", async () => {
  const { bridge, review, props } = await fixture();
  vi.spyOn(bridge, "learningReviewList").mockResolvedValue([
    { ...review, reviewId: "unbound-review", rationale: "An unrelated later rationale." },
  ]);
  render(<LensLearningPanel {...props} />);
  expect(await screen.findByRole("region", { name: "Why this candidate was saved" })).toHaveTextContent(review.rationale!);
  expect(screen.queryByText("An unrelated later rationale.")).toBeNull();
});

it("does not present review admission as a saved publication when publication provenance is missing", async () => {
  const { bridge, props } = await fixture();
  vi.spyOn(bridge, "knowledgeLensChanges").mockResolvedValue([]);
  await act(async () => { render(<LensLearningPanel {...props} />); });
  expect(screen.queryByRole("region", { name: "Why this candidate was saved" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Mark as seen" })).toBeNull();
});
