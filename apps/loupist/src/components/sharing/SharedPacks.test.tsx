import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { PackImportView } from "../../contracts/sharing";
import { SharedPacks } from "./SharedPacks";

async function shared(configure: (bridge: MockLoupistBridge) => void = () => {}) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  configure(bridge);
  const imports = await bridge.packList({ loupistId: "loupist-quality" });
  const onChanged = vi.fn<(imports: PackImportView[]) => void>();
  render(<SharedPacks bridge={bridge} loupistId="loupist-quality" imports={imports} indexConfigured onChanged={onChanged} />);
  return { bridge, imports, onChanged, user: userEvent.setup() };
}

it("checks once when the section opens and applies only the chosen part of an update", async () => {
  let check: ReturnType<typeof vi.spyOn> | undefined;
  const { bridge, onChanged, user } = await shared(bridge => {
    bridge.sharing.seedImport("loupist-quality", { version: 3 });
    check = vi.spyOn(bridge, "packCheckUpdate");
  });
  const apply = vi.spyOn(bridge, "packApplyUpdate");
  const diff = await screen.findByText("Update available: version 3 → 5 · Oct 26");
  const update = diff.closest("[data-pack-update]") as HTMLElement;
  expect(check).toHaveBeenCalledTimes(1);
  expect(check).toHaveBeenCalledWith({ loupistId: "loupist-quality", importId: "imp-1", automatic: true });
  // Clean items start chosen; one with a warning, or one changed here since, does not.
  expect(within(update).getByRole("checkbox", { name: "Inventory turns check" })).toBeChecked();
  expect(within(update).getByRole("checkbox", { name: /Order book against capacity/ })).not.toBeChecked();
  expect(within(update).getByText("Looks like an instruction to the AI")).toBeVisible();
  expect(within(update).getByRole("checkbox", { name: "Reverse DCF sanity check" })).toBeChecked();
  const edited = within(update).getByRole("checkbox", { name: /Read the filing before the headline\./ });
  expect(edited).not.toBeChecked();
  expect(within(update).getByText("Capex cycle reading")).toBeVisible();
  expect(within(update).getByRole("checkbox", { name: "Map: +48 names · +130 relationships · 6 withdrawn" })).toBeChecked();

  // Theirs beside yours; "Use theirs" chooses it.
  const row = edited.closest("li") as HTMLElement;
  await user.click(within(row).getByRole("button", { name: "Then / now" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText("You changed this. Theirs is shown beside yours.")).toBeVisible();
  expect(within(dialog).getByText(/I also check the footnotes\./)).toBeVisible();
  expect(within(dialog).getByText(/skip restatements\./)).toBeVisible();
  await user.click(within(dialog).getByRole("button", { name: "Use theirs" }));
  expect(edited).toBeChecked();

  await user.click(within(update).getByRole("checkbox", { name: "Inventory turns check" }));
  await user.click(within(update).getByRole("button", { name: "Apply selected" }));
  await waitFor(() => expect(onChanged).toHaveBeenCalled());
  expect(apply).toHaveBeenCalledWith({
    loupistId: "loupist-quality", importId: "imp-1", stagingId: "staging-update",
    itemKeys: expect.arrayContaining(["m-1a2b3c4d5e6f7081", "p-5e6f708192a3b4c5"]), applyMap: true,
  });
  expect(apply.mock.calls[0][0].itemKeys).toHaveLength(2);
  expect(screen.queryByText(/Update available/)).toBeNull();

  // A manual check after that is up to date.
  await user.click(screen.getByRole("button", { name: "Check for updates" }));
  expect(await screen.findByText("Up to date")).toBeVisible();
});

it("offers a newer copy for a pack added from a file, and a removal banner when the Gallery removed it", async () => {
  const { bridge, user } = await shared(bridge => {
    bridge.sharing.seedImport("loupist-quality", { sourceUrl: null, name: "Folder desk" });
    bridge.sharing.seedImport("loupist-quality", { blockedReason: "reported", name: "Reported desk" });
    bridge.sharing.updateCheck = { kind: "differentPack" };
  });
  const choose = vi.spyOn(bridge, "packChooseNewer");
  const banner = screen.getByText("Removed from the Gallery after reports. You can remove what it added.").closest("[data-pack-removed-banner]") as HTMLElement;
  await user.click(within(banner).getByRole("button", { name: "Remove what it added" }));
  expect(await screen.findByRole("dialog")).toHaveTextContent("Reported desk");
  await user.keyboard("{Escape}");
  const folder = screen.getByText("Folder desk").closest("li") as HTMLElement;
  expect(within(folder).queryByRole("button", { name: "Check for updates" })).toBeNull();
  await user.click(within(folder).getByRole("button", { name: "Choose a newer copy" }));
  await user.click(await screen.findByRole("menuitem", { name: "Choose a folder" }));
  await waitFor(() => expect(choose).toHaveBeenCalledWith({ loupistId: "loupist-quality", importId: "imp-1", source: { folder: true } }));
  expect(await within(folder).findByText("That copy is a different shared Loupist.")).toBeVisible();
});
