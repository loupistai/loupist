import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { WorkingNote } from "../../types";
import { ChatWorkingContext } from "./ChatWorkingContext";
import { renderWithOpen, destinations } from "../../test/open";

const first: WorkingNote = { researchId: "first", chatId: "chat-a", title: "Monthly savings", markdown: "Contribution: **500**. Assumed return: 4%.", changeSummary: "Created the plan", recordedAtMs: 1_700_000_000_000 };
const second: WorkingNote = { ...first, researchId: "second", markdown: "Contribution: **750**. Assumed return: 4%.", changeSummary: "Changed the contribution" };
const setup = (bridge = new MockLoupistBridge({ delay_ms: 0 })) => {
  const onAskForChange = vi.fn();
  const props = { bridge, loupistId: "loupist-a", chatId: "chat-a", isRunning: false, updateKey: "initial", onAskForChange };
  return { bridge, props, onAskForChange };
};

describe("Chat working context", () => {
  it("keeps the conversation visible and opens only the current note", async () => {
    const user = userEvent.setup();
    const { bridge, props, onAskForChange } = setup();
    const read = vi.spyOn(bridge, "knowledgeWorkingNotes").mockResolvedValue({ current: second });
    render(<ChatWorkingContext {...props}><p>Conversation content</p></ChatWorkingContext>);
    expect(screen.getByText("Conversation content")).toBeVisible();
    await user.click(await screen.findByRole("button", { name: "Working note" }));
    expect(await screen.findByRole("article", { name: "Working note" })).toHaveTextContent("750");
    expect(screen.getByText(second.changeSummary)).toBeVisible();
    expect(read).toHaveBeenLastCalledWith({ loupistId: "loupist-a", chatId: "chat-a" });
    expect(screen.queryByRole("button", { name: "Changes" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Undo last change" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Ask for a change" }));
    expect(onAskForChange).toHaveBeenCalledOnce();
    expect(screen.getByText("Conversation content")).toBeVisible();
  });

  it("opens the exact Source and Lens versions used by the note", async () => {
    const user = userEvent.setup();
    const { bridge, props } = setup();
    const note: WorkingNote = { ...first, inputs: {
      sources: [{ receiptId: "read-source", recordId: "source-record", revisionId: "source-at-research", spanId: "original-span", coverage: "included_full", attribution: "Original filing" }],
      worlds: [],
      lenses: [{ recordId: "lens-record", revisionId: "lens-at-research", markdown: "Original method", status: "applied" }],
    } };
    vi.spyOn(bridge, "knowledgeWorkingNotes").mockResolvedValue({ current: note });
    const { open } = renderWithOpen(<ChatWorkingContext {...props}><p>Conversation content</p></ChatWorkingContext>);
    await user.click(await screen.findByRole("button", { name: "Working note" }));
    await user.click(screen.getByRole("button", { name: "Source: Original filing" }));
    expect(destinations(open).at(-1)).toEqual({ kind: "memory", title: "Source: Original filing", location: expect.objectContaining({ loupistId: "loupist-a", surface: "source", recordId: "source-record", revisionId: "source-at-research", selection: { kind: "source", sourceId: undefined, spanId: "original-span" } }) });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Working note" }));
    await user.click(screen.getByRole("button", { name: "Lens 1 · Applied" }));
    expect(destinations(open).at(-1)).toEqual({ kind: "memory", title: "Lens 1 · Applied", location: expect.objectContaining({ surface: "lens", recordId: "lens-record", revisionId: "lens-at-research" }) });
    expect(screen.getByText("Conversation content")).toBeVisible();
  });

  it("keeps active work in Conversation and ignores a late note from a previous chat", async () => {
    const { bridge, props } = setup();
    let resolve!: (page: { current: WorkingNote }) => void;
    const read = vi.spyOn(bridge, "knowledgeWorkingNotes").mockImplementationOnce(() => new Promise((done) => { resolve = done; })).mockResolvedValue({ current: null });
    const view = render(<ChatWorkingContext key="chat-a" {...props}><p>First chat</p></ChatWorkingContext>);
    await waitFor(() => expect(read).toHaveBeenCalledOnce());
    view.rerender(<ChatWorkingContext key="chat-b" {...props} chatId="chat-b" isRunning><p>Second chat is streaming</p></ChatWorkingContext>);
    await act(async () => resolve({ current: first }));
    expect(screen.queryByRole("article", { name: "Working note" })).toBeNull();
    expect(screen.getByText("Second chat is streaming")).toBeVisible();
    expect(read).toHaveBeenCalledOnce();
  });
});
