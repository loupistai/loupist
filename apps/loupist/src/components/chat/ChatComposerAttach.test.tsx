import { useState } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TooltipProvider } from "../ui/tooltip";
import { ChatComposer } from "./ChatComposer";
import type { KnowledgePin } from "../../types";

const pin = { kind: "source" as const, loupistId: "loupist", recordId: "sources", revisionId: "rev", objectId: "src-1", label: "Q3 operating review" };

function MenuComposer({ status, onAddKnowledgePin }: { status: "ready" | "empty" | "closed"; onAddKnowledgePin: (value: KnowledgePin) => void }) {
  const [prompt, setPrompt] = useState("");
  return <TooltipProvider><ChatComposer
    prompt={prompt} onPromptChange={setPrompt}
    privateChat={false} isRunning={false} error={null} streamAnnouncement=""
    models={[{ id: "model", provider: "test", model: "model", name: "Model", input: ["text", "image"], reasoning: true, context_window: 10000, max_tokens: 1000, thinking_levels: ["medium"], thinking_level_map: {}, run_controls: [], credential_source: "saved" }]} providers={[]} modelSelection={{ model_profile_id: "model", thinking_level: "medium", run_options: {} }} modelAuto onModelResetToAuto={() => {}}
    skills={[]} plugins={[]}
    onPrivateChatChange={vi.fn()} onModelSelectionChange={vi.fn()}
    onSend={vi.fn()} onFollowUp={vi.fn()} onAbort={vi.fn()}
    mapScreenshot={{ status: () => status, take: async () => new File([new Uint8Array([1])], "map.png", { type: "image/png" }) }}
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
  expect(screen.getByRole("menuitem", { name: /Screenshot of the map/ })).toHaveAttribute("aria-disabled", "true");
  expect(screen.getByRole("menuitem", { name: /Screenshot of the map/ })).toHaveAttribute("title", "The map is empty");
  await user.click(screen.getByRole("menuitem", { name: "Q3 operating review" }));
  expect(onAddKnowledgePin).toHaveBeenCalledWith(pin);
});

it("attaches a screenshot of the map when one is drawn and counts it in the tray", async () => {
  const user = userEvent.setup();
  render(<MenuComposer status="ready" onAddKnowledgePin={vi.fn()} />);
  await user.click(screen.getByRole("button", { name: "Add to this message" }));
  await user.click(screen.getByRole("menuitem", { name: "Screenshot of the map" }));
  await user.keyboard("{Escape}");
  expect(await screen.findByRole("img", { name: "map.png" })).toBeInTheDocument();
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
