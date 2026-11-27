import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { OpenProvider } from "../navigation/Open";
import { LoupistLearningSettings } from "./LoupistLearningSettings";

it("links to learning programs and names the default weekday time", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const open = vi.fn(async () => undefined);
  render(<OpenProvider open={open}><LoupistLearningSettings bridge={bridge} loupistId="loupist-quality" onChanged={vi.fn()} /></OpenProvider>);
  const link = await screen.findByRole("button", { name: "Learning programs" });
  expect(screen.getByText(/Default time:/)).toHaveTextContent(/Weekdays/);
  expect(screen.getByText(/Default time:/)).toHaveTextContent(/6:30/);
  await userEvent.setup().click(link);
  expect(open).toHaveBeenCalledWith({ kind: "schedule", taskId: "" });
});

it("saves the daily learning allowance and the program cap for this Loupist, says when learning is off, and shows the host's refusal under the field (plan 4.4)", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const user = userEvent.setup();
  render(<OpenProvider open={vi.fn(async () => undefined)}><LoupistLearningSettings bridge={bridge} loupistId="loupist-quality" onChanged={vi.fn()} /></OpenProvider>);
  const allowance = await screen.findByRole("spinbutton", { name: "Daily learning allowance" });
  await waitFor(() => expect(allowance).toHaveValue(132));
  expect(screen.getByRole("spinbutton", { name: "Active learning programs" })).toHaveValue(20);

  await user.clear(allowance);
  await user.type(allowance, "5");
  await user.click(screen.getByRole("button", { name: "Save Daily learning allowance" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Enter 0 to turn learning runs off, or at least 26 requests.");
  expect(allowance).toHaveAccessibleDescription(/at least 26 requests/);

  await user.clear(allowance);
  await user.type(allowance, "0");
  await user.click(screen.getByRole("button", { name: "Save Daily learning allowance" }));
  expect(await screen.findByText("Learning runs are off.")).toBeVisible();
  expect(screen.queryByRole("alert")).toBeNull();

  const cap = screen.getByRole("spinbutton", { name: "Active learning programs" });
  await user.clear(cap);
  await user.type(cap, "60");
  await user.click(screen.getByRole("button", { name: "Save Active learning programs" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Enter a number from 1 to 50.");
  await user.clear(cap);
  await user.type(cap, "8");
  await user.click(screen.getByRole("button", { name: "Save Active learning programs" }));
  await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  const saved = await bridge.configRead({ scope: { kind: "project", loupistId: "loupist-quality" } });
  expect(saved.effective).toMatchObject({ learning_daily_requests: 0, learning_program_cap: 8 });
});
