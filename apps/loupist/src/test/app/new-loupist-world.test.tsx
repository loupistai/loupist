import { headerAction } from "../renderApp";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { openApp, selectedLoupistButton } from "../renderApp";

it("uses shared templates and editable profile fields without a folder or World picker", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const create = vi.spyOn(bridge, "loupistCreate"); const choose = vi.spyOn(bridge, "loupistChooseFolder");
  const user = userEvent.setup();
  await openApp(bridge); await user.click(headerAction("New Loupist"));
  const dialog = screen.getByRole("dialog", { name: "New Loupist" });
  // The background-reading notice is in the fixed footer with Create, not in the scrolling form.
  const notice = within(dialog).getByText(/reads it in the background to build its World/);
  expect(notice.parentElement).toBe(within(dialog).getByRole("button", { name: "Create Loupist" }).parentElement);
  expect(notice.closest("[data-slot=dialog-body]")).toBeNull();
  await user.click(within(dialog).getByRole("button", { name: /Dividend income Dividend payers/ }));
  const name = within(dialog).getByRole("textbox", { name: "Name" });
  await user.clear(name); await user.type(name, "Income research");
  await user.click(within(dialog).getByRole("button", { name: "Create Loupist" }));
  await waitFor(() => expect(create).toHaveBeenCalledWith({ name: "Income research", focus: "Dividend payers and payout safety", avatar: "loupist-03", template: "dividends", tone: "balanced" }));
  expect(choose).not.toHaveBeenCalled();
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

it("offers all ten face colors while creating a Loupist and saves the chosen one", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const create = vi.spyOn(bridge, "loupistCreate");
  const user = userEvent.setup();
  await openApp(bridge);
  await user.click(headerAction("New Loupist"));
  const dialog = screen.getByRole("dialog", { name: "New Loupist" });
  const avatars = within(dialog).getByRole("group", { name: "Face color" });
  expect(within(avatars).getAllByRole("button")).toHaveLength(10);
  await user.click(within(avatars).getByRole("button", { name: "Brown" }));
  expect(within(avatars).getByRole("button", { name: "Brown", pressed: true })).toBeVisible();
  await user.click(within(dialog).getByRole("button", { name: "Create Loupist" }));
  await waitFor(() => expect(create).toHaveBeenCalledWith(expect.objectContaining({ avatar: "loupist-10" })));
});

it("resets a cancelled creation draft on the next opening", async () => {
  const user = userEvent.setup(); await openApp();
  await user.click(headerAction("New Loupist"));
  await user.click(screen.getByRole("button", { name: /Custom Describe/ }));
  await user.type(screen.getByRole("textbox", { name: "Focus" }), "Unsubmitted draft");
  await user.click(screen.getByRole("button", { name: "Cancel" }));
  // The switcher has closed, so focus returns to the Loupist that opened it.
  await waitFor(() => expect(selectedLoupistButton()).toHaveFocus());
  await user.click(headerAction("New Loupist"));
  expect(screen.getByRole("textbox", { name: "Name" })).toHaveValue("Portfolio Loupist");
  expect(screen.getByLabelText("Focus")).toHaveValue("Companies I follow");
});

it("follows a picked starter in the Loupist it creates, not the open one, and nothing when cancelled", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const follow = vi.spyOn(bridge, "assetFollowName");
  const user = userEvent.setup();
  await openApp(bridge);
  const before = new Set((await bridge.loupistList()).map(item => item.id));
  await user.click(headerAction("New Loupist"));
  let dialog = screen.getByRole("dialog", { name: "New Loupist" });
  await user.click(within(dialog).getByRole("button", { name: /^Crypto/ }));
  await user.click(within(dialog).getByRole("button", { name: "Follow Bitcoin" }));
  expect(within(dialog).getByRole("button", { name: "Follow Bitcoin" })).toHaveAttribute("aria-pressed", "true");
  await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(follow).not.toHaveBeenCalled();
  await user.click(headerAction("New Loupist"));
  dialog = screen.getByRole("dialog", { name: "New Loupist" });
  await user.click(within(dialog).getByRole("button", { name: /^Crypto/ }));
  await user.click(within(dialog).getByRole("button", { name: "Follow Ethereum" }));
  // Picking follows nothing in the Loupist that is open.
  expect(follow).not.toHaveBeenCalled();
  await user.click(within(dialog).getByRole("button", { name: "Create Loupist" }));
  await waitFor(() => expect(follow).toHaveBeenCalledTimes(1));
  const created = (await bridge.loupistList()).find(item => !before.has(item.id));
  expect(follow).toHaveBeenCalledWith({ loupistId: created?.id, name: "Ethereum" });
});
