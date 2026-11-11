import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MapToolbar, defaultFilters } from "./MapToolbar";

const noop = () => {};

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
      exportControl={<button type="button" data-map-export onClick={() => save(false)}>Export</button>} />);
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


it("collapses Scope when pinned Export and More leave too little room", () => {
  const width = vi.spyOn(Element.prototype, "clientWidth", "get").mockImplementation(function (this: Element) { return this.classList.contains("map-toolbar") ? 250 : 0; });
  const rect = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    return { width: this.classList.contains("map-scope-segment") ? 150 : this.matches("button") ? 60 : 0, height: 32, top: 0, left: 0, right: 0, bottom: 0, x: 0, y: 0, toJSON: noop } as DOMRect;
  });
  try {
    const focal = { id: "acme", label: "Acme", kind: "entity" as const, degree: 1, watched: false, held: false };
    render(<MapToolbar nodes={[]} query="" onQuery={noop} filters={defaultFilters()} onFilters={noop} onSelect={noop} list={false} onList={noop} onFit={noop}
      lenses={[]} onOverlay={noop} libraryOpen={false} onLibrary={noop} exportControl={<button data-map-export>Export</button>}
      local={{ focal, followed: [], recent: [], onFocal: noop, onPickOnMap: noop, scope: "1", onScope: noop }} />);
    expect(document.querySelector("[data-overflow-item=scope]")).toHaveAttribute("inert");
    expect(document.querySelector("[data-map-scope-menu]")).toHaveAttribute("data-scope-folded", "true");
    expect(screen.getByRole("button", { name: "Export" })).not.toHaveAttribute("inert");
  } finally { width.mockRestore(); rect.mockRestore(); }
});
