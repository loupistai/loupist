import { headerAction } from "../renderApp";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { openApp } from "../renderApp";

it("uses shared templates and editable profile fields without a folder or World picker", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const create = vi.spyOn(bridge, "loupistCreate"); const choose = vi.spyOn(bridge, "loupistChooseFolder");
  const user = userEvent.setup();
  await openApp(bridge); await user.click(headerAction("New Loupist"));
  const dialog = screen.getByRole("dialog", { name: "New Loupist" });
  await user.click(within(dialog).getByRole("button", { name: /Dividend income Dividend payers/ }));
  const name = within(dialog).getByRole("textbox", { name: "Name" });
  await user.clear(name); await user.type(name, "Income research");
  await user.click(within(dialog).getByRole("button", { name: "Create Loupist" }));
  await waitFor(() => expect(create).toHaveBeenCalledWith({ name: "Income research", focus: "Dividend payers and payout safety", avatar: "loupist-03", template: "dividends", tone: "balanced" }));
  expect(choose).not.toHaveBeenCalled();
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

it("offers all ten portraits while creating a Loupist and saves the chosen one", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const create = vi.spyOn(bridge, "loupistCreate");
  const user = userEvent.setup();
  await openApp(bridge);
  await user.click(headerAction("New Loupist"));
  const dialog = screen.getByRole("dialog", { name: "New Loupist" });
  const avatars = within(dialog).getByRole("group", { name: "Avatar" });
  expect(within(avatars).getAllByRole("button")).toHaveLength(10);
  await user.click(within(avatars).getByRole("button", { name: "Ship’s lantern" }));
  expect(within(avatars).getByRole("button", { name: "Ship’s lantern", pressed: true })).toBeVisible();
  await user.click(within(dialog).getByRole("button", { name: "Create Loupist" }));
  await waitFor(() => expect(create).toHaveBeenCalledWith(expect.objectContaining({ avatar: "loupist-10" })));
});

it("resets a cancelled creation draft on the next opening", async () => {
  const user = userEvent.setup(); await openApp();
  const opener = headerAction("New Loupist");
  await user.click(opener);
  await user.click(screen.getByRole("button", { name: /Custom Describe/ }));
  await user.type(screen.getByRole("textbox", { name: "Focus" }), "Unsubmitted draft");
  await user.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(opener).toHaveFocus());
  await user.click(headerAction("New Loupist"));
  expect(screen.getByRole("textbox", { name: "Name" })).toHaveValue("Portfolio Loupist");
  expect(screen.getByRole("textbox", { name: "Focus" })).toHaveValue("Companies I hold and watch");
});
