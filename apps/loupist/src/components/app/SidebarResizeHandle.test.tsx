import { fireEvent, render, screen } from "@testing-library/react";
import { SidebarProvider, useSidebar } from "../ui/sidebar";
import { clampSidebarWidth, readSidebarWidth, SIDEBAR_DEFAULT, SIDEBAR_MIN, SIDEBAR_RAIL, sidebarMax, SidebarResizeHandle } from "./SidebarResizeHandle";

const State = () => <output data-testid="state">{useSidebar().state}</output>;
const renderHandle = () => {
  render(<SidebarProvider><SidebarResizeHandle /><State /></SidebarProvider>);
  return screen.getByRole("separator", { name: "Resize sidebar" });
};
const width = () => document.documentElement.style.getPropertyValue("--loupist-sidebar-width");
const state = () => screen.getByTestId("state").textContent;
// The handle sits on the container's left edge at 0 in jsdom, so clientX is the width.
const drag = (handle: HTMLElement, ...xs: number[]) => {
  fireEvent.pointerDown(handle, { pointerId: 1, button: 0, clientX: xs[0] });
  for (const x of xs) fireEvent.pointerMove(handle, { pointerId: 1, clientX: x });
  fireEvent.pointerUp(handle, { pointerId: 1, clientX: xs.at(-1) });
};

// jsdom has no PointerEvent; a MouseEvent with an id is what the handle reads.
if (typeof window.PointerEvent === "undefined") {
  window.PointerEvent = class extends MouseEvent {
    pointerId: number;
    constructor(type: string, init: PointerEventInit = {}) { super(type, init); this.pointerId = init.pointerId ?? 0; }
  } as unknown as typeof PointerEvent;
}

beforeEach(() => { localStorage.clear(); document.documentElement.style.removeProperty("--loupist-sidebar-width"); });

it("keeps the width between the minimum and about a third of the window", () => {
  expect(sidebarMax(1280)).toBe(420);
  expect(sidebarMax(960)).toBe(320);
  expect(sidebarMax(500)).toBe(SIDEBAR_MIN);
  expect(clampSidebarWidth(150, 1280)).toBe(SIDEBAR_MIN);
  expect(clampSidebarWidth(900, 960)).toBe(320);
});

it("is a vertical separator with its value, and resizes from the keyboard", () => {
  const handle = renderHandle();
  expect(handle).toHaveAttribute("aria-orientation", "vertical");
  expect(handle).toHaveAttribute("tabindex", "0");
  expect(handle).toHaveAttribute("aria-valuenow", String(SIDEBAR_DEFAULT));
  expect(handle).toHaveAttribute("aria-valuemin", String(SIDEBAR_RAIL));
  expect(handle).toHaveAttribute("aria-valuemax", String(sidebarMax(window.innerWidth)));
  fireEvent.keyDown(handle, { key: "ArrowRight" });
  expect(handle).toHaveAttribute("aria-valuenow", String(SIDEBAR_DEFAULT + 20));
  expect(width()).toBe(`${SIDEBAR_DEFAULT + 20}px`);
  // One press back returns to the default: it sits on the arrow keys' grid.
  fireEvent.keyDown(handle, { key: "ArrowLeft" });
  expect(handle).toHaveAttribute("aria-valuenow", String(SIDEBAR_DEFAULT));
  fireEvent.keyDown(handle, { key: "End" });
  expect(handle).toHaveAttribute("aria-valuenow", String(sidebarMax(window.innerWidth)));
  // Home folds into the rail; the right arrow brings back the narrowest labelled sidebar.
  fireEvent.keyDown(handle, { key: "Home" });
  expect(state()).toBe("collapsed");
  expect(handle).toHaveAttribute("aria-valuenow", String(SIDEBAR_RAIL));
  fireEvent.keyDown(handle, { key: "ArrowRight" });
  expect(state()).toBe("expanded");
  expect(handle).toHaveAttribute("aria-valuenow", String(SIDEBAR_MIN));
  fireEvent.keyDown(handle, { key: "ArrowLeft" });
  expect(state()).toBe("collapsed");
});

it("snaps into the icon rail below the minimum and back out when dragged past it", () => {
  const handle = renderHandle();
  drag(handle, 240, 300);
  expect(width()).toBe("300px");
  expect(handle).toHaveAttribute("aria-valuenow", "300");
  drag(handle, 300, 180);
  expect(state()).toBe("expanded");
  expect(width()).toBe(`${SIDEBAR_MIN}px`);
  drag(handle, 180, 90);
  expect(state()).toBe("collapsed");
  drag(handle, 76, 260);
  expect(state()).toBe("expanded");
  expect(width()).toBe("260px");
});

it("remembers the width, and double-click returns to the default", () => {
  const handle = renderHandle();
  drag(handle, 248, 320);
  expect(readSidebarWidth()).toBe(320);
  fireEvent.doubleClick(handle);
  expect(width()).toBe(`${SIDEBAR_DEFAULT}px`);
  expect(readSidebarWidth()).toBe(SIDEBAR_DEFAULT);
  localStorage.setItem("loupist.sidebarWidth", "280");
  expect(readSidebarWidth()).toBe(280);
  localStorage.setItem("loupist.sidebarWidth", "nonsense");
  expect(readSidebarWidth()).toBe(SIDEBAR_DEFAULT);
});
