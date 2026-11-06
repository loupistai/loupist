import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { MOCK_GALLERY_ENTRIES } from "../../bridge/mock/sharing";
import { Gallery } from "./Gallery";

afterEach(() => localStorage.clear());

function gallery(configure: (bridge: MockLoupistBridge) => void = () => {}) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  bridge.sharing.account = { ...bridge.sharing.account, indexConfigured: true };
  configure(bridge);
  const onOpened = vi.fn();
  const onUseLinkOrFile = vi.fn();
  const view = render(<Gallery bridge={bridge} onBack={vi.fn()} onOpened={onOpened} onUseLinkOrFile={onUseLinkOrFile} />);
  return { bridge, onOpened, onUseLinkOrFile, view, user: userEvent.setup() };
}

const cardNames = () => screen.getAllByRole("article").map(card => within(card).getByRole("heading").textContent);

it("combines search and filters, sorts by use, and remembers the filters on this device", async () => {
  const { user, view } = gallery();
  await waitFor(() => expect(screen.getAllByRole("article")).toHaveLength(MOCK_GALLERY_ENTRIES.length));
  // Most used first: copies, then stars.
  expect(cardNames()[0]).toBe("2차전지 밸류체인");
  const card = screen.getByRole("article", { name: "Semiconductor supply chain" });
  expect(within(card).getByText("Kim J. · English · CC BY 4.0")).toBeVisible();
  // Non-commercial licenses read by their names too.
  expect(within(screen.getByRole("article", { name: "Rate cycle watch" })).getByText("English · CC BY-NC-SA 4.0")).toBeVisible();
  expect(within(screen.getByRole("article", { name: "국내 지수 ETF 비교" })).getByText(/· CC BY-NC 4\.0$/)).toBeVisible();
  expect(within(card).getByText("14 methods · 312 names")).toHaveClass("gg-num");
  expect(within(card).getByText("Stocks")).toBeVisible();
  expect(within(card).getByRole("button", { name: "Preview Semiconductor supply chain" })).toBeVisible();

  await user.selectOptions(screen.getByRole("combobox", { name: "What it covers" }), "etfs");
  await user.selectOptions(screen.getByRole("combobox", { name: "Language" }), "en");
  expect(cardNames()).toEqual(["Semiconductor supply chain", "Dividend growers"]);
  await user.type(screen.getByRole("searchbox", { name: "Search by name, focus or author" }), "tidewater");
  expect(cardNames()).toEqual(["Dividend growers"]);
  await user.type(screen.getByRole("searchbox"), " nothing");
  expect(screen.getByText("No shared Loupists match. Try other words or clear the filters.")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Clear filters" }));
  expect(screen.getAllByRole("article")).toHaveLength(MOCK_GALLERY_ENTRIES.length);

  await user.selectOptions(screen.getByRole("combobox", { name: "Sort" }), "recent");
  expect(cardNames()[0]).toBe("2차전지 밸류체인");
  view.unmount();
  gallery();
  await waitFor(() => expect(screen.getByRole("combobox", { name: "Sort" })).toHaveValue("recent"));
});

it("opens the review from a card by its id: Use asks to focus Add, Preview does not", async () => {
  const { bridge, onOpened, user } = gallery();
  const open = vi.spyOn(bridge, "packOpen");
  const card = await screen.findByRole("article", { name: "Rate cycle watch" });
  await user.click(within(card).getByRole("button", { name: "Use Rate cycle watch" }));
  await waitFor(() => expect(onOpened).toHaveBeenCalledWith(expect.objectContaining({ kind: "preview" }), true));
  expect(open).toHaveBeenLastCalledWith({ source: { gallery: "4b8e9b0e-2f1d-4c3e-9a10-6d1f8a521b9c" } });
  await user.click(within(card).getByRole("button", { name: "Preview Rate cycle watch" }));
  await waitFor(() => expect(onOpened).toHaveBeenLastCalledWith(expect.objectContaining({ kind: "preview" }), false));
});

it("shows the kept list when offline, and a way to use a link or file when there is none", async () => {
  gallery(bridge => { bridge.sharing.gallery = { ...bridge.sharing.gallery, status: "offline", fetchedAtMs: Date.parse("2026-10-30T18:00:00Z") }; });
  expect(await screen.findByText(/^You're offline\. Showing the list from /)).toBeVisible();
  expect(screen.getAllByRole("article")).toHaveLength(MOCK_GALLERY_ENTRIES.length);
  const { onUseLinkOrFile, user } = gallery(bridge => { bridge.sharing.gallery = { status: "unavailable", fetchedAtMs: null, entries: [] }; });
  await user.click(await screen.findByRole("button", { name: "Use a link or file" }));
  expect(onUseLinkOrFile).toHaveBeenCalled();
});
