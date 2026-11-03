import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatMessage } from "../../types";
import { openApp } from "../renderApp";

const png = (name: string) => `data:image/png;base64,${btoa(name)}`;
const image = (id: string, filename: string) => ({ id, filename, media_type: "image/png", size_bytes: 2048, url: png(filename) });

describe("Loupist · images in a Chat (B83, B85)", () => {
  it("steps through the chat's images across messages and adds one to the question without sending", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const workspace = await bridge.loupistSelect("loupist-quality");
    const thread = workspace.threads.find((item) => item.title === "How should we read the margin decline?")!;
    // Two questions, each with images, and an answer between them.
    const sent: ChatMessage[] = [
      { id: "q-first", role: "user", content: "What do these show?", created_at: "2026-08-07T10:41:00+09:00", status: "complete", attachments: [image("i1", "margin.png"), image("i2", "inventory.png")] },
      { id: "a-first", role: "assistant", content: "Margins fell while inventory rose.", created_at: "2026-08-07T10:41:10+09:00", status: "complete" },
      { id: "q-later", role: "user", content: "And this chart?", created_at: "2026-08-07T10:42:00+09:00", status: "complete", attachments: [image("i3", "capacity.png")] },
      { id: "a-later", role: "assistant", content: "Capacity grew in the third quarter.", created_at: "2026-08-07T10:42:10+09:00", status: "complete" },
    ];
    thread.messages.push(...sent);
    vi.spyOn(bridge, "loupistSelect").mockResolvedValue(workspace);
    const chatSend = vi.spyOn(bridge, "chatSend");
    await openApp(bridge);

    await user.click(await screen.findByRole("button", { name: "Open inventory.png" }));
    const viewer = await screen.findByRole("dialog", { name: "Image viewer" });
    expect(viewer).toHaveTextContent("2 of 3");
    // The next image was sent with a later question.
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("dialog")).toHaveTextContent("capacity.png");
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Add to question" }));

    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    // The image waits in the composer's tray as an attachment; nothing was sent.
    const tray = await screen.findByLabelText("1 attachment");
    expect(within(tray).getByRole("img", { name: "capacity.png" })).toBeVisible();
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Message Loupist" })).toHaveFocus());
    expect(chatSend).not.toHaveBeenCalled();
  });
});
