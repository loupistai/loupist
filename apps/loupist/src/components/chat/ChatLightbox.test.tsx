import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ChatMessage } from "../../types";
import { ChatLightboxProvider } from "./ChatLightbox";
import { SentImages } from "./SentImages";

const image = (id: string, filename: string) => ({ id, filename, media_type: "image/png", size_bytes: 2048 });
const question = (id: string, attachments: ReturnType<typeof image>[]): ChatMessage =>
  ({ id, role: "user", content: "", created_at: "2026-10-28T00:00:00Z", status: "complete", attachments });

// Two questions in one chat: two images, then one more and a file.
const messages = [
  question("q1", [image("i1", "margin.png"), image("i2", "inventory.png")]),
  { id: "a1", role: "assistant", content: "Margins fell.", created_at: "2026-10-28T00:01:00Z", status: "complete" } as ChatMessage,
  question("q2", [image("i3", "capacity.png"), { id: "f1", filename: "notes.txt", media_type: "text/plain", size_bytes: 12 }]),
];
const read = vi.fn(async (_messageId: string, attachmentId: string) => `data:image/png;base64,${btoa(attachmentId)}`);

function renderChat(onAddToQuestion?: (file: File) => void) {
  return render(<ChatLightboxProvider messages={messages} onRead={read} onAddToQuestion={onAddToQuestion}>
    {messages.map((message) => <SentImages key={message.id} message={message} />)}
  </ChatLightboxProvider>);
}

describe("chat lightbox (context rail §2.4)", () => {
  beforeEach(() => read.mockClear());

  it("steps through every image in the chat, across messages, and wraps around", async () => {
    renderChat();
    // The file is not an image; only the three images become thumbnails.
    expect(screen.getAllByRole("button", { name: /^Open / })).toHaveLength(3);
    // A click, as in WebKit, leaves focus where it was; closing still returns to the thumbnail.
    fireEvent.click(screen.getByRole("button", { name: "Open inventory.png" }));
    const viewer = await screen.findByRole("dialog", { name: "Image viewer" });
    expect(viewer).toHaveTextContent("inventory.png");
    expect(viewer).toHaveTextContent("2 of 3");
    // The next image belongs to the second question.
    fireEvent.keyDown(viewer, { key: "ArrowRight" });
    expect(screen.getByRole("dialog")).toHaveTextContent("capacity.png");
    expect(screen.getByRole("dialog")).toHaveTextContent("3 of 3");
    await waitFor(() => expect(screen.getByRole("img", { name: "capacity.png" })).toHaveAttribute("src", `data:image/png;base64,${btoa("i3")}`));
    fireEvent.click(screen.getByRole("button", { name: "Next image" }));
    expect(screen.getByRole("dialog")).toHaveTextContent("margin.png");
    fireEvent.click(screen.getByRole("button", { name: "Previous image" }));
    expect(screen.getByRole("dialog")).toHaveTextContent("capacity.png");
    // Each image is read once, however often thumbnails and the viewer show it.
    expect(read.mock.calls.filter(([, id]) => id === "i3")).toHaveLength(1);
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    // Focus returns to the thumbnail that opened the viewer.
    await waitFor(() => expect(screen.getByRole("button", { name: "Open inventory.png" })).toHaveFocus());
  });

  it("adds the shown image to the question as a file and closes without sending", async () => {
    const user = userEvent.setup();
    const onAdd = vi.fn();
    renderChat(onAdd);
    await user.click(screen.getByRole("button", { name: "Open margin.png" }));
    await user.click(await screen.findByRole("button", { name: "Next image" }));
    const add = screen.getByRole("button", { name: "Add to question" });
    await waitFor(() => expect(add).toBeEnabled());
    await user.click(add);
    await waitFor(() => expect(onAdd).toHaveBeenCalledTimes(1));
    const file = onAdd.mock.calls[0]![0] as File;
    expect(file).toBeInstanceOf(File);
    expect(file.name).toBe("inventory.png");
    expect(file.type).toBe("image/png");
    // The bytes are the image the viewer showed ("i2", as the mock read it).
    expect(file.size).toBe(2);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("offers no Add to question where nothing can be asked", async () => {
    renderChat();
    fireEvent.click(screen.getByRole("button", { name: "Open margin.png" }));
    await screen.findByRole("dialog", { name: "Image viewer" });
    expect(screen.queryByRole("button", { name: "Add to question" })).toBeNull();
  });

  it("shows thumbnails without a viewer outside a chat", () => {
    render(<SentImages message={messages[0]!} />);
    expect(screen.queryByRole("button", { name: /^Open / })).toBeNull();
    expect(screen.getByRole("img", { name: "margin.png" })).toBeInTheDocument();
  });
});
