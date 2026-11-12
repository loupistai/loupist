import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { headerAction, mainTab, openApp } from "../renderApp";

const SOURCE = "github.com/kimj/loupist-semiconductor-supply-chain";
const SHARED_LINE = "Shared · Semiconductor supply chain by Kim J.";

/** Pack-sharing controls. Map image export is also named Share and is checked on its own. */
function expectNoPackSharing(scope: ParentNode = document.body) {
  expect(scope.querySelector("[data-share-loupist], [data-share-item], [data-share-sheet]")).toBeNull();
  expect(scope.querySelector("[data-loupist-shared-entry], [data-browse-gallery], [data-use-link-or-file]")).toBeNull();
  expect(scope.querySelector("[data-pack-suggestions], [data-shared-packs], [data-deep-link-dialog], .pack-review-layer")).toBeNull();
  expect(screen.queryByRole("dialog", { name: "Open a shared Loupist?" })).toBeNull();
  expect(screen.queryByText("This link can't be opened.")).toBeNull();
  expect(screen.queryByText("Or start from a shared Loupist")).toBeNull();
  expect(screen.queryByRole("button", { name: "Browse the Gallery" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Use a link or file" })).toBeNull();
  expect(screen.queryByText("Shared Loupists you added")).toBeNull();
  expect(screen.queryByText(/From Semiconductor supply chain/)).toBeNull();
  expect(screen.queryByRole("button", { name: "About this shared Loupist" })).toBeNull();
}

it("keeps World and Lens pack sharing out of the user app", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  bridge.sharing.account.indexConfigured = true;
  bridge.sharing.account.galleryTermsUrl = "https://example.com/gallery-terms";
  bridge.sharing.seedImport("loupist-quality");
  const principles = await bridge.principleList({ loupistId: "loupist-quality" });
  const principle = principles.find(item => item.statement.startsWith("Check operating cash flow"));
  if (!principle) throw new Error("missing seeded principle");
  bridge.sharing.setOrigin(principle.recordId, { importId: "imp-1", packName: "Semiconductor supply chain", author: "Kim J." });
  bridge.sharing.setOrigin("lens-compare-accounts", { importId: "imp-1", packName: "Semiconductor supply chain", author: "Kim J." });
  bridge.sharing.setOrigin("map-entity-00", { importId: "imp-1", packName: "Semiconductor supply chain", author: "Kim J." });
  bridge.sharing.receiveDeepLink({ status: "ok", source: SOURCE }, true);
  const dismiss = vi.spyOn(bridge, "deepLinkDismiss");
  const user = userEvent.setup();

  await openApp(bridge);
  await waitFor(() => expect(dismiss).toHaveBeenCalled());
  expectNoPackSharing();

  bridge.sharing.receiveDeepLink({ status: "bad" });
  bridge.sharing.receiveDeepLink({ status: "ok", source: SOURCE });
  await waitFor(() => expect(dismiss.mock.calls.length).toBeGreaterThanOrEqual(3));
  expectNoPackSharing();

  await user.click(headerAction("New Loupist"));
  const create = screen.getByRole("dialog", { name: "New Loupist" });
  expect(within(create).getByRole("button", { name: "Create Loupist" })).toBeVisible();
  expectNoPackSharing(create);
  await user.click(within(create).getByRole("button", { name: "Cancel" }));

  await user.click(await mainTab(/Lens/));
  const lens = await screen.findByRole("region", { name: /What .* knows/ });
  expectNoPackSharing(lens);
  await user.click(within(lens).getByRole("tab", { name: /Pages/ }));

  await user.click(await within(lens).findByRole("button", { name: /Compare independent accounts/ }));
  const method = await within(lens).findByRole("complementary", { name: "Compare independent accounts" });
  const article = await within(method).findByRole("article", { name: "Page" });
  expect(await within(article).findByText(SHARED_LINE)).toBeVisible();
  expectNoPackSharing(article);
  const back = within(method).queryByRole("button", { name: "Pages" });
  if (back) await user.click(back);

  await user.click(await within(lens).findByRole("button", { name: /Check operating cash flow/ }));
  const rule = await within(lens).findByRole("complementary", { name: /Check operating cash flow/ });
  expect(await within(rule).findByText(SHARED_LINE)).toBeVisible();
  expectNoPackSharing(rule);

  await user.click(await mainTab(/Map/));
  const map = await screen.findByRole("region", { name: "Map" });
  // Export lives in More and saves a file; it never opens a share sheet.
  await user.click(within(map).getByRole("button", { name: "More actions" }));
  expect(await screen.findByRole("menuitem", { name: "Export image (PNG)" })).toHaveAttribute("data-map-export");
  expect(document.querySelector("[data-share-sheet]")).toBeNull();
  await user.keyboard("{Escape}");

  await user.click(within(map).getByRole("button", { name: "More actions" }));
  await user.click(screen.getByRole("menuitemcheckbox", { name: "List view" }));
  await user.click(within(screen.getByRole("table", { name: "List view" })).getByRole("button", { name: "Apple" }));
  const apple = await screen.findByRole("complementary", { name: "Apple" });
  expect(await within(apple).findByText(SHARED_LINE)).toBeVisible();
  expect(within(apple).queryByRole("button", { name: "About this shared Loupist" })).toBeNull();

  await user.click(screen.getByRole("button", { name: "Search" }));
  const palette = await screen.findByRole("combobox", { name: "Command palette" });
  const actions = [...document.querySelectorAll<HTMLElement>('[data-palette-row^="action:"]')];
  expect(actions.map(row => row.getAttribute("data-palette-row"))).toEqual(expect.arrayContaining(["action:export-chat", "action:print-chat", "action:lens", "action:map"]));
  for (const row of actions) expect(row.textContent ?? "").not.toMatch(/gallery|shared loupist|browse the gallery|use a link/i);
  await user.type(palette, "Gallery");
  expect(screen.queryByRole("option", { name: /gallery/i })).toBeNull();
  await user.keyboard("{Escape}");

  await user.click(await mainTab(/Settings/));
  await screen.findByRole("heading", { name: "Loupists", level: 1 });
  const rows = document.querySelector(".settings-loupists");
  if (!rows) throw new Error("missing Loupist rows");
  expect(within(rows as HTMLElement).getByRole("button", { name: "Import Loupist" })).toBeVisible();
  expect(within(rows as HTMLElement).getAllByRole("button", { name: "Open Loupist settings" }).length).toBeGreaterThan(0);
  expectNoPackSharing(rows);
  const sections = screen.getByRole("navigation", { name: "Settings sections" });
  expect(within(sections).queryByRole("button", { name: /Gallery|Share/ })).toBeNull();
  await user.type(screen.getByRole("searchbox", { name: "Search settings" }), "gallery");
  expect(screen.getByText("No settings found.")).toBeVisible();
});
