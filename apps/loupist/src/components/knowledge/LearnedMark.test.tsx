import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { OpenProvider } from "../navigation/Open";
import { LearnedMark } from "./LearnedMark";

const learned = { atMs: Date.UTC(2026, 10, 3, 12), taskId: "learn-kyushu", runId: "run-7", programTitle: "Kyushu port throughput" };

it("says when and by which program a section was learned, and opens that run in Schedule", async () => {
  const open = vi.fn(async () => {});
  render(<OpenProvider open={open}><LearnedMark learned={learned} /></OpenProvider>);
  const chip = screen.getByRole("link", { name: "Learned Nov 3 · Kyushu port throughput" });
  expect(chip).toHaveAccessibleDescription("Opens this run in Schedule");
  await userEvent.setup().click(chip);
  expect(open).toHaveBeenCalledWith({ kind: "run", runId: "run-7", taskId: "learn-kyushu" });
});

it("is plain text outside a navigator and absent without the field", () => {
  const { container, rerender } = render(<LearnedMark learned={learned} />);
  expect(screen.queryByRole("link")).toBeNull();
  expect(screen.getByText("Learned Nov 3 · Kyushu port throughput")).toBeVisible();
  rerender(<LearnedMark />);
  expect(container).toBeEmptyDOMElement();
});
