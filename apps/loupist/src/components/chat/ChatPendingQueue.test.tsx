import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "@/i18n";
import { REPLY_FAILED, REPLY_STOPPED } from "../../chat/draftSession";
import { ChatPendingQueue } from "./ChatPendingQueue";

const queued = (id: string, text: string) => ({ privateChat: false, modelProfileId: "test-model", thinkingLevel: "medium", runOptions: {}, files: [], id, text, createdAt: "2026-08-17T00:00:00Z", pins: [] });
const handlers = () => ({ onRemove: vi.fn(), onEdit: vi.fn(), onMove: vi.fn(), onSendNow: vi.fn() });
const openActions = async (user: ReturnType<typeof userEvent.setup>, index = 0) => {
  await user.click(screen.getAllByRole("button", { name: "Queued message actions" })[index]!);
};

it("focuses edits, keeps IME input, and reopens the latest saved text", async () => {
  const user = userEvent.setup();
  const props = handlers();
  const { rerender } = render(<ChatPendingQueue models={[]} {...props} queued={[queued("q1", "First draft")]} />);
  await openActions(user);
  await user.click(screen.getByRole("menuitem", { name: "Edit queued message" }));
  const editor = screen.getByRole("textbox", { name: "Queued message text" });
  expect(editor).toHaveFocus();
  fireEvent.keyDown(editor, { key: "Enter", isComposing: true, keyCode: 229 });
  expect(props.onEdit).not.toHaveBeenCalled();
  await user.keyboard("{Escape}");
  await waitFor(() => expect(screen.getByRole("button", { name: "Queued message actions" })).toHaveFocus());
  rerender(<ChatPendingQueue models={[]} {...props} queued={[queued("q1", "Latest saved text")]} />);
  await openActions(user);
  await user.click(screen.getByRole("menuitem", { name: "Edit queued message" }));
  expect(screen.getByRole("textbox", { name: "Queued message text" })).toHaveValue("Latest saved text");
});

it("edits, reorders, sends, and removes queued messages from each row's menu", async () => {
  const user = userEvent.setup();
  const props = handlers();
  render(<ChatPendingQueue models={[]} {...props} canSendNow queued={[queued("q1", "First queued"), queued("q2", "Second queued")]} />);
  expect(screen.getByRole("button", { name: /Up next/ })).toHaveAttribute("aria-expanded", "true");
  await openActions(user, 0);
  await user.click(screen.getByRole("menuitem", { name: "Edit queued message" }));
  await user.clear(screen.getByLabelText("Queued message text"));
  await user.type(screen.getByLabelText("Queued message text"), "Edited first");
  await user.click(screen.getByRole("button", { name: "Save queued message" }));
  expect(props.onEdit).toHaveBeenCalledWith("q1", "Edited first");
  await openActions(user, 0);
  await user.click(screen.getByRole("menuitem", { name: "Move queued message down" }));
  expect(props.onMove).toHaveBeenCalledWith("q1", 1);
  await openActions(user, 0);
  await user.click(screen.getByRole("menuitem", { name: "Send queued message now" }));
  expect(props.onSendNow).toHaveBeenCalledWith("q1");
  await openActions(user, 0);
  await user.click(screen.getByRole("menuitem", { name: "Remove queued message" }));
  expect(props.onRemove).toHaveBeenCalledWith("q1");
});

it("calls out a missing model and leaves an automatic one unnamed", () => {
  const { rerender } = render(<ChatPendingQueue models={[]} {...handlers()} queued={[{ ...queued("q1", "Next"), modelProfileId: "" }]} />);
  expect(screen.queryByText("Unavailable model")).toBeNull();
  rerender(<ChatPendingQueue models={[]} {...handlers()} queued={[queued("q1", "Next")]} />);
  expect(screen.getByText("Unavailable model")).toBeVisible();
});

it("folds the queue to a count and opens it again", async () => {
  const user = userEvent.setup();
  render(<ChatPendingQueue models={[]} {...handlers()} queued={[queued("q1", "First queued"), queued("q2", "Second queued")]} />);
  expect(screen.getByText("First queued")).toBeVisible();
  await user.click(screen.getByRole("button", { name: /Up next/ }));
  expect(screen.getByRole("button", { name: /Up next/ })).toHaveAttribute("aria-expanded", "false");
  expect(screen.getByText("First queued")).not.toBeVisible();
  await user.click(screen.getByRole("button", { name: /Up next/ }));
  expect(screen.getByText("Second queued")).toBeVisible();
});

it("does not send a queued message while the current response is running", async () => {
  const user = userEvent.setup();
  const props = handlers();
  render(<ChatPendingQueue models={[]} {...props} canSendNow={false} queued={[queued("q1", "Wait for the answer")]} />);
  await openActions(user);
  const send = screen.getByRole("menuitem", { name: /^Send queued message now/ });
  expect(send).toHaveAttribute("aria-disabled", "true");
  await user.click(send);
  expect(props.onSendNow).not.toHaveBeenCalled();
});

