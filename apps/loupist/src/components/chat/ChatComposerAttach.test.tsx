import { useState, type ComponentProps } from "react";
import { createEvent, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TooltipProvider } from "../ui/tooltip";
import { ChatComposer } from "./ChatComposer";
import type { KnowledgePin } from "../../types";

const pin = { kind: "source" as const, loupistId: "loupist", recordId: "sources", revisionId: "rev", objectId: "src-1", label: "Q3 operating review" };

function MenuComposer({ status, onAddKnowledgePin, onSend = vi.fn() }: { status: "ready" | "empty" | "closed"; onAddKnowledgePin: (value: KnowledgePin) => void; onSend?: ComponentProps<typeof ChatComposer>["onSend"] }) {
  const [prompt, setPrompt] = useState("");
  return <TooltipProvider><ChatComposer
    prompt={prompt} onPromptChange={setPrompt}
    privateChat={false} isRunning={false} error={null} streamAnnouncement=""
    models={[{ id: "model", provider: "test", model: "model", name: "Model", input: ["text", "image"], reasoning: true, context_window: 10000, max_tokens: 1000, thinking_levels: ["medium"], thinking_level_map: {}, run_controls: [], credential_source: "saved" }]} providers={[]} modelSelection={{ model_profile_id: "model", thinking_level: "medium", run_options: {} }} modelIsDefault onChooseDefault={() => {}}
    skills={[]} plugins={[]}
    onPrivateChatChange={vi.fn()} onModelSelectionChange={vi.fn()}
    onSend={onSend} onFollowUp={vi.fn()} onAbort={vi.fn()}
    mapScreenshot={{ status: () => status, take: async () => new File([new Uint8Array([1])], "world.png", { type: "image/png" }) }}
    evidenceSources={[{ key: "sources", label: pin.label, pin }]}
    onAddKnowledgePin={onAddKnowledgePin}
  /></TooltipProvider>;
}

it("offers files, a pasted image, a map screenshot and saved sources from one + menu", async () => {
  const user = userEvent.setup();
  const onAddKnowledgePin = vi.fn();
  render(<MenuComposer status="empty" onAddKnowledgePin={onAddKnowledgePin} />);
  await user.click(screen.getByRole("button", { name: "Add to this message" }));
  expect(screen.getByRole("menuitem", { name: "Files…" })).toBeInTheDocument();
  expect(screen.getByRole("menuitem", { name: "Paste image" })).toBeInTheDocument();
  // An empty map cannot be captured, and the item says why.
  expect(screen.getByRole("menuitem", { name: /Screenshot of World/ })).toHaveAttribute("aria-disabled", "true");
  expect(screen.getByRole("menuitem", { name: /Screenshot of World/ })).toHaveAttribute("title", "World is empty");
  await user.click(screen.getByRole("menuitem", { name: "Q3 operating review" }));
  expect(onAddKnowledgePin).toHaveBeenCalledWith(pin);
});

it("attaches a screenshot of World when one is drawn and counts it in the tray", async () => {
  const user = userEvent.setup();
  render(<MenuComposer status="ready" onAddKnowledgePin={vi.fn()} />);
  await user.click(screen.getByRole("button", { name: "Add to this message" }));
  await user.click(screen.getByRole("menuitem", { name: "Screenshot of World" }));
  await user.keyboard("{Escape}");
  expect(await screen.findByRole("img", { name: "world.png" })).toBeInTheDocument();
  expect(screen.getByText("1 attachment")).toBeInTheDocument();
});

it("opens the + menu from the whole composer, not from the + button", async () => {
  const user = userEvent.setup();
  render(<MenuComposer status="ready" onAddKnowledgePin={vi.fn()} />);
  const trigger = screen.getByRole("button", { name: "Add to this message" });
  const composer = trigger.closest<HTMLElement>('[data-slot="input-group"]')!;
  // The composer box spans the chat column; the + button sits in its bottom row, 48 px in.
  const boxes = new Map<Element, DOMRect>([[composer, new DOMRect(200, 600, 720, 120)], [trigger, new DOMRect(248, 680, 28, 28)]]);
  const measure = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) { return boxes.get(this) ?? new DOMRect(); });
  // jsdom lays nothing out: a 1280×820 window and a menu 270 px wide, as `.composer-add-menu` sets it.
  const width = vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockImplementation(function (this: HTMLElement) { return this.hasAttribute("data-radix-popper-content-wrapper") ? 270 : 0; });
  const viewWidth = vi.spyOn(Element.prototype, "clientWidth", "get").mockImplementation(function (this: Element) { return this === document.documentElement ? 1280 : 0; });
  const viewHeight = vi.spyOn(Element.prototype, "clientHeight", "get").mockImplementation(function (this: Element) { return this === document.documentElement ? 820 : 0; });
  await user.click(trigger);
  const menu = await screen.findByRole("menu");
  // The menu's left edge meets the composer's, and its bottom sits 10 px above the composer's top edge.
  await waitFor(() => expect(menu.closest<HTMLElement>("[data-radix-popper-content-wrapper]")!.style.transform).toBe("translate(200px, 590px)"));
  for (const spy of [measure, width, viewWidth, viewHeight]) spy.mockRestore();
});

