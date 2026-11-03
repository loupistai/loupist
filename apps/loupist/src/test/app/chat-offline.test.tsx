import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { openApp, openChat } from "../renderApp";

const composer = () => screen.getByRole("textbox", { name: "Message Loupist" });
const setOnline = async (online: boolean) => {
  Object.defineProperty(window.navigator, "onLine", { configurable: true, get: () => online });
  await act(async () => { window.dispatchEvent(new Event(online ? "online" : "offline")); });
};

afterEach(async () => { await setOnline(true); });

describe("Chat while offline (X11)", () => {
  it("keeps a message waiting while offline and sends it when the connection returns", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const chatSend = vi.spyOn(bridge, "chatSend");
    await openApp(bridge);
    await openChat();
    await setOnline(false);
    expect(screen.getByText("You're offline. Messages you send will wait.")).toBeVisible();
    await user.type(composer(), "Compare the margins once you're back");
    await user.click(screen.getByRole("button", { name: "Send" }));
    const queue = await screen.findByLabelText("Pending chat instructions");
    expect(within(queue).getByText("Compare the margins once you're back")).toBeVisible();
    expect(screen.getByText("Waiting to send until you're back online.")).toBeVisible();
    expect(chatSend).not.toHaveBeenCalled();

    await setOnline(true);
    expect(screen.queryByText("You're offline. Messages you send will wait.")).toBeNull();
    await waitFor(() => expect(chatSend).toHaveBeenCalledTimes(1));
    expect(chatSend.mock.calls[0]![0].prompt).toBe("Compare the margins once you're back");
  });

  it("shows one quiet notice when a reply's data reads could not reach their source", async () => {
    const user = userEvent.setup();
    await openApp(new MockLoupistBridge({ delay_ms: 0 }));
    await openChat();
    const notice = "Some data couldn't be fetched because a source or the network couldn't be reached. Check your connection, then ask again.";
    expect(screen.queryByText(notice)).toBeNull();
    await user.type(composer(), "Check the offline data read path");
    await user.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(screen.getAllByText(notice)).toHaveLength(1));
    expect(screen.getByText(notice)).toHaveAttribute("role", "status");

    // A later reply that reached its sources clears it.
    await user.type(composer(), "Compare the margins");
    await user.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(screen.queryByText(notice)).toBeNull());
  });

  it("keeps the newest answer's actions visible and marks earlier answers for hover or focus (X1)", async () => {
    await openApp(new MockLoupistBridge({ delay_ms: 0 }));
    await openChat();
    const answers = [...document.querySelectorAll<HTMLElement>(".message.assistant")];
    expect(answers.length).toBeGreaterThan(0);
    expect(answers.at(-1)).toHaveAttribute("data-latest", "true");
    for (const earlier of answers.slice(0, -1)) expect(earlier).not.toHaveAttribute("data-latest");
    // Earlier answers keep real, focusable controls; CSS only fades them until hover or focus.
    for (const answer of answers) expect(within(answer).getAllByRole("button", { name: /Copy answer/ }).length).toBeGreaterThan(0);
  });
});