it("waits quietly while offline: no guide hint and no send-now", async () => {
  const user = userEvent.setup();
  render(<ChatPendingQueue models={[]} {...handlers()} canSendNow offline queued={[queued("q1", "Check the filing")]} />);
  expect(screen.queryByText(/Guide now/)).toBeNull();
  expect(screen.queryByText("Queued")).toBeNull();
  await openActions(user);
  expect(screen.getByRole("menuitem", { name: "Send queued message now Waits until you're back online" })).toHaveAttribute("aria-disabled", "true");
});

it("says why Send now is unavailable: the running reply, or what to reconnect", async () => {
  const user = userEvent.setup();
  const { rerender } = render(<ChatPendingQueue models={[]} {...handlers()} canSendNow={false} queued={[queued("q1", "Next")]} />);
  await openActions(user);
  expect(screen.getByRole("menuitem", { name: "Send queued message now Waits until the current response finishes" })).toHaveAttribute("aria-disabled", "true");
  await user.keyboard("{Escape}");
  rerender(<ChatPendingQueue models={[]} {...handlers()} canSendNow={false} blockedReason="Reconnect the workspace folder to send messages" queued={[queued("q1", "Next")]} />);
  await openActions(user);
  expect(screen.getByRole("menuitem", { name: "Send queued message now Reconnect the workspace folder to send messages" })).toBeVisible();
});

it("offers the next message after a stopped reply held the queue", async () => {
  const user = userEvent.setup();
  const props = handlers();
  const { rerender } = render(<ChatPendingQueue models={[]} {...props} holdReason={REPLY_STOPPED} queued={[queued("q1", "First"), queued("q2", "Second")]} />);
  await user.click(screen.getByRole("button", { name: "Send next now" }));
  expect(props.onSendNow).toHaveBeenCalledWith("q1");
  // Nothing to offer while the message cannot go: offline, or while a reply runs.
  rerender(<ChatPendingQueue models={[]} {...props} offline holdReason="Waiting to send until you're back online." queued={[queued("q1", "First")]} />);
  expect(screen.queryByRole("button", { name: "Send next now" })).toBeNull();
  rerender(<ChatPendingQueue models={[]} {...props} canSendNow={false} holdReason="Reconnect the workspace folder before sending." queued={[queued("q1", "First")]} />);
  expect(screen.queryByRole("button", { name: "Send next now" })).toBeNull();
});

it("says queued messages were kept after a stopped or failed reply only while one is queued, in both languages", async () => {
  // The notice's own words, without the Send next now button beside them.
  const hold = () => screen.getByRole("status").firstElementChild?.textContent;
  const { rerender } = render(<ChatPendingQueue models={[]} {...handlers()} holdReason={REPLY_STOPPED} queued={[queued("q1", "First")]} />);
  expect(hold()).toBe("Response stopped. Queued messages were kept.");
  // The last queued message removed: nothing is left to keep.
  rerender(<ChatPendingQueue models={[]} {...handlers()} holdReason={REPLY_STOPPED} queued={[]} />);
  expect(hold()).toBe("Response stopped.");
  rerender(<ChatPendingQueue models={[]} {...handlers()} holdReason={REPLY_FAILED} queued={[]} />);
  expect(hold()).toBe("Response failed.");
  await act(async () => { await i18n.changeLanguage("ko"); });
  try {
    rerender(<ChatPendingQueue models={[]} {...handlers()} holdReason={REPLY_STOPPED} queued={[]} />);
    expect(hold()).toBe("응답을 중지했습니다.");
    rerender(<ChatPendingQueue models={[]} {...handlers()} holdReason={REPLY_STOPPED} queued={[queued("q1", "First")]} />);
    expect(hold()).toBe("응답을 중지했습니다. 대기 중인 메시지는 유지되어 있습니다.");
    rerender(<ChatPendingQueue models={[]} {...handlers()} holdReason={REPLY_FAILED} queued={[]} />);
    expect(hold()).toBe("응답하지 못했습니다.");
  } finally {
    await act(async () => { await i18n.changeLanguage("en"); });
  }
});

it("moves focus to the next row, then to the message, when a row's menu removes it", async () => {
  const user = userEvent.setup();
  const props = handlers();
  const composer = document.createElement("textarea");
  composer.setAttribute("data-chat-composer", "true");
  document.body.append(composer);
  const { rerender } = render(<ChatPendingQueue models={[]} {...props} queued={[queued("q1", "First"), queued("q2", "Second")]} />);
  await openActions(user, 0);
  await user.click(screen.getByRole("menuitem", { name: "Remove queued message" }));
  rerender(<ChatPendingQueue models={[]} {...props} queued={[queued("q2", "Second")]} />);
  await waitFor(() => expect(screen.getByRole("button", { name: "Queued message actions" })).toHaveFocus());
  await openActions(user, 0);
  await user.click(screen.getByRole("menuitem", { name: "Remove queued message" }));
  rerender(<ChatPendingQueue models={[]} {...props} queued={[]} />);
  await waitFor(() => expect(composer).toHaveFocus());
  composer.remove();
});
