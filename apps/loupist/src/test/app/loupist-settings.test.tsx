import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { setPreviewLanguage } from "../../i18n/useLanguage";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { chooseLoupist, selectedLoupistButton, headerAction, mainTab, openApp } from "../renderApp";

it("keeps profile, chat, learning and skills on one page with changes scoped to that Loupist", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const update = vi.spyOn(bridge, "configUpdate");
  const learning = vi.spyOn(bridge, "memoryLearningSettingsSet");
  const user = userEvent.setup();
  await openApp(bridge);
  await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "Keep this research draft");
  const opener = screen.getByRole("button", { name: "Loupist settings" });
  await user.click(headerAction("Loupist settings"));
  const page = await screen.findByRole("region", { name: "Loupist settings" });
  expect(screen.queryByRole("dialog", { name: "Loupist settings" })).toBeNull();
  expect(within(page).getByRole("textbox", { name: "Instructions for this Loupist" })).toBeVisible();
  await user.click(within(page).getByRole("button", { name: "Chat preferences" }));
  expect(within(page).queryByRole("combobox", { name: "Settings apply to" })).toBeNull();
  await user.click(await within(page).findByRole("switch", { name: "Remember new chats" }));
  await waitFor(() => expect(update).toHaveBeenCalledWith(expect.objectContaining({
    scope: { kind: "project", loupistId: "loupist-quality" },
    set: { chat: { use_memory: false, update_memory: false } },
  })));
  await user.click(within(page).getByRole("button", { name: "Learning" }));
  const learn = await within(page).findByRole("switch", { name: /^Learn methods/ });
  await waitFor(() => expect(learn).toBeEnabled());
  const enabled = learn.getAttribute("data-state") === "checked";
  await user.click(learn);
  await waitFor(() => expect(learning).toHaveBeenCalledWith({ loupistId: "loupist-quality", learnMethods: !enabled }));
  await user.click(within(page).getByRole("button", { name: "Skills & connections" }));
  await user.click(within(page).getByRole("button", { name: "Manage skills" }));
  expect(await within(page).findByRole("button", { name: "Create skill" })).toBeVisible();
  expect(within(page).queryByRole("combobox", { name: "Available to" })).toBeNull();
  expect((await bridge.configRead({ scope: { kind: "global" } })).explicit).toEqual({});
  expect((await bridge.configRead({ scope: { kind: "project", loupistId: "loupist-value" } })).explicit).toEqual({});
  await user.click(within(page).getByRole("button", { name: "Back" }));
  expect(screen.getByRole("textbox", { name: "Message Loupist" })).toHaveValue("Keep this research draft");
  await waitFor(() => expect(opener).toHaveFocus());
  await user.click(headerAction("Loupist settings"));
  await screen.findByRole("region", { name: "Loupist settings" });
  act(() => bridge.emitMenuAction("map"));
  expect(screen.queryByRole("region", { name: "Loupist settings" })).toBeNull();
  expect(screen.getByRole("button", { name: /Panel kind/ })).toHaveTextContent("World");
  expect(await screen.findByRole("region", { name: "Map" })).toBeVisible();
});

it("opens the same page for the chosen Loupist from app settings", async () => {
  const user = userEvent.setup();
  await openApp();
  await user.click(await mainTab(/Settings/));
  await screen.findByRole("heading", { name: "Loupists", level: 1 });
  const rows = document.querySelector(".settings-loupists")!;
  const target = within(rows as HTMLElement).getByText("Contrarian Value").closest("li")!;
  await user.click(within(target).getByRole("button", { name: "Open Loupist settings" }));
  const page = await screen.findByRole("region", { name: "Loupist settings" });
  expect(within(page).getByRole("textbox", { name: "Name" })).toHaveValue("Contrarian Value");
  await user.click(within(page).getByRole("button", { name: "Back" }));
  expect(await screen.findByRole("heading", { name: "Loupists", level: 1 })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Chat defaults" }));
  expect(await screen.findByRole("heading", { name: "Chat defaults", level: 1 })).toBeVisible();
  expect(screen.queryByRole("combobox", { name: "Settings apply to" })).toBeNull();
  expect(screen.queryByRole("button", { name: "World processing" })).toBeNull();
});

it("opens the exact unavailable connection's setup from Loupist tools without enabling it", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const enable = vi.spyOn(bridge, "loupistCapabilityEnable");
  const user = userEvent.setup();
  await openApp(bridge);
  await user.click(await mainTab(/Loupist settings/));
  await user.click(await screen.findByRole("button", { name: "Connection settings for Korea Investment Open Trading API" }));
  expect(await screen.findByRole("dialog", { name: "Set up Korea Investment Open Trading API" })).toBeVisible();
  expect(enable).not.toHaveBeenCalled();
});