it("marks the + menu as opened by a press or by a key, so only keys get the focus ring", async () => {
  const user = userEvent.setup();
  render(<MenuComposer status="ready" onAddKnowledgePin={vi.fn()} />);
  const trigger = screen.getByRole("button", { name: "Add to this message" });
  // Typing in the composer first is what made WebKit ring the menu a click opened.
  await user.type(screen.getByRole("textbox"), "Hanul");
  await user.click(trigger);
  expect(await screen.findByRole("menu")).toHaveAttribute("data-input", "pointer");
  expect(trigger).toHaveAttribute("data-input", "pointer");
  await user.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
  // From the keyboard the same menu keeps its ring.
  trigger.focus();
  await user.keyboard("{Enter}");
  expect(await screen.findByRole("menu")).toHaveAttribute("data-input", "keyboard");
  expect(trigger).toHaveAttribute("data-input", "keyboard");
});

/** A clipboard as WebKit hands it to a paste: its files as file items, and its text. */
function clipboard(files: File[], text = "") {
  return {
    items: files.map((file) => ({ kind: "file", type: file.type, getAsFile: () => file })),
    files,
    types: [...(files.length ? ["Files"] : []), ...(text ? ["text/plain"] : [])],
    getData: (type: string) => (type === "text/plain" ? text : ""),
  };
}
const png = (name = "image.png", size = 8) => new File([new Uint8Array(size).fill(1)], name, { type: "image/png" });

describe("⌘V in the composer", () => {
  afterEach(() => { vi.useRealTimers(); });

  it("attaches a screenshot on the clipboard under a name of its own, and sends it", async () => {
    const onSend = vi.fn();
    render(<MenuComposer status="closed" onAddKnowledgePin={vi.fn()} onSend={onSend} />);
    const message = screen.getByRole("textbox", { name: "Message Loupist" });
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date(2026, 10, 23, 21, 4));
    const paste = createEvent.paste(message, { clipboardData: clipboard([png()]) });
    fireEvent(message, paste);
    vi.useRealTimers();
    expect(paste.defaultPrevented).toBe(true);
    const thumb = await screen.findByRole("img", { name: "Pasted image 2026-11-23 21.04.png" });
    expect(thumb).toHaveAttribute("src", expect.stringMatching(/^data:image\/png;base64,/));
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(onSend).toHaveBeenCalledOnce());
    expect(onSend.mock.calls[0][1]).toEqual([expect.objectContaining({ filename: "Pasted image 2026-11-23 21.04.png", mediaType: "image/png", url: expect.stringMatching(/^data:image\/png;base64,/) })]);
  });

  it("leaves a spreadsheet range's text to the textarea and attaches no picture of it", () => {
    render(<MenuComposer status="closed" onAddKnowledgePin={vi.fn()} />);
    const message = screen.getByRole("textbox", { name: "Message Loupist" });
    const paste = createEvent.paste(message, { clipboardData: clipboard([png()], "Revenue\t1,204\nMargin\t31%") });
    fireEvent(message, paste);
    expect(paste.defaultPrevented).toBe(false);
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("attaches several images from one paste, numbered, up to the tray's four", async () => {
    render(<MenuComposer status="closed" onAddKnowledgePin={vi.fn()} />);
    const message = screen.getByRole("textbox", { name: "Message Loupist" });
    fireEvent.paste(message, { clipboardData: clipboard([png(), png(), png(), png(), png()]) });
    expect(await screen.findByText("4 attachments")).toBeInTheDocument();
    expect(screen.getAllByRole("img").map((image) => image.getAttribute("alt"))).toEqual([
      expect.stringMatching(/^Pasted image .+\.png$/), expect.stringMatching(/ \(2\)\.png$/), expect.stringMatching(/ \(3\)\.png$/), expect.stringMatching(/ \(4\)\.png$/),
    ]);
    expect(screen.getByRole("alert")).toHaveTextContent("Too many files. Some were not added.");
  });

  it("refuses a pasted image over the image limit with the existing message", async () => {
    render(<MenuComposer status="closed" onAddKnowledgePin={vi.fn()} />);
    const message = screen.getByRole("textbox", { name: "Message Loupist" });
    fireEvent.paste(message, { clipboardData: clipboard([png("image.png", 5 * 1024 * 1024 + 1)]) });
    expect(await screen.findByRole("alert")).toHaveTextContent("Image attachments must be 5 MB or smaller");
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("attaches a file copied in Finder, whose name is the clipboard's text, under its own name", async () => {
    render(<MenuComposer status="closed" onAddKnowledgePin={vi.fn()} />);
    const message = screen.getByRole("textbox", { name: "Message Loupist" });
    const report = new File(["quarter"], "Q3 report.csv", { type: "text/csv" });
    const chart = png("margin chart.png");
    fireEvent.paste(message, { clipboardData: clipboard([report, chart], "Q3 report.csv\nmargin chart.png") });
    expect(await screen.findByText("Q3 report.csv")).toBeVisible();
    expect(screen.getByRole("img", { name: "margin chart.png" })).toBeInTheDocument();
  });

  it("attaches an image a browser copied with its address as text", async () => {
    render(<MenuComposer status="closed" onAddKnowledgePin={vi.fn()} />);
    const message = screen.getByRole("textbox", { name: "Message Loupist" });
    fireEvent.paste(message, { clipboardData: clipboard([png()], "https://example.com/chart.png") });
    expect(await screen.findByRole("img", { name: /^Pasted image / })).toBeInTheDocument();
  });
});
