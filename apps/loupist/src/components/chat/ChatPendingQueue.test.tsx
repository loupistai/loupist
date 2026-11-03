import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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
  const send = screen.getByRole("menuitem", { name: "Send queued message now" });
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
  expect(screen.getByRole("menuitem", { name: "Send queued message now" })).toHaveAttribute("aria-disabled", "true");
});
