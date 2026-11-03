import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { mainTab, openApp, selectedLoupistButton } from "../renderApp";

const folder = "/Users/research/Backups/Restored Loupist";

async function openImport(bridge: MockLoupistBridge) {
  const user = userEvent.setup();
  await openApp(bridge);
  await user.click(await mainTab(/Settings/));
  await user.click(screen.getByRole("button", { name: "Loupists" }));
  await user.click(await screen.findByRole("button", { name: "Import Loupist" }));
  return { user, dialog: screen.getByRole("dialog", { name: "Open a Loupist folder" }) };
}

it("requires a chosen and trusted folder, then imports, selects the Loupist and opens Chat", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "loupistChooseFolder").mockResolvedValue(folder);
  const importLoupist = vi.spyOn(bridge, "loupistImport");
  const createLoupist = vi.spyOn(bridge, "loupistCreate");
  const { user, dialog } = await openImport(bridge);
  const submit = within(dialog).getByRole("button", { name: "Import" });
  expect(submit).toBeDisabled();
  await user.click(within(dialog).getByRole("button", { name: "Choose folder" }));
  expect(within(dialog).getByLabelText("Selected workspace folder")).toHaveTextContent(folder);
  expect(submit).toBeDisabled();
  await user.click(within(dialog).getByRole("checkbox", { name: /I trust this folder/ }));
  await user.click(submit);
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Open a Loupist folder" })).toBeNull());
  expect(importLoupist).toHaveBeenCalledExactlyOnceWith({ workspace_path: folder });
  expect(selectedLoupistButton()).toHaveTextContent("Restored Loupist");
  expect(screen.getByRole("button", {name:"Bot",current:"page"})).toBeVisible();
  expect(createLoupist).not.toHaveBeenCalled();
});

it("keeps a native rejection visible and retries the same folder", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "loupistChooseFolder").mockResolvedValue(folder);
  const importLoupist = vi.spyOn(bridge, "loupistImport").mockRejectedValueOnce(new Error("This Loupist is already registered."));
  const selectLoupist = vi.spyOn(bridge, "loupistSelect");
  const { user, dialog } = await openImport(bridge);
  await user.click(within(dialog).getByRole("button", { name: "Choose folder" }));
  await user.click(within(dialog).getByRole("checkbox", { name: /I trust this folder/ }));
  const selectionsBefore = selectLoupist.mock.calls.length;
  await user.click(within(dialog).getByRole("button", { name: "Import" }));
  const alert = await within(dialog).findByRole("alert");
  await user.click(within(alert).getByText("Error details"));
  expect(within(alert).getByText("This Loupist is already registered.")).toBeVisible();
  expect(selectLoupist).toHaveBeenCalledTimes(selectionsBefore);
  expect(within(dialog).getByRole("checkbox")).toBeChecked();
  await user.click(within(dialog).getByRole("button", { name: "Import" }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Open a Loupist folder" })).toBeNull());
  expect(importLoupist).toHaveBeenCalledTimes(2);
});

it("resets folder trust when changing folders", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "loupistChooseFolder").mockResolvedValueOnce(folder).mockResolvedValueOnce("/Users/research/Another Loupist");
  const { user, dialog } = await openImport(bridge);
  await user.click(within(dialog).getByRole("button", { name: "Choose folder" }));
  await user.click(within(dialog).getByRole("checkbox"));
  await user.click(within(dialog).getByRole("button", { name: "Change folder" }));
  expect(within(dialog).getByRole("checkbox")).not.toBeChecked();
  expect(within(dialog).getByRole("button", { name: "Import" })).toBeDisabled();
});
