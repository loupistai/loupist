import { render, screen, within } from "@testing-library/react";
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

it("folds Scope into the Around menu, which then reads the focal name, when Library and More leave too little room", async () => {
  const width = vi.spyOn(Element.prototype, "clientWidth", "get").mockImplementation(function (this: Element) { return this.classList.contains("map-toolbar") ? 250 : 0; });
  const rect = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    return { width: this.classList.contains("map-scope-segment") ? 150 : this.matches("button") ? 60 : 0, height: 32, top: 0, left: 0, right: 0, bottom: 0, x: 0, y: 0, toJSON: noop } as DOMRect;
  });
  try {
    const user = userEvent.setup();
    const onScope = vi.fn();
    const focal = { id: "acme", label: "Acme", kind: "entity" as const, degree: 1, watched: false };
    render(<MapToolbar nodes={[]} query="" onQuery={noop} filters={defaultFilters()} onFilters={noop} onSelect={noop} list={false} onList={noop}
      lenses={[]} onOverlay={noop} libraryOpen={false} onLibrary={noop}
      local={{ focal, followed: [], recent: [], onFocal: noop, onPickOnMap: noop, scope: "1", onScope }} />);
    expect(document.querySelector("[data-overflow-item=scope]")).toHaveAttribute("inert");
    expect(document.querySelector("[data-map-scope-menu]")).toBeNull();
    const around = screen.getByRole("button", { name: "Around Acme, Scope: Direct connections" });
    expect(around).toHaveAttribute("data-scope-folded", "true");
    expect(around.querySelector(".map-around-name")).toHaveTextContent("Acme");
    expect(screen.getByRole("button", { name: "Library" })).not.toHaveAttribute("inert");
    await user.click(around);
    const menuScope = await screen.findByRole("region", { name: "Scope" });
    await user.click(within(menuScope).getByRole("button", { name: "One step further" }));
    expect(onScope).toHaveBeenCalledWith("2");
  } finally { width.mockRestore(); rect.mockRestore(); }
});

it("lets Escape through from search unless it is closing the result list", async () => {
  const user = userEvent.setup();
  const outside = vi.fn();
  const node = { id: "apple", label: "Apple", kind: "entity" as const, degree: 1, watched: false };
  const toolbar = (query: string) => <div onKeyDown={event => { if (event.key === "Escape") outside(); }}>
    <MapToolbar nodes={[node]} query={query} onQuery={noop} filters={defaultFilters()} onFilters={noop} onSelect={noop} list={false} onList={noop} lenses={[]} onOverlay={noop} libraryOpen={false} onLibrary={noop} />
  </div>;
  const view = render(toolbar(""));
  await user.click(screen.getByRole("combobox"));
  await user.keyboard("{Escape}");
  expect(outside).toHaveBeenCalledOnce();
  view.rerender(toolbar("App"));
  await user.click(screen.getByRole("combobox"));
  expect(screen.getByRole("listbox")).toBeVisible();
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(outside).toHaveBeenCalledOnce();
});

it("keeps the List view item's name and says it is on with its checked state", async () => {
  const user = userEvent.setup();
  render(<MapToolbar nodes={[]} query="" onQuery={noop} filters={defaultFilters()} onFilters={noop} onSelect={noop} list onList={noop} lenses={[]} onOverlay={noop} libraryOpen={false} onLibrary={noop} />);
  await user.click(screen.getByRole("button", { name: "More actions" }));
  expect(screen.getByRole("menuitemcheckbox", { name: "List view" })).toHaveAttribute("aria-checked", "true");
});
