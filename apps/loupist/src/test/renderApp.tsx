import {
  render,
  screen,
  fireEvent,
  waitFor,
  within,

} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../App";
import { APP_DESTINATIONS, APP_PAGES } from "../app/navigation";
import { MockLoupistBridge } from "../bridge/mockBridge";

export const openApp = async (
  bridge = new MockLoupistBridge({ delay_ms: 0 }),
) => {
  render(<App bridge={bridge} />);
  // A Loupist opens on its home Thread; these tests work in the seeded research Chat.
  fireEvent.click(await screen.findByRole("button", { name: "How should we read the margin decline?" }));
  await screen.findByRole("heading", {
    name: "How should we read the margin decline?",
  });
  await screen.findByText(
    /Want to determine whether this quarter's margin decline is temporary/,
  );
  return bridge;
};

/** A Loupist opens on its home Thread; open one of its saved Chats from the sidebar. */
export const openChat = async (title = "How should we read the margin decline?") => {
  fireEvent.click(await screen.findByRole("button", { name: title }));
  await screen.findByRole("heading", { name: title });
};

export const mainTab = async (name: RegExp) => {
  const labels = [...APP_DESTINATIONS.map(destination => destination.label), "Loupist settings", "Connections", APP_PAGES.settings.label];
  // Tests name the standing conversation "Chat" (its id) or "Bot" (its label).
  const label = name.test("Chat") ? APP_DESTINATIONS[0]!.label : labels.find(candidate => name.test(candidate));
  const settingsBack = document.querySelector<HTMLButtonElement>("[data-loupist-settings-back]");
  if (settingsBack) fireEvent.click(settingsBack);
  if (label === "Settings") {
    const button = document.getElementById("main-tab-settings");
    if (button) return button;
    return screen.getByRole("navigation", { name: "Settings sections" });
  }
  if (label === "Loupist settings") {
    if (!document.querySelector(".loupist-header-identity")) fireEvent.click(screen.getByRole("button", { name: "Back" }));
    fireEvent.click(headerAction("Loupist settings"));
    const page = await screen.findByRole("region", { name: "Loupist settings" });
    fireEvent.click(page.querySelector('[data-loupist-section="capabilities"] > summary')!);
    fireEvent.click(page.querySelector('[data-loupist-section="workspace"] > summary')!);
    return page;
  }
  if (label === "Connections") {
    const settings = document.getElementById("main-tab-settings");
    if (settings) fireEvent.click(settings);
    return await screen.findByRole("button", { name: "Data connections" });
  }
  if (!document.querySelector("[data-testid=loupist-navigation]")) fireEvent.click(screen.getByRole("button", { name: "Back" }));
  if (label === "World" || label === "Lens") {
    // Map and Lens are sidebar destinations; each opens beside the conversation.
    return within(document.querySelector<HTMLElement>(`[data-sidebar-place="${label === "World" ? "map" : "lens"}"]`)!).getByRole("button");
  }
  if (label === APP_DESTINATIONS[0]!.label) {
    const close = document.querySelector<HTMLButtonElement>("[data-workspace-close]");
    if (close) fireEvent.click(close);
    const last = JSON.parse(localStorage.getItem("loupist.lastConversation") ?? "null");
    return (last && document.querySelector<HTMLButtonElement>(`[data-thread-id="${CSS.escape(last[1])}"]`)) || screen.getByRole("button", { name:"Bot" });
  }
  fireEvent.keyDown(document.querySelector<HTMLButtonElement>("[data-bot-menu]")!, { key: "Enter" });
  return screen.findByRole("menuitem", { name: "Scheduled tasks" });
};

export const newChatButton = () => screen.getByRole("button", { name: "New research" });

export const savedSurface = (name: string) => screen.getByRole("tab", { name });

/** Private chat is a checkbox in the + menu; while it is on, the composer shows its chip with an off button. */
export const privateChatOn = () => Boolean(screen.queryByRole("button", { name: "Turn off private chat" }));
export const togglePrivateChat = async () => {
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "Add to this message" }));
  await user.click(await screen.findByRole("menuitemcheckbox", { name: /Private chat/ }));
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
};

/** Opens the Loupist switcher at the top of the sidebar. */
export const openLoupistSwitcher = () => {
  if (!document.querySelector(".loupist-switcher-menu")) fireEvent.keyDown(selectedLoupistButton(), { key: "Enter" });
  return document.querySelector<HTMLElement>(".loupist-switcher-menu")!;
};

export const chooseLoupist = async (
  user: { click: (element: Element) => Promise<unknown> },
  name: string,
) => {
  await screen.findByRole("navigation", { name: "Loupists" });
  await user.click(within(openLoupistSwitcher()).getByRole("menuitemradio", { name }));
};

export const loupistIsRunning = (name: string) => {
  return Boolean(screen.queryByRole("status", { name: `${name} has active sessions` }));
};

/** A chat's row in the sidebar (Bot or a research session), by its title. */
export const sidebarRow = (title: string) =>
  within(document.querySelector<HTMLElement>("[data-testid=loupist-navigation]")!).getByRole("button", { name: title });

/** The row shows the spinner, and its description says so. */
export const rowIsRunning = (title: string) => {
  const row = sidebarRow(title);
  const described = (row.getAttribute("aria-describedby") ?? "").split(" ").map((id) => document.getElementById(id)?.textContent);
  return Boolean(row.querySelector("[data-slot=spinner].thread-running")) && described.includes("Running");
};

export const waitForLoupistRunning = async (name: string, running = true) => {
  await waitFor(() => expect(loupistIsRunning(name)).toBe(running), {timeout:15_000});
};

export const selectedLoupistButton = () => document.querySelector<HTMLButtonElement>("[data-testid=loupist-header-identity]")!;

/** The chat's materials open from the activity sheet's In use tab, as a person opens them. */
export async function openChatMaterials(user: ReturnType<typeof userEvent.setup>) {
  const activity = headerAction("Activity");
  if (activity.getAttribute("aria-pressed") !== "true") await user.click(activity);
  const sheet = await screen.findByRole("complementary", { name: "Activity" });
  await user.click(within(sheet).getByRole("tab", { name: "In use" }));
  const materials = within(sheet).getByRole("button", { name: "Open chat materials" });
  await user.click(materials);
  return materials;
}

export function headerAction(name: string) {
  const switcher = name === "New Loupist" || name === "Loupist settings";
  if (switcher) return within(openLoupistSwitcher()).getByRole("menuitem", { name });
  // The page header's one conversation action opens the activity sheet.
  if (name === "Activity") return screen.getByRole("button", { name: "Activity" });
  // Map and Lens are sidebar destinations.
  return within(document.querySelector<HTMLElement>(`[data-sidebar-place="${name === "World" ? "map" : "lens"}"]`)!).getByRole("button");
}
