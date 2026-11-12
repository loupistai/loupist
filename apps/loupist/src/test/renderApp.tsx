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
  // Tests name the home destination "Chat" (its id) or "Home" (its label).
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
  if (!document.querySelector(".header-workspace-controls")) fireEvent.click(screen.getByRole("button", { name: "Back" }));
  if (label === "Map" || label === "Lens") {
    fireEvent.keyDown(screen.getByRole("button", { name: /Panel kind/ }), { key: "Enter" });
    return screen.findByRole("menuitemradio", { name: label === "Map" ? "Map" : "Lens" });
  }
  if (label === APP_DESTINATIONS[0]!.label) {
    const close = document.querySelector<HTMLButtonElement>("[data-workspace-close]");
    if (close) fireEvent.click(close);
    const last = JSON.parse(localStorage.getItem("loupist.lastConversation") ?? "null");
    return (last && document.querySelector<HTMLButtonElement>(`[data-thread-id="${CSS.escape(last[1])}"]`)) || screen.getByRole("button", { name:"Home" });
  }
  fireEvent.keyDown(document.querySelector<HTMLButtonElement>("[data-bot-menu]")!, { key: "Enter" });
  return screen.findByRole("menuitem", { name: "Scheduled tasks" });
};

export const newChatButton = () => screen.getByRole("button", { name: "New chat" });

export const savedSurface = (name: string) => screen.getByRole("tab", { name });

/** Private chat is a checkbox in the + menu; while it is on, the composer shows its chip with an off button. */
export const privateChatOn = () => Boolean(screen.queryByRole("button", { name: "Turn off private chat" }));
export const togglePrivateChat = async () => {
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "Add to this message" }));
  await user.click(await screen.findByRole("menuitemcheckbox", { name: /Private chat/ }));
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
};

export const chooseLoupist = async (
  user: { click: (element: Element) => Promise<unknown> },
  name: string,
) => {
  const selector = screen.getByRole("navigation", { name: "Loupists" });
  await user.click(await within(selector).findByRole("button", { name }));
};

export const loupistIsRunning = (name: string) => {
  return Boolean(screen.queryByRole("status", { name: `${name} has chats running` }));
};

export const waitForLoupistRunning = async (name: string, running = true) => {
  await waitFor(() => expect(loupistIsRunning(name)).toBe(running), {timeout:15_000});
};

export const selectedLoupistButton = () => document.querySelector<HTMLButtonElement>('.loupist-header-identity[aria-pressed="true"]')!;

export function headerAction(name: string) {
  const switcher = name === "New Loupist" || name === "Loupist settings";
  if (switcher) return screen.getByRole("button", { name });
  if (name === "Panel") return screen.getByRole("button", { name: "Split left and right" });
  const trigger = screen.getByRole("button", { name: /Panel kind/ });
  fireEvent.keyDown(trigger, { key: "Enter" });
  return screen.getByRole("menuitemradio", { name });
}
