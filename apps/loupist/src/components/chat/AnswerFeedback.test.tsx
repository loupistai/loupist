import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AnswerFeedback } from "./AnswerFeedback";
import { ChatMessageCard } from "./ChatMessageCard";
import type { ChatMessage } from "../../types";

const message: ChatMessage = { id: "answer", role: "assistant", content: "Profit rose 12%.", status: "complete", created_at: "2026-10-18T00:00:00Z" };
it("toggles ratings, saves and clears corrections, and teaches only on explicit click", async () => {
  const user = userEvent.setup();
  const onSave = vi.fn(async (_id, value) => ({ ...value, correction: value.correction?.trim() || null, updatedAt: 1 }));
  const onTeach = vi.fn();
  render(<AnswerFeedback message={message} onSave={onSave} onTeach={onTeach} />);
  await user.click(screen.getByRole("button", { name: "Helpful" }));
  expect(screen.getByRole("button", { name: "Helpful" })).toHaveAttribute("aria-pressed", "true");
  await user.click(screen.getByRole("button", { name: "Helpful" }));
  expect(onSave).toHaveBeenLastCalledWith("answer", { rating: null, correction: null });
  await user.click(screen.getByRole("button", { name: "Not helpful" }));
  await user.click(screen.getByRole("button", { name: "Suggest a correction" }));
  await user.type(screen.getByRole("textbox", { name: "Your correction" }), "Use consolidated profit.");
  await user.click(screen.getByRole("button", { name: "Save correction" }));
  expect(onSave).toHaveBeenLastCalledWith("answer", { rating: "down", correction: "Use consolidated profit." });
  expect(onTeach).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Teach the Loupist" }));
  expect(onTeach).toHaveBeenCalledWith(expect.stringContaining("> Use consolidated profit."));
  await user.click(screen.getByRole("button", { name: "Suggest a correction" }));
  await user.clear(screen.getByRole("textbox", { name: "Your correction" }));
  await user.click(screen.getByRole("button", { name: "Save correction" }));
  expect(screen.queryByRole("button", { name: "Teach the Loupist" })).toBeNull();
});

it("disables feedback while a reply is in progress and restores it when the reply finishes", async () => {
  const user = userEvent.setup();
  const onSave = vi.fn(async (_id, value) => ({ ...value, correction: value.correction ?? null, updatedAt: 1 }));
  const view = render(<AnswerFeedback message={message} onSave={onSave} onTeach={vi.fn()} feedbackBusy />);
  expect(screen.getByRole("button", { name: "Helpful" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Suggest a correction" })).toBeDisabled();
  screen.getByRole("button", { name: "Helpful" }).click();
  expect(onSave).not.toHaveBeenCalled();
  view.rerender(<AnswerFeedback message={message} onSave={onSave} onTeach={vi.fn()} feedbackBusy={false} />);
  await user.click(screen.getByRole("button", { name: "Helpful" }));
  expect(onSave).toHaveBeenCalledTimes(1);
});

it("keeps the previous rating when a later save conflicts", async () => {
  const user = userEvent.setup();
  const rated: ChatMessage = { ...message, feedback: { rating: "up", correction: null, updatedAt: 1 } };
  render(<AnswerFeedback message={rated} onSave={vi.fn().mockRejectedValue(new Error("this Loupist target already has an active run"))} onTeach={vi.fn()} />);
  await user.click(screen.getByRole("button", { name: "Not helpful" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("active run");
  expect(screen.getByRole("button", { name: "Helpful" })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("button", { name: "Not helpful" })).toHaveAttribute("aria-pressed", "false");
});

it("keeps the correction editable after a failed save", async () => {
  const user = userEvent.setup();
  render(<AnswerFeedback message={message} onSave={vi.fn().mockRejectedValue(new Error("Store unavailable"))} onTeach={vi.fn()} />);
  await user.click(screen.getByRole("button", { name: "Suggest a correction" }));
  await user.type(screen.getByRole("textbox"), "Correction draft");
  await user.click(screen.getByRole("button", { name: "Save correction" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Store unavailable");
  expect(screen.getByRole("textbox")).toHaveValue("Correction draft");
});

it.each(["streaming", "aborted", "error", "user"] as const)("hides feedback for %s messages", (state) => {
  const item = state === "user" ? { ...message, role: "user" as const } : { ...message, status: state };
  render(<ChatMessageCard message={item} loupistId="loupist" loupistName="Loupist" onOpenMemory={vi.fn()} onOpenMemoryPage={vi.fn()} onOpenArtifact={vi.fn()} onOpenLink={vi.fn()} onReadAttachment={vi.fn()} onSaveFeedback={vi.fn()} onTeachFeedback={vi.fn()} />);
  expect(screen.queryByRole("button", { name: "Helpful" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Suggest a correction" })).toBeNull();
});

it("saves a correction as a user principle, opens it and undoes it", async () => {
  const { MockLoupistBridge } = await import("../../bridge/mockBridge");
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0].id;
  const create = vi.spyOn(bridge, "principleCreate"), archive = vi.spyOn(bridge, "principleArchive"), open = vi.fn();
  const user = userEvent.setup();
  render(<AnswerFeedback message={{ ...message, feedback: { rating: "down", correction: "Always distinguish operating cash flow from profit.", updatedAt: 1 } }} bridge={bridge} loupistId={loupistId} chatId="original-chat" onOpenPrinciple={open} onSave={vi.fn()} onTeach={vi.fn()} />);
  await user.click(screen.getByRole("button", { name: "Save as a principle" }));
  expect(screen.getByRole("textbox", { name: "Statement" })).toHaveValue("Always distinguish operating cash flow from profit.");
  await user.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByText("Saved to your principles")).toBeVisible();
  expect(create).toHaveBeenCalledWith(expect.objectContaining({ origin: "feedback", originChatId: "original-chat", originMessageId: message.id }));
  const recordId = (await create.mock.results[0].value).recordId;
  await user.click(screen.getByRole("button", { name: "View" }));
  expect(open).toHaveBeenCalledWith({ loupistId, surface: "lens", recordId });
  await user.click(screen.getByRole("button", { name: "Undo" }));
  expect(await screen.findByRole("button", { name: "Save as a principle" })).toBeVisible();
  expect(archive).toHaveBeenCalledWith({ loupistId, recordId, archived: true });
  expect((await bridge.principleList({ loupistId })).some(item => item.recordId === recordId)).toBe(false);
});

it("offers a new principle after the saved correction changes", async () => {
  const { MockLoupistBridge } = await import("../../bridge/mockBridge");
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0].id;
  const user = userEvent.setup();
  render(<AnswerFeedback bridge={bridge} loupistId={loupistId} chatId="chat" message={{ ...message, feedback: { rating: "down", correction: "Check leverage.", updatedAt: 1 } }} onSave={async (_id, value) => ({ ...value, updatedAt: 2 })} onTeach={vi.fn()} />);
  await user.click(screen.getByRole("button", { name: "Save as a principle" }));
  await user.click(screen.getByRole("button", { name: "Save" }));
  await screen.findByText("Saved to your principles");
  await user.click(screen.getByRole("button", { name: "Suggest a correction" }));
  await user.clear(screen.getByRole("textbox", { name: "Your correction" }));
  await user.type(screen.getByRole("textbox", { name: "Your correction" }), "Check liquidity.");
  await user.click(screen.getByRole("button", { name: "Save correction" }));
  expect(await screen.findByRole("button", { name: "Save as a principle" })).toBeVisible();
});
