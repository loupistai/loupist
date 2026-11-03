import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MapToolbar, defaultFilters } from "./MapToolbar";
import { foldProps, foldedActions } from "./toolbarOverflow";

const noop = () => {};

it("folds the lowest-priority actions first, and only as many as the row needs", () => {
  const items = { list: 90, export: 80, fit: 60, replay: 80 };
  const base = { gap: 8, fixed: [200, 120, 240], more: 32 };
  // 560 + 310 of actions + six gaps fits in 920.
  expect(foldedActions({ ...base, available: 920, items })).toEqual([]);
  // Folding List view alone saves too little once More takes its place; Export goes too.
  expect(foldedActions({ ...base, available: 850, items })).toEqual(["list", "export"]);
  expect(foldedActions({ ...base, available: 300, items })).toEqual(["list", "export", "fit", "replay"]);
  // An action the toolbar does not show is never counted.
  expect(foldedActions({ ...base, available: 800, items: { list: 90, fit: 60, replay: 80 } })).toEqual(["list"]);
  // The scope segment turns into its menu button only when folding every action is not enough,
  // and the room it frees brings actions back: 560 + 32 + 32 + Replay 80 + five gaps fits in 750.
  const withScope = { ...items, scope: 240 };
  expect(foldedActions({ ...base, available: 920, items: withScope })).toEqual(["list", "export", "fit", "replay"]);
  expect(foldedActions({ ...base, available: 750, items: withScope })).toEqual(["list", "export", "fit", "scope"]);
});

it("keeps Export in the row and runs secondary actions from More on a narrow map", async () => {
  const user = userEvent.setup();
  const onList = vi.fn(), save = vi.fn();
  // The compact World toolbar keeps Export visible even when the map is narrow.
  const width = vi.spyOn(Element.prototype, "clientWidth", "get").mockImplementation(function (this: Element) { return this.classList.contains("map-toolbar") ? 500 : 0; });
  const rect = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    return { width: this.matches("button") ? 100 : 0, height: 32, top: 0, left: 0, right: 0, bottom: 0, x: 0, y: 0, toJSON: noop } as DOMRect;
  });
  try {
    render(<MapToolbar nodes={[]} query="" onQuery={noop} filters={defaultFilters()} onFilters={noop} onSelect={noop} list={false} onList={onList} onFit={noop}
      lenses={[]} onOverlay={noop} libraryOpen={false} onLibrary={noop} onReplay={noop}
      exportControl={(folded) => <button type="button" data-map-export {...foldProps("export", folded)} onClick={() => save(false)}>Export</button>} exportActions={{ disabled: false, save }} />);
    expect(document.querySelector("[data-map-list]")).toHaveAttribute("inert");
    expect(screen.getByRole("button", { name: "Export" })).not.toHaveAttribute("data-overflowed");
    expect(document.querySelector("[data-map-fit]")).toHaveAttribute("inert");
    await user.click(screen.getByRole("button", { name: "Export" }));
    expect(save).toHaveBeenCalledWith(false);
    await user.click(screen.getByRole("button", { name: "More actions" }));
    await user.click(screen.getByRole("menuitemcheckbox", { name: "List view" }));
    expect(onList).toHaveBeenCalledOnce();
  } finally {
    width.mockRestore();
    rect.mockRestore();
  }
});

it("keeps secondary World actions together in More even when there is space", async () => {
  const user = userEvent.setup();
  render(<MapToolbar nodes={[]} query="" onQuery={noop} filters={defaultFilters()} onFilters={noop} onSelect={noop} list={false} onList={noop} onFit={noop}
    lenses={[]} onOverlay={noop} libraryOpen={false} onLibrary={noop} onReplay={noop} />);
  expect(document.querySelector("[data-map-library]")).toHaveAttribute("inert");
  expect(document.querySelector("[data-map-fit]")).toHaveAttribute("inert");
  expect(document.querySelector("[data-map-list]")).toHaveAttribute("inert");
  await user.click(screen.getByRole("button", { name: "More actions" }));
  expect(screen.getByRole("menuitemcheckbox", { name: "Library" })).toBeVisible();
  expect(screen.getByRole("menuitemcheckbox", { name: "List view" })).toBeVisible();
  expect(screen.getByRole("menuitem", { name: "Fit" })).toBeVisible();
});
