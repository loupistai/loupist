import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { LoupistWorkspace } from "../../types";
import { chooseLoupist, mainTab, openApp, selectedLoupistButton } from "../renderApp";

describe("Loupist shell navigation", () => {
  it("searches Settings and returns to the previous Loupist tab", async () => {
    const user = userEvent.setup();
    await openApp();
    await user.click(await mainTab(/Map/));
    await user.click(await mainTab(/Settings/));
    expect(screen.queryByRole("button", { name: /^New research/ })).toBeNull();
    const search = screen.getByRole("searchbox", { name: "Search settings" });
    await user.type(search, "retry");
    const sections = screen.getByRole("navigation", { name: "Settings sections" });
    expect(within(sections).queryByRole("button", { name: "AI accounts" })).toBeNull();
    await user.click(within(sections).getByRole("button", { name: "Chat defaults" }));
    expect(await screen.findByRole("heading", { name: "Chat defaults", level: 1 })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Clear settings search" }));
    expect(within(sections).getByRole("button", { name: "Data connections" })).toBeVisible();
    expect(within(sections).getByRole("button", { name: "Loupists" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Back" }));
    expect(await screen.findByLabelText("Chat workspace panel")).toHaveAttribute("data-panel-kind", "world");
  });

  it("keeps Settings labels readable when the app sidebar is an icon rail", async () => {
    const user = userEvent.setup();
    await openApp();
    const appSidebar = document.querySelector("[data-slot='sidebar']")!;
    await user.click(within(appSidebar as HTMLElement).getByRole("button", { name: "Toggle Sidebar", expanded: true }));
    expect(appSidebar).toHaveAttribute("data-collapsible", "icon");
    await user.click(document.getElementById("main-tab-settings")!);
    const sections = await screen.findByRole("navigation", { name: "Settings sections" });
    const settingsSidebar = sections.closest("[data-slot='sidebar']");
    expect(settingsSidebar?.closest("[data-collapsible='icon']")).toBeNull();
    expect(within(sections).getByRole("button", { name: "Keyboard" })).toBeVisible();
    expect(screen.getByRole("searchbox", { name: "Search settings" })).toBeVisible();
  });

  it("does not mount chat titles in the icon rail", async () => {
    const user = userEvent.setup();
    await openApp();
    const appSidebar = document.querySelector("[data-slot='sidebar']") as HTMLElement;
    await user.click(within(appSidebar).getByRole("button", { name: "Toggle Sidebar", expanded: true }));
    expect(appSidebar).toHaveAttribute("data-collapsible", "icon");
    expect(within(appSidebar).getByRole("button", { name: "Bot" })).toBeVisible();
    expect(within(appSidebar).queryByRole("button", {name:"Capital allocation checklist"})).toBeNull();
    expect(within(appSidebar).queryByText("Chat, briefings, and alerts")).toBeNull();
  });

  it("finds older sessions from Search and switches Loupists from the header", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    for (let i = 0; i < 7; i++) {
      const thread = await bridge.chatCreate({ loupist_id: "loupist-value" });
      await bridge.chatRename({ loupist_id: thread.loupist_id, thread_id: thread.id, title: `Older research ${i}` });
    }
    await openApp(bridge);
    await chooseLoupist(user, "Contrarian Value");
    expect(await screen.findByRole("button", { name: "Bot", current: "page" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Search" }));
    await user.type(await screen.findByRole("combobox", { name: "Command palette" }), "Older research 0");
    await user.click(await screen.findByRole("option", { name: /Older research 0/ }));
    expect(await screen.findByRole("button", { name: "Older research 0", current: "page" })).toBeVisible();
  });

  it("shows profiles together in the header with direct profile settings and creation", async () => {
    const user = userEvent.setup();
    await openApp();
    const header = document.querySelector(".app-header")!;
    expect(header.querySelector("h1")).toBeNull();
    const profiles = screen.getByRole("navigation", { name: "Loupists" });
    expect(within(profiles).getByRole("button", { name: "Quality Compounder", pressed: true })).toBeVisible();
    expect(within(profiles).getByRole("button", { name: "Cycle Reader", pressed: false })).toBeVisible();
    expect(within(profiles).getByRole("button", { name: "New Loupist" })).toBeVisible();
    expect(within(profiles).queryByRole("button", { expanded: false })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Loupist settings" }));
    expect(await screen.findByRole("region", { name: "Loupist settings" })).toBeVisible();
  });

  it("supports view shortcuts and the typed native menu hook", async () => {
    const bridge = await openApp();
    fireEvent.keyDown(window, { key: "2", metaKey: true });
    expect(await screen.findByLabelText("Chat workspace panel")).toHaveAttribute("data-panel-kind", "world");
    fireEvent.keyDown(window, { key: "4", metaKey: true });
    expect(await screen.findByRole("region", {name:"Schedule"})).toBeVisible();
    act(() => bridge.emitMenuAction("chat"));
    expect(await screen.findByRole("textbox", { name:"Message Loupist" })).toBeEnabled();
  });

  it("opens Home with ⌘1 from a topic Chat", async () => {
    const user = userEvent.setup();
    await openApp();
    await user.click(screen.getByRole("button", { name: "Capital allocation checklist" }));
    await screen.findByRole("heading", { name: "Capital allocation checklist" });
    fireEvent.keyDown(window, { key: "1", metaKey: true });
    await waitFor(() => expect(document.querySelector('[data-testid="loupist-chat-row"][data-home] [data-testid="loupist-chat-button"]')).toHaveAttribute("aria-current", "page"));
    expect(screen.getByRole("button", { name: "Capital allocation checklist" })).not.toHaveAttribute("aria-current");
  });

  it("opens a name found on another Loupist's Map in that Loupist", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const read = bridge.knowledgeMap.bind(bridge);
    vi.spyOn(bridge, "knowledgeMap").mockImplementation(async (request) => {
      const map = await read(request);
      if (request.loupistId === "loupist-value") map.nodes.push({ id: "value-pemberton", label: "Pemberton Rail", kind: "entity", entityKind: "Organization", degree: 0, watched: false, held: false });
      return map;
    });
    await openApp(bridge);
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    await user.type(await screen.findByRole("combobox", { name: "Command palette" }), "pemberton");
    const option = await screen.findByRole("option", { name: /Pemberton Rail/ });
    expect(option).toHaveTextContent("Contrarian Value");
    fireEvent.click(option);
    await waitFor(() => expect(selectedLoupistButton()).toHaveTextContent("Contrarian Value"));
    expect(await screen.findByLabelText("Chat workspace panel")).toHaveAttribute("data-panel-kind", "world");
    expect(await screen.findByRole("heading", { level: 2, name: "Pemberton Rail" })).toBeVisible();
  });

  it("opens the holdings import from the command palette", async () => {
    const user = userEvent.setup();
    await openApp();
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    await user.type(await screen.findByRole("combobox", { name: "Command palette" }), "import holdings");
    fireEvent.click(await screen.findByRole("option", { name: /Import holdings/ }));
    expect(await screen.findByRole("dialog", { name: /holdings/i })).toBeVisible();
  });

  it("opens a principle found in another Loupist on that Loupist's Memory page", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    await bridge.principleCreate({ loupistId: "loupist-value", statement: "Pemberton rule: buy only below book value", origin: "settings" });
    await openApp(bridge);
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    await user.type(await screen.findByRole("combobox", { name: "Command palette" }), "pemberton");
    const principles = await screen.findByRole("group", { name: "Principles" });
    fireEvent.click(within(principles).getByRole("option", { name: /Pemberton rule/ }));
    await waitFor(() => expect(selectedLoupistButton()).toHaveTextContent("Contrarian Value"));
    expect((await screen.findAllByText("Pemberton rule: buy only below book value")).length).toBeGreaterThan(0);
    expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull();
  });

  it("ignores an older Loupist selection after selecting a chat in the current Loupist", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const value = await bridge.loupistSelect("loupist-value");
    let release!: (workspace: LoupistWorkspace) => void;
    vi.spyOn(bridge, "loupistSelect").mockImplementationOnce(() => new Promise(resolve => { release = resolve; }));
    await chooseLoupist(user, "Contrarian Value");
    await user.click(screen.getByRole("button", { name: "Capital allocation checklist" }));
    await screen.findByRole("heading", { name: "Capital allocation checklist" });
    await act(async () => release(value));
    await waitFor(() => expect(selectedLoupistButton()).toHaveTextContent("Quality Compounder"));
  });
});
