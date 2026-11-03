import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "@/App";
import { MockLoupistBridge } from "@/bridge/mockBridge";
import { LaunchNotice, NoticeSettingsRow } from "./Notice";

it("requires a checkbox, blocks Escape/outside dismissal, saves and stays dismissed on relaunch", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0, notice_acknowledged_version: 0 });
  const save = vi.spyOn(bridge, "noticeAcknowledge");
  const user = userEvent.setup();
  const view = render(<App bridge={bridge} />);
  const dialog = await screen.findByRole("dialog", { name: "Before you start" });
  await waitFor(() => expect(within(dialog).getByRole("checkbox")).toBeEnabled());
  expect(within(dialog).getByRole("button", { name: "Continue" })).toBeDisabled();
  expect(within(dialog).queryByRole("button", { name: "Close" })).toBeNull();
  await user.keyboard("{Escape}");
  await user.click(document.querySelector('[data-slot="dialog-overlay"]')!);
  expect(dialog).toBeVisible(); expect(save).not.toHaveBeenCalled();
  await user.click(within(dialog).getByRole("checkbox", { name: "I understand" }));
  await user.click(within(dialog).getByRole("button", { name: "Continue" }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Before you start" })).toBeNull());
  expect(save).toHaveBeenCalledTimes(1);
  view.unmount();
  render(<App bridge={bridge} />);
  await screen.findByRole("textbox", { name: "Message Loupist" });
  expect(screen.queryByRole("dialog", { name: "Before you start" })).toBeNull();
});

it("fails closed when the stored version cannot be read and supports retry", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "noticeSettingsGet").mockRejectedValueOnce(new Error("read failed"));
  const user = userEvent.setup(); render(<LaunchNotice bridge={bridge} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("The notice status couldn’t be loaded");
  expect(screen.getByRole("checkbox")).toBeDisabled();
  await user.click(screen.getByRole("button", { name: "Try again" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

it("opens the same notice from Settings without asking for acknowledgment again", async () => {
  const user = userEvent.setup(); render(<NoticeSettingsRow />);
  await user.click(screen.getByRole("button", { name: "Read again" }));
  const dialog = screen.getByRole("dialog", { name: "Before you start" });
  expect(within(dialog).queryByRole("checkbox")).toBeNull();
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).toBeNull();
});
