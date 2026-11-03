import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { SidebarProvider } from "../ui/sidebar";
import { ChatListPanel } from "./ChatListPanel";

async function setup(compact: boolean, availableWidth = 1000) {
  vi.spyOn(window, "matchMedia").mockImplementation(query => ({ matches: query === "(max-width: 899px)" && compact, media: query, onchange: null, addEventListener: vi.fn(), removeEventListener: vi.fn(), addListener: vi.fn(), removeListener: vi.fn(), dispatchEvent: vi.fn() }));
  const boundingRect = HTMLElement.prototype.getBoundingClientRect;
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    return this.classList.contains("chat-tab-panel") ? DOMRect.fromRect({ width: availableWidth, height: 600 }) : boundingRect.call(this);
  });
  let resize: (() => void) | undefined;
  vi.stubGlobal("ResizeObserver", class {
    constructor(callback: () => void) { resize = callback; }
    observe() {}
    disconnect() {}
  });
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList();
  const { threads } = await bridge.loupistSelect(loupist.id);
  const onSelect = vi.fn(), onClose = vi.fn();
  const view = render(<SidebarProvider><div className="chat-tab-panel"><ChatListPanel threads={threads} activeThreadId={null} runningThreadKeys={new Set()} onSelect={onSelect} onRename={vi.fn()} onDelete={vi.fn()} onClose={onClose} /><div className="chat-layout"><button>Chat content</button></div></div></SidebarProvider>);
  return { ...view, threads, onSelect, onClose, setAvailableWidth: (width: number) => { availableWidth = width; act(() => resize?.()); } };
}

afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it("closes the compact sheet on scrim, Escape, and chat selection", async () => {
  const { container, threads, onClose, onSelect } = await setup(true);
  fireEvent.click(container.querySelector(".chat-list-scrim")!);
  expect(onClose).toHaveBeenCalledTimes(1);
  fireEvent.keyDown(window, { key: "Escape" });
  expect(onClose).toHaveBeenCalledTimes(2);
  await userEvent.click(screen.getByRole("button", { name: threads[0].title }));
  expect(onSelect).toHaveBeenCalledWith(threads[0]);
  expect(onClose).toHaveBeenCalledTimes(3);
});

it("contains keyboard focus inside the compact panel", async () => {
  const { container } = await setup(true);
  const controls = container.querySelectorAll<HTMLElement>(".chat-list-panel button, .chat-list-panel input");
  const first = controls[0], last = controls[controls.length - 1];
  first.focus();
  fireEvent.keyDown(first, { key: "Tab", shiftKey: true });
  expect(last).toHaveFocus();
  fireEvent.keyDown(last, { key: "Tab" });
  expect(first).toHaveFocus();
});

it("uses a sheet when docked panels leave too little space, and restores the chat when space returns", async () => {
  const { container, threads, onClose, onSelect, setAvailableWidth, unmount } = await setup(false);
  const panel = container.querySelector(".chat-list-panel")!;
  const chat = container.querySelector<HTMLElement>(".chat-layout")!;
  expect(panel).not.toHaveAttribute("data-overlay");
  expect(chat.inert).toBeFalsy();

  screen.getByRole("button", { name: "Chat content" }).focus();
  setAvailableWidth(600);
  expect(panel).toHaveAttribute("data-overlay", "true");
  expect(chat.inert).toBe(true);
  expect(panel.querySelector("input")).toHaveFocus();
  const controls = panel.querySelectorAll<HTMLElement>("button, input");
  controls[controls.length - 1].focus();
  fireEvent.keyDown(document.activeElement!, { key: "Tab" });
  expect(controls[0]).toHaveFocus();
  await userEvent.click(screen.getByRole("button", { name: threads[0].title }));
  expect(onSelect).toHaveBeenCalledWith(threads[0]);
  expect(onClose).toHaveBeenCalledTimes(1);

  setAvailableWidth(900);
  expect(panel).not.toHaveAttribute("data-overlay");
  expect(chat.inert).toBeFalsy();
  await userEvent.click(screen.getByRole("button", { name: threads[0].title }));
  expect(onClose).toHaveBeenCalledTimes(1);
  setAvailableWidth(600);
  unmount();
  expect(chat.inert).toBeFalsy();
});

it("leaves Escape to an overlay above the chat list", async () => {
  const { container, onClose } = await setup(true);
  container.querySelector(".chat-tab-panel")!.setAttribute("inert", "");
  fireEvent.keyDown(window, { key: "Escape" });
  expect(onClose).not.toHaveBeenCalled();
});