it("keeps a failed reconnect visible with the selected folder and retries without creating another Loupist", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "loupistChooseFolder").mockResolvedValue("/Users/research/copy");
  const reconnect = vi.spyOn(bridge, "loupistReconnect").mockRejectedValueOnce(new Error("This folder belongs to a different Loupist."));
  const create = vi.spyOn(bridge, "loupistCreate");
  const user = userEvent.setup();
  await openApp(bridge);
  await user.click(await mainTab(/Loupist settings/));
  await user.click(screen.getByRole("button", { name: "Reconnect folder" }));
  const dialog = screen.getByRole("dialog", { name: "Reconnect workspace folder" });
  await user.click(within(dialog).getByRole("button", { name: "Choose folder" }));
  await user.click(within(dialog).getByRole("checkbox", { name: /I trust this folder/ }));
  await user.click(within(dialog).getByRole("button", { name: "Reconnect folder" }));
  expect(await within(dialog).findByRole("alert")).toHaveTextContent("different Loupist");
  expect(within(dialog).getByLabelText("Selected workspace folder")).toHaveTextContent("/Users/research/copy");
  await user.click(within(dialog).getByRole("button", { name: "Reconnect folder" }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Reconnect workspace folder" })).toBeNull());
  expect(reconnect).toHaveBeenCalledTimes(2);
  // Reconnecting keeps the Loupist's profile name instead of adopting the folder name.
  expect(within(screen.getByRole("region", { name: "Loupist settings" })).getByRole("textbox", { name: "Name" })).toHaveValue("Quality Compounder");
  expect(create).not.toHaveBeenCalled();
});

it("distinguishes Loupists with matching folder names and opens the intended one", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupists = (await bridge.loupistList()).slice(0, 2).map((loupist, index) => ({ ...loupist, name: "research", workspace_path: `/Users/${index ? "work" : "personal"}/research` }));
  vi.spyOn(bridge, "loupistList").mockResolvedValue(loupists);
  const originalSelect = bridge.loupistSelect.bind(bridge);
  const select = vi.spyOn(bridge, "loupistSelect").mockImplementation(async (id) => ({ ...await originalSelect(id), loupist: loupists.find((loupist) => loupist.id === id)! }));
  const user = userEvent.setup();
  await openApp(bridge);
  await chooseLoupist(user, "research · /Users/work");
  await waitFor(() => expect(select).toHaveBeenLastCalledWith(loupists[1].id));
  expect(selectedLoupistButton()).toHaveTextContent("research");
});

it("keeps what the map tracks in World processing, not Learning", async () => {
  const user = userEvent.setup();
  await openApp();
  await user.click(headerAction("Loupist settings"));
  const page = await screen.findByRole("region", { name: "Loupist settings" });
  await user.click(within(page).getByRole("button", { name: "World processing" }));
  const world = page.querySelector<HTMLElement>('[data-loupist-section="world"]')!;
  const learning = page.querySelector<HTMLElement>('[data-loupist-section="learning"]')!;
  expect(await within(world).findByText("What the map tracks")).toBeVisible();
  expect(within(learning).queryByText("What the map tracks")).toBeNull();
});

it("opens the current Loupist's Skills & connections from the Connections page and leaves the sidebar on Loupists", async () => {
  const user = userEvent.setup();
  const scrolled: Element[] = [];
  const scrollIntoView = vi.spyOn(HTMLElement.prototype, "scrollIntoView").mockImplementation(function (this: HTMLElement) { scrolled.push(this); });
  try {
    await openApp();
    await user.click(await mainTab(/Connections/));
    expect(document.querySelector('.settings-navigation [aria-current="page"]')).toHaveTextContent("Connections");
    await user.click(await screen.findByRole("button", { name: "Choose which Loupists can use them" }));
    const page = await screen.findByRole("region", { name: "Loupist settings" });
    await waitFor(() => expect(page.querySelector<HTMLDetailsElement>('[data-loupist-section="capabilities"]')!.open).toBe(true));
    // It lands on the connections list, past the Skills above it.
    await waitFor(() => expect(scrolled).toContain(page.querySelector('[data-loupist-section="connections"]')));
    expect(within(page).getByRole("textbox", { name: "Name" })).toHaveValue("Quality Compounder");
    expect(document.querySelector('.settings-navigation [aria-current="page"]')).toHaveTextContent("Loupists");
  } finally {
    scrollIntoView.mockRestore();
  }
});

it("translates connection names, summaries and group labels in Loupist settings", async () => {
  const user = userEvent.setup();
  await openApp();
  await user.click(headerAction("Loupist settings"));
  const page = await screen.findByRole("region", { name: "Loupist settings" });
  await user.click(within(page).getByRole("button", { name: "Skills & connections" }));
  await within(page).findByText("Market data platform");
  setPreviewLanguage("ko");
  await act(async () => { window.dispatchEvent(new Event("languagechange")); });
  try {
    const list = (await within(page).findByText("시장 데이터 플랫폼")).closest(".agent-capability-list") as HTMLElement;
    expect(within(list).getByText("온체인 데이터")).toBeVisible();
    expect(within(list).getByText("세계은행")).toBeVisible();
    expect(within(list).queryByText("Market data platform")).toBeNull();
    expect(within(list).queryByText(/Public blockchains|Global development/)).toBeNull();
    const names = [...list.querySelectorAll(".agent-capability strong")].map((item) => item.textContent);
    expect(names[0]).toBe("시장 데이터 플랫폼");
  } finally {
    setPreviewLanguage("en");
    await act(async () => { window.dispatchEvent(new Event("languagechange")); });
  }
});
