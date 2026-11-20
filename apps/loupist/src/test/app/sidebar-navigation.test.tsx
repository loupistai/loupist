import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { LoupistWorkspace } from "../../types";
import { chooseLoupist, mainTab, openApp, openLoupistSwitcher, selectedLoupistButton } from "../renderApp";

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
    expect(await screen.findByTestId("map-view")).toBeVisible();
    expect(document.getElementById("main-panel-map")).toContainElement(screen.getByTestId("map-view"));
  });

  it("scrolls the Settings section list so the open section's row shows above Back on a short window", async () => {
    const user = userEvent.setup();
    await openApp();
    await user.click(await mainTab(/Settings/));
    const sections = screen.getByRole("navigation", { name: "Settings sections" });
    const list = sections.closest<HTMLElement>("[data-slot='sidebar-content']")!;
    const updates = within(sections).getByRole("button", { name: "Updates" });
    // A 560 px window: the list ends at 480 px, above Back, and the Updates row is drawn below that.
    vi.spyOn(list, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 120, 248, 360));
    vi.spyOn(updates, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 520, 248, 32));
    await user.click(updates);
    expect(updates).toHaveAttribute("aria-current", "page");
    expect(list.scrollTop).toBe(72);
  });

  it("shows no market-session status in the app chrome", async () => {
    // Following US and Korean listings, the shell names no market as open or closed.
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    bridge.assets.installAsset({ name: "NVIDIA", ticker: "NVDA", exchange: "US" });
    bridge.assets.installAsset({ name: "Samsung Electronics", ticker: "005930", exchange: "KS" });
    await openApp(bridge);
    // What is followed is read after the shell draws; give that read time to land.
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 100)); });
    expect(screen.queryByRole("group", { name: "Market status" })).toBeNull();
    expect(document.querySelector("[data-slot='sidebar']")).not.toHaveTextContent(/\b(?:US|Korea) (?:open|closed|opens in)/u);
  });

  it("keeps Settings labels readable when the app sidebar is an icon rail", async () => {
    const user = userEvent.setup();
    await openApp();
    const appSidebar = document.querySelector("[data-slot='sidebar']")!;
    fireEvent.keyDown(window, { key: "b", metaKey: true });
    expect(appSidebar).toHaveAttribute("data-collapsible", "icon");
    await user.click(document.getElementById("main-tab-settings")!);
    const sections = await screen.findByRole("navigation", { name: "Settings sections" });
    const settingsSidebar = sections.closest("[data-slot='sidebar']");
    expect(settingsSidebar?.closest("[data-collapsible='icon']")).toBeNull();
    expect(within(sections).getByRole("button", { name: "Keyboard" })).toBeVisible();
    expect(screen.getByRole("searchbox", { name: "Search settings" })).toBeVisible();
  });

  it("does not mount chat titles in the icon rail", async () => {
    await openApp();
    const appSidebar = document.querySelector("[data-slot='sidebar']") as HTMLElement;
    fireEvent.keyDown(window, { key: "b", metaKey: true });
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
    await user.type(await screen.findByRole("combobox", { name: "Search" }), "Older research 0");
    await user.click(await screen.findByRole("option", { name: /Older research 0/ }));
    expect(await screen.findByRole("button", { name: "Older research 0", current: "page" })).toBeVisible();
  });

  it("keeps price setup sessions searchable and out of the sidebar", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const setup = await bridge.chatCreate({ loupist_id: "loupist-quality", title: "Price setup" });
    const select = bridge.loupistSelect.bind(bridge);
    vi.spyOn(bridge, "loupistSelect").mockImplementation(async id => {
      const workspace = await select(id);
      return { ...workspace, threads: workspace.threads.map(thread => thread.id === setup.id ? { ...thread, hide_from_recent: true } : thread) };
    });
    await openApp(bridge);
    expect(within(document.querySelector<HTMLElement>("[data-slot='sidebar']")!).queryByRole("button", { name: "Price setup" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Search" }));
    await user.type(await screen.findByRole("combobox", { name: "Search" }), "Price setup");
    expect(await screen.findByRole("option", { name: /Price setup/ })).toBeVisible();
  });

  it("lists a scheduled run's session only while it is open, and Search finds it", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    // The host keeps a session the scheduler made out of Research sessions until the user writes in it.
    const run = await bridge.chatCreate({ loupist_id: "loupist-quality", title: "Daily chip check" });
    const select = bridge.loupistSelect.bind(bridge);
    vi.spyOn(bridge, "loupistSelect").mockImplementation(async id => {
      const workspace = await select(id);
      return { ...workspace, threads: workspace.threads.map(thread => thread.id === run.id ? { ...thread, hide_from_recent: true } : thread) };
    });
    await openApp(bridge);
    const sidebar = document.querySelector<HTMLElement>("[data-slot='sidebar']")!;
    expect(within(sidebar).queryByRole("button", { name: "Daily chip check" })).toBeNull();

    // Before typing, the palette's recent sessions match the sidebar's.
    await user.click(screen.getByRole("button", { name: "Search" }));
    const palette = await screen.findByRole("combobox", { name: "Search" });
    expect(screen.queryByRole("option", { name: /Daily chip check/ })).toBeNull();
    await user.type(palette, "chip check");
    await user.click(await screen.findByRole("option", { name: /Daily chip check/ }));

    // Open, it is where the user is, so the sidebar shows it as the current conversation.
    expect(await within(sidebar).findByRole("button", { name: "Daily chip check", current: "page" })).toBeVisible();
    await user.click(within(sidebar).getByRole("button", { name: "Bot" }));
    await waitFor(() => expect(within(sidebar).queryByRole("button", { name: "Daily chip check" })).toBeNull());
  });

  it("starts the sidebar with the Loupist, whose switcher lists every Loupist, its settings and creation", async () => {
    const user = userEvent.setup();
    await openApp();
    const header = document.querySelector(".app-header")!;
    expect(header.querySelector("h1")).toBeNull();
    const profiles = screen.getByRole("navigation", { name: "Loupists" });
    expect(profiles.closest("[data-slot='sidebar']")).not.toBeNull();
    expect(within(profiles).getByRole("button", { name: /Quality Compounder/ })).toHaveAttribute("aria-haspopup", "menu");
    const menu = openLoupistSwitcher();
    expect(within(menu).getByRole("menuitemradio", { name: "Quality Compounder", checked: true })).toBeVisible();
    expect(within(menu).getByRole("menuitemradio", { name: "Cycle Reader", checked: false })).toBeVisible();
    expect(within(menu).getByRole("menuitem", { name: "New Loupist" })).toBeVisible();
    await user.click(within(menu).getByRole("menuitem", { name: "Loupist settings" }));
    expect(await screen.findByRole("region", { name: "Loupist settings" })).toBeVisible();
  });

  it("opens Map and Lens as full pages: the open one is the current row, and Bot leaves it", async () => {
    const user = userEvent.setup();
    await openApp();
    const map = await mainTab(/Map/);
    await user.click(map);
    expect(await screen.findByTestId("map-view")).toBeVisible();
    expect(document.getElementById("main-panel-map")).toContainElement(screen.getByTestId("map-view"));
    expect(screen.queryByLabelText("Side panel")).toBeNull();
    expect(document.querySelector(".app-header-title")).toHaveTextContent("Map");
    expect(map).toHaveAttribute("aria-current", "page");
    // Choosing the open place again keeps it open.
    await user.click(map);
    expect(screen.getByTestId("map-view")).toBeVisible();
    const lens = await mainTab(/Lens/);
    await user.click(lens);
    expect(await screen.findByTestId("memory-page")).toBeVisible();
    expect(document.getElementById("main-panel-lens")).toContainElement(screen.getByTestId("memory-page"));
    expect(lens).toHaveAttribute("aria-current", "page");
    expect(map).not.toHaveAttribute("aria-current");
    await user.click(screen.getByRole("button", { name: "Bot" }));
    await waitFor(() => expect(screen.queryByTestId("memory-page")).toBeNull());
    expect(lens).not.toHaveAttribute("aria-current");
    expect(screen.getByRole("textbox", { name: "Message Loupist" })).toBeVisible();
  });

  it("leads a Map page opened from a research session with the way back to it", async () => {
    const user = userEvent.setup();
    await openApp();
    await user.click(screen.getByRole("button", { name: "Capital allocation checklist" }));
    await screen.findByRole("heading", { name: "Capital allocation checklist" });
    await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "Still writing");
    await user.click(await mainTab(/Map/));
    expect(await screen.findByTestId("map-view")).toBeVisible();
    // Moving on to Lens keeps the way back to the conversation.
    await user.click(await mainTab(/Lens/));
    await user.click(await screen.findByRole("button", { name: "Back to chat" }));
    expect(await screen.findByRole("heading", { name: "Capital allocation checklist" })).toBeVisible();
    expect(screen.getByRole("textbox", { name: "Message Loupist" })).toHaveValue("Still writing");
    // From Schedule there is no conversation to return to.
    fireEvent.keyDown(window, { key: "4", metaKey: true });
    await screen.findByRole("region", { name: "Schedule" });
    fireEvent.keyDown(window, { key: "2", metaKey: true });
    expect(await screen.findByTestId("map-view")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Back to chat" })).toBeNull();
  });

  it("supports view shortcuts and the typed native menu hook", async () => {
    const bridge = await openApp();
    fireEvent.keyDown(window, { key: "2", metaKey: true });
    expect(await screen.findByTestId("map-view")).toBeVisible();
    fireEvent.keyDown(window, { key: "3", metaKey: true });
    expect(await screen.findByTestId("memory-page")).toBeVisible();
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
      if (request.loupistId === "loupist-value") map.nodes.push({ id: "value-pemberton", label: "Pemberton Rail", kind: "entity", entityKind: "Organization", degree: 0, watched: false });
      return map;
    });
    await openApp(bridge);
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    await user.type(await screen.findByRole("combobox", { name: "Search" }), "pemberton");
    const option = await screen.findByRole("option", { name: /Pemberton Rail/ });
    expect(option).toHaveTextContent("Contrarian Value");
    fireEvent.click(option);
    await waitFor(() => expect(selectedLoupistButton()).toHaveTextContent("Contrarian Value"));
    expect(await screen.findByTestId("map-view")).toBeVisible();
    expect(await screen.findByRole("heading", { level: 2, name: "Pemberton Rail" })).toBeVisible();
  });

  it("offers no holdings import in the command palette", async () => {
    const user = userEvent.setup();
    await openApp();
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    const palette = await screen.findByRole("combobox", { name: "Search" });
    // The actions are listed before anything is typed; the settings action shows the list is there.
    expect(await screen.findByRole("option", { name: /^Settings/ })).toBeVisible();
    expect(screen.queryByRole("option", { name: /holdings/i })).toBeNull();
    await user.type(palette, "import holdings");
    expect(screen.queryByRole("option", { name: /Import holdings/ })).toBeNull();
    expect(screen.queryByRole("dialog", { name: /holdings/i })).toBeNull();
  });

  it("opens a principle found in another Loupist on that Loupist's Memory page", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    await bridge.principleCreate({ loupistId: "loupist-value", statement: "Pemberton rule: buy only below book value", origin: "settings" });
    await openApp(bridge);
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    await user.type(await screen.findByRole("combobox", { name: "Search" }), "pemberton");
    const principles = await screen.findByRole("group", { name: "Principles" });
    fireEvent.click(within(principles).getByRole("option", { name: /Pemberton rule/ }));
    await waitFor(() => expect(selectedLoupistButton()).toHaveTextContent("Contrarian Value"));
    expect((await screen.findAllByText("Pemberton rule: buy only below book value")).length).toBeGreaterThan(0);
    expect(screen.queryByRole("dialog", { name: "Search" })).toBeNull();
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
