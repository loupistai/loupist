import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MapToolbar, defaultFilters } from "./MapToolbar";

const noop = () => {};

it("keeps Library in the row and runs Export and the other secondary actions from More", async () => {
  const user = userEvent.setup();
  const onList = vi.fn(), onLibrary = vi.fn(), image = vi.fn(), video = vi.fn();
  render(<MapToolbar nodes={[]} query="" onQuery={noop} filters={defaultFilters()} onFilters={noop} onSelect={noop} list={false} onList={onList}
    lenses={[]} onOverlay={noop} libraryOpen={false} onLibrary={onLibrary} onReplay={noop} exportActions={{ image, video }} />);
  await user.click(screen.getByRole("button", { name: "Library" }));
  expect(onLibrary).toHaveBeenCalledOnce();
  expect(document.querySelector(".map-toolbar [data-map-fit], .map-toolbar [data-map-list], .map-toolbar [data-map-export]")).toBeNull();
  await user.click(screen.getByRole("button", { name: "More actions" }));
  expect(screen.getByRole("menuitemcheckbox", { name: "Replay" })).toBeVisible();
  await user.click(screen.getByRole("menuitem", { name: "Export image (PNG)" }));
  expect(image).toHaveBeenCalledOnce();
  await user.click(screen.getByRole("button", { name: "More actions" }));
  await user.click(screen.getByRole("menuitemcheckbox", { name: "List view" }));
  expect(onList).toHaveBeenCalledOnce();
  expect(video).not.toHaveBeenCalled();
});

it("collapses Scope when Library and More leave too little room", () => {
  const width = vi.spyOn(Element.prototype, "clientWidth", "get").mockImplementation(function (this: Element) { return this.classList.contains("map-toolbar") ? 250 : 0; });
  const rect = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    return { width: this.classList.contains("map-scope-segment") ? 150 : this.matches("button") ? 60 : 0, height: 32, top: 0, left: 0, right: 0, bottom: 0, x: 0, y: 0, toJSON: noop } as DOMRect;
  });
  try {
    const focal = { id: "acme", label: "Acme", kind: "entity" as const, degree: 1, watched: false, held: false };
    render(<MapToolbar nodes={[]} query="" onQuery={noop} filters={defaultFilters()} onFilters={noop} onSelect={noop} list={false} onList={noop}
      lenses={[]} onOverlay={noop} libraryOpen={false} onLibrary={noop}
      local={{ focal, followed: [], recent: [], onFocal: noop, onPickOnMap: noop, scope: "1", onScope: noop }} />);
    expect(document.querySelector("[data-overflow-item=scope]")).toHaveAttribute("inert");
    expect(document.querySelector("[data-map-scope-menu]")).toHaveAttribute("data-scope-folded", "true");
    expect(screen.getByRole("button", { name: "Library" })).not.toHaveAttribute("inert");
  } finally { width.mockRestore(); rect.mockRestore(); }
});
