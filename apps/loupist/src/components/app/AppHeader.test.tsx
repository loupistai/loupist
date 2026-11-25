import { act, fireEvent, render, screen } from "@testing-library/react";
import i18n from "@/i18n";
import { AppHeader } from "./AppHeader";
import { SidebarProvider } from "../ui/sidebar";
import { MOCK_LOUPISTS } from "../../bridge/mock/fixtures";

afterEach(async () => { await act(async () => { await i18n.changeLanguage("en"); }); });

const header = (props: Partial<Parameters<typeof AppHeader>[0]> = {}) => render(<SidebarProvider><AppHeader tab="chat" loupist={MOCK_LOUPISTS[0]!} running={false} title={null}
  onNavigate={vi.fn()} materials={null} workspaceOpen={false} workspacePlacement="side" onWorkspaceToggle={vi.fn()} onWorkspaceSplit={vi.fn()} {...props} /></SidebarProvider>);

it("names the Loupist on Bot and says when it is working", () => {
  const opened = vi.fn();
  window.addEventListener("loupist:open-standing", opened);
  const { rerender } = header();
  const banner = screen.getByRole("banner");
  expect(banner).toHaveTextContent(MOCK_LOUPISTS[0]!.name);
  expect(banner.querySelector("h1")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Standing" }));
  expect(opened).toHaveBeenCalledOnce();
  window.removeEventListener("loupist:open-standing", opened);
  rerender(<SidebarProvider><AppHeader tab="chat" loupist={MOCK_LOUPISTS[0]!} running title={null}
    onNavigate={vi.fn()} materials={null} workspaceOpen={false} workspacePlacement="side" onWorkspaceToggle={vi.fn()} onWorkspaceSplit={vi.fn()} /></SidebarProvider>);
  expect(banner).toHaveTextContent("Working");
});

it("shows a page title instead of the Loupist, and one activity control that toggles the activity sheet", async () => {
  const onToggle = vi.fn();
  const opened = vi.fn();
  window.addEventListener("loupist:open-activity", opened);
  header({ title: "Capital allocation checklist", materials: { open: false, onToggle } });
  const banner = screen.getByRole("banner");
  expect(banner).toHaveTextContent("Capital allocation checklist");
  expect(banner).not.toHaveTextContent(MOCK_LOUPISTS[0]!.name);
  fireEvent.click(screen.getByRole("button", { name: "Activity" }));
  expect(opened).toHaveBeenCalledOnce();
  expect((opened.mock.calls[0]![0] as CustomEvent).detail).toEqual({ toggle: true });
  expect(onToggle).not.toHaveBeenCalled();
  window.removeEventListener("loupist:open-activity", opened);
  await act(async () => { await i18n.changeLanguage("en-XA"); });
  expect(screen.getByTestId("header-panel-toggle").getAttribute("aria-label")).toMatch(/^⟦.*⟧$/);
});

it("offers the side panel's two splits on a conversation and marks the one in use", () => {
  const onWorkspaceSplit = vi.fn();
  const { rerender } = header({ materials: { open: false, onToggle: vi.fn() }, onWorkspaceSplit });
  const topBottom = screen.getByRole("button", { name: "Split top and bottom" });
  const leftRight = screen.getByRole("button", { name: "Split left and right" });
  expect(topBottom).toHaveAttribute("aria-pressed", "false");
  fireEvent.click(topBottom);
  expect(onWorkspaceSplit).toHaveBeenCalledWith("bottom");
  rerender(<SidebarProvider><AppHeader tab="chat" loupist={MOCK_LOUPISTS[0]!} running={false} title={null} onNavigate={vi.fn()}
    materials={{ open: true, onToggle: vi.fn() }} workspaceOpen workspacePlacement="side" onWorkspaceToggle={vi.fn()} onWorkspaceSplit={onWorkspaceSplit} /></SidebarProvider>);
  expect(leftRight).toHaveAttribute("aria-pressed", "true");
  expect(topBottom).toHaveAttribute("aria-pressed", "false");
});

it("has no split buttons off a conversation", () => {
  header({ materials: null });
  expect(screen.queryByRole("button", { name: "Split left and right" })).toBeNull();
});

it("keeps the sidebar toggle after it expands the rail in a narrow window, and has none beside a wide expanded sidebar", () => {
  const original = window.matchMedia;
  const narrow = (rail: boolean) => Object.defineProperty(window, "matchMedia", { configurable: true, value: (query: string) => ({
    matches: rail && query === "(max-width: 939px)", media: query, addEventListener: vi.fn(), removeEventListener: vi.fn(),
  }) });
  try {
    narrow(false);
    const wide = header();
    expect(screen.queryByRole("button", { name: "Show or hide sidebar" })).toBeNull();
    wide.unmount();
    narrow(true);
    // The provider starts expanded: the rail the user just widened.
    header();
    expect(screen.getByRole("button", { name: "Show or hide sidebar" })).toBeVisible();
  } finally {
    Object.defineProperty(window, "matchMedia", { configurable: true, value: original });
  }
});

it("opens ⌘1–⌘5 destinations", () => {
  const onNavigate = vi.fn();
  header({ onNavigate });
  fireEvent.keyDown(window, { key: "4", metaKey: true });
  expect(onNavigate).toHaveBeenCalledWith("lens");
});
