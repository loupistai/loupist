import { render, screen } from "@testing-library/react";
import type { ScheduledTaskListItem } from "../../types";
import { BotStandingStrip } from "./BotStandingStrip";

const loupist = { id: "research", name: "Research" };
const task = (id: string, quiet: boolean) => ({
  id, kind: "custom", status: "active", report: "when_met", title: "Filings and results",
  latest_run: { started_at_ms: Date.now(), status: "succeeded", quiet },
}) as ScheduledTaskListItem;

it("uses host snapshots to distinguish an empty Bot, quiet checks, and a reported result", () => {
  const empty = new Map();
  const quiet = task("quiet", true);
  const reported = task("reported", false);
  const { rerender } = render(<BotStandingStrip loupist={loupist} tasks={empty} />);
  expect(screen.getByRole("status")).toHaveTextContent("Nothing runs on its own yet");
  rerender(<BotStandingStrip loupist={loupist} tasks={new Map([[quiet.id, quiet]])} />);
  expect(screen.getByRole("status")).toHaveTextContent("All quiet");
  rerender(<BotStandingStrip loupist={loupist} tasks={new Map([[quiet.id, quiet], [reported.id, reported]])} />);
  expect(screen.getByRole("status")).toHaveTextContent("2 watches");
  expect(screen.getByRole("status")).not.toHaveTextContent("All quiet");
});
