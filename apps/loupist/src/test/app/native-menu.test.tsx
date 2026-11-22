import { act, fireEvent, renderHook, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useNativeMenu } from "../../app/useNativeMenu";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { chooseLoupist, openApp } from "../renderApp";

it("requests a localized menu after Settings changes language and dispatches New Loupist", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0, userSkillsAvailable: false, customConnectionsAvailable: false });
  const update = vi.spyOn(bridge, "appMenuUpdate");
  await openApp(bridge);
  await waitFor(() => expect(update).toHaveBeenLastCalledWith({ language: "en", shell: true, loupist: true, print: true, sidebar: true, modal: false }));

  act(() => bridge.emitMenuAction("settings"));
  const navigation = await screen.findByRole("navigation", { name: "Settings sections" });
  expect(within(navigation).queryByRole("button", { name: "Skills" })).toBeNull();
  await user.click(within(navigation).getByRole("button", { name: /^Keyboard/ }));
  const keyboard = within(await screen.findByRole("table"));
  expect(keyboard.getByRole("row", { name: /^Bot · Following · World · Lens · Schedule/ })).toBeVisible();
  expect(keyboard.queryByRole("row", { name: /skills|custom connections/i })).toBeNull();

  fireEvent.keyDown(window, { key: "k", metaKey: true });
  const search = within(await screen.findByRole("dialog", { name: "Search" }));
  const actions = within(await search.findByRole("group", { name: "Actions" }));
  expect(actions.getByRole("option", { name: "Keyboard shortcuts" })).toBeVisible();
  expect(actions.queryByRole("option", { name: /skills|custom connections/i })).toBeNull();
  await user.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Search" })).toBeNull());

  await user.click(await screen.findByRole("button", { name: /^Appearance/ }));
  await user.click(await screen.findByRole("button", { name: "한국어" }));
  await waitFor(() => expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ language: "ko", shell: true })));

  act(() => bridge.emitMenuAction("new-loupist"));
  const dialog = await screen.findByRole("dialog", { name: "새 루피스트" });
  expect(dialog).toBeVisible();
  await waitFor(() => expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ language: "ko", modal: true })));
  act(() => bridge.emitMenuAction("map"));
  expect(screen.queryByTestId("map-view")).toBeNull();
  await user.click(within(dialog).getByRole("button", { name: "취소" }));
  await waitFor(() => expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ language: "ko", shell: true, modal: false })));
  act(() => bridge.emitMenuAction("map"));
  expect(await screen.findByTestId("map-view")).toBeVisible();
});

it("re-enables menu commands after confirming a new Loupist", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const update = vi.spyOn(bridge, "appMenuUpdate");
  await openApp(bridge);
  act(() => bridge.emitMenuAction("new-loupist"));
  const dialog = await screen.findByRole("dialog", { name: "New Loupist" });
  await waitFor(() => expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ modal: true })));
  await user.click(within(dialog).getByRole("button", { name: "Create Loupist" }));
  await waitFor(() => expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ shell: true, loupist: true, modal: false })));
});

it("reports action availability when switching between available and unavailable Loupists", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const select = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async id => {
    const workspace = await select(id);
    if (workspace.loupist.name === "Cycle Reader") workspace.loupist.availability = { status: "workspace_unavailable" };
    return workspace;
  });
  const update = vi.spyOn(bridge, "appMenuUpdate");
  await openApp(bridge);
  await chooseLoupist(user, "Cycle Reader");
  await waitFor(() => expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ shell: true, loupist: false, modal: false })));
  await chooseLoupist(user, "Quality Compounder");
  await waitFor(() => expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ shell: true, loupist: true, modal: false })));
});

const readyMenu = { language: "en", shell: true, loupist: true, sidebar: true, print: true } as const;

it.each([[0, 1], [1, 0]])("derives overlapping modals when they close in order %i, %i", async (first, last) => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const update = vi.spyOn(bridge, "appMenuUpdate");
  const onAction = vi.fn();
  const { unmount } = renderHook(() => useNativeMenu(bridge, readyMenu, onAction));
  const dialogs = [document.createElement("div"), document.createElement("div")];
  try {
    for (const dialog of dialogs) {
      dialog.setAttribute("role", "dialog");
      dialog.setAttribute("aria-modal", "true");
      document.body.append(dialog);
    }
    await waitFor(() => expect(update).toHaveBeenLastCalledWith({ ...readyMenu, modal: true }));
    dialogs[first]!.removeAttribute("role");
    // The other dialog still blocks actions.
    fireEvent(window, new Event("focus"));
    expect(update).toHaveBeenLastCalledWith({ ...readyMenu, modal: true });
    act(() => bridge.emitMenuAction("new-loupist"));
    expect(onAction).not.toHaveBeenCalled();

    // Some closing paths reuse their container. Removing the role must be observed.
    dialogs[last]!.removeAttribute("role");
    await waitFor(() => expect(update).toHaveBeenLastCalledWith({ ...readyMenu, modal: false }));
    act(() => bridge.emitMenuAction("new-loupist"));
    expect(onAction).toHaveBeenCalledWith("new-loupist");
  } finally {
    unmount();
    dialogs.forEach(dialog => dialog.remove());
  }
});

it("keeps a modeless evidence popover out of the modal gate and resends on focus", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const update = vi.spyOn(bridge, "appMenuUpdate");
  const onAction = vi.fn();
  const { unmount } = renderHook(() => useNativeMenu(bridge, readyMenu, onAction));
  const popover = document.createElement("div");
  try {
    popover.setAttribute("role", "dialog");
    popover.setAttribute("aria-modal", "false");
    popover.dataset.state = "open";
    document.body.append(popover);
    update.mockClear();
    fireEvent(window, new Event("focus"));
    expect(update).toHaveBeenLastCalledWith({ ...readyMenu, modal: false });
    act(() => bridge.emitMenuAction("new-loupist"));
    expect(onAction).toHaveBeenCalledWith("new-loupist");
  } finally {
    unmount();
    popover.remove();
  }
});

it.each(["change", "cancel"])("resends an unblocked window after the file picker fires %s", eventName => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const update = vi.spyOn(bridge, "appMenuUpdate");
  const { unmount } = renderHook(() => useNativeMenu(bridge, readyMenu, vi.fn()));
  const input = document.createElement("input");
  input.type = "file";
  document.body.append(input);
  try {
    update.mockClear();
    fireEvent(input, new Event(eventName));
    expect(update).toHaveBeenCalledOnce();
    expect(update).toHaveBeenLastCalledWith({ ...readyMenu, modal: false });
  } finally {
    unmount();
    input.remove();
  }
});
