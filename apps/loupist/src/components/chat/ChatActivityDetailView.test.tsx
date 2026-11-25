import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatActivityDetail, LoupistBridge } from "../../types";
import { ChatActivityDetailView } from "./ChatActivityDetailView";

const detail: ChatActivityDetail = { id: "activity", tool_name: "run_python", status: "succeeded", input: "print(391)", output: "391", truncated: false, started_at_ms: 10, finished_at_ms: 20, file_path: "outputs/margin.png" };
it("ignores late activity from another thread", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  let resolve!: (value: ChatActivityDetail) => void;
  vi.spyOn(bridge, "chatActivityRead").mockReturnValueOnce(new Promise((done) => { resolve = done; })).mockResolvedValue({ ...detail, id: "other", output: "Other result" });
  const props = { bridge, loupistId: "loupist" };
  const { rerender } = render(<ChatActivityDetailView {...props} threadId="first" detailId="activity" />);
  rerender(<ChatActivityDetailView {...props} threadId="second" detailId="other" />);
  await screen.findByText("Other result");
  await act(async () => resolve(detail));
  expect(screen.queryByText("391")).not.toBeInTheDocument();
});

it("presents command parameters and failures without displaying the raw request envelope", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const input = JSON.stringify({ code: "print(391)", timeout_seconds: 30, working_directory: "research" });
  vi.spyOn(bridge, "chatActivityRead").mockResolvedValue({ ...detail, input, status: "failed", output: "No such file: prices.csv" });
  render(<ChatActivityDetailView bridge={bridge} loupistId="loupist" threadId="chat" detailId="activity" />);
  expect(await screen.findByText("No such file: prices.csv")).toBeVisible();
  expect(screen.getByText("print(391)", { selector: "pre" })).toBeVisible();
  expect(screen.getByRole("alert")).toBeInTheDocument();
  await user.click(screen.getByText("Details"));
  expect(screen.getByText("research", { selector: "dd" })).toBeVisible();
  expect(screen.queryByText(input)).not.toBeInTheDocument();
});

it("polls running activity, keeps saved output when a poll fails, and retries", async () => {
  vi.useFakeTimers();
  try {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const read = vi.spyOn(bridge, "chatActivityRead")
      .mockResolvedValueOnce({ ...detail, status: "running", output: "First rows…" })
      .mockRejectedValueOnce(new Error("Read failed"))
      .mockResolvedValueOnce(detail);
    render(<ChatActivityDetailView bridge={bridge} loupistId="loupist" threadId="chat" detailId="activity" />);
    await act(async () => { await Promise.resolve(); });
    expect(screen.getByText("First rows…")).toBeVisible();
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    expect(screen.getByRole("alert")).toHaveTextContent("Read failed");
    expect(screen.getByText("First rows…")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await act(async () => { await Promise.resolve(); });
    expect(screen.getByText("391")).toBeVisible();
    expect(read).toHaveBeenCalledTimes(3);
  } finally { vi.useRealTimers(); }
});

it("rejects an activity returned under the wrong detail ID", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "chatActivityRead").mockResolvedValue({ ...detail, id: "unexpected", output: "Wrong record" });
  render(<ChatActivityDetailView bridge={bridge} loupistId="loupist" threadId="chat" detailId="activity" />);
  expect(await screen.findByRole("alert")).toHaveTextContent("This activity is not available in this chat.");
  expect(screen.queryByText("Wrong record")).not.toBeInTheDocument();
});

it.each([
  { tool: "write", input: { path: "analysis.py", content: "Revenue: 391" }, summary: "Written content", text: "Revenue: 391" },
  { tool: "edit", input: { path: "analysis.py", oldText: "Revenue: 390", newText: "Revenue: 391" }, summary: "Requested change", text: "Revenue: 391" },
])("keeps $tool content inspectable without its JSON envelope", async ({ tool, input, summary, text }) => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "chatActivityRead").mockResolvedValue({ ...detail, tool_name: tool, input: JSON.stringify(input) });
  render(<ChatActivityDetailView bridge={bridge} loupistId="loupist" threadId="chat" detailId="activity" />);
  await userEvent.click(await screen.findByText(summary));
  expect(screen.getByText(text)).toBeVisible();
  expect(screen.queryByText(JSON.stringify(input))).not.toBeInTheDocument();
});

it("shows nested Python reads and replays the selected step from stored data", async () => {
  const python = { code: "print(42)", stdout: "42\n", stderr: "", status: "completed", rounds: 2,
    reads: [{ tool: "dart_financials", status: "ok", provider: "OpenDART", asOf: "2026-09-30" }] };
  const replay = vi.fn().mockResolvedValue({ step: python, matches: true, stdout: "42\n", stderr: "" });
  const bridge = { chatActivityRead: vi.fn().mockResolvedValue({
    id: "activity", tool_name: "run_python", input: '{}', output: "", status: "succeeded",
    python, python_message_id: "answer", step_id: "call-1",
  }), pythonStepReplay: replay } as unknown as LoupistBridge;
  render(<ChatActivityDetailView bridge={bridge} threadId="chat" loupistId="scope" detailId="activity" />);
  expect(await screen.findByRole("heading", { name: "Show the math" })).toBeInTheDocument();
  expect(screen.getByRole("region", { name: "Read from Python" })).toHaveTextContent("OpenDART");
  fireEvent.click(screen.getByRole("button", { name: "Run again on the same data" }));
  expect(replay).toHaveBeenCalledWith({ chatId: "chat", messageId: "answer", stepId: "call-1" });
  expect(await screen.findByText("Same output as the original run.")).toBeInTheDocument();
});
