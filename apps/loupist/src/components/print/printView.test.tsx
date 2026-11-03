import { act, fireEvent } from "@testing-library/react";
import type { ChatThread } from "../../types";
import { printChat, printableCopy } from "./printDocuments";
import { endPrint } from "./printView";

const thread: ChatThread = {
  id: "chat", loupist_id: "loupist", title: "Margin decline", updated_at: "2026-10-28T00:00:00Z", use_memory: true, update_memory: true,
  messages: [
    { id: "q", role: "user", content: "Why did margins fall?", created_at: "2026-10-28T00:00:00Z", status: "complete" },
    { id: "a", role: "assistant", content: "Qualification costs rose at the new line [1](#evidence-e1).", created_at: "2026-10-28T00:00:10Z", status: "complete",
      evidence_refs: [{ id: "e1", title: "Q3 operating review", kind: "original", url: "https://hanul.example/q3", tool_name: "web_fetch", retrieved_at: "2026-10-28T00:00:00Z", excerpt: "", excerpt_truncated: false, content_sha256: "a".repeat(64), warnings: [] }] },
  ],
};

const sheet = () => document.querySelector<HTMLElement>(".print-sheet");

describe("print to PDF (X9)", () => {
  let print: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    document.title = "Loupist";
    print = vi.spyOn(window, "print").mockImplementation(() => undefined);
  });
  afterEach(() => {
    act(() => endPrint());
    print.mockRestore();
  });

  it("prints a chat as its transcript on a sheet outside the app, then gives the page back", async () => {
    const app = document.body.appendChild(document.createElement("main"));
    app.textContent = "The app itself";
    await act(async () => { await printChat(thread, "Quality Compounder"); });
    expect(print).toHaveBeenCalledTimes(1);
    const paper = sheet()!;
    expect(paper.parentElement).toBe(document.body);
    // The transcript: title, who wrote each message, the answer, its source as a footnote and the AI notice.
    expect(paper.querySelector("h1")).toHaveTextContent("Margin decline");
    expect(paper).toHaveTextContent("You ·");
    expect(paper).toHaveTextContent("Quality Compounder ·");
    expect(paper).toHaveTextContent("Qualification costs rose at the new line");
    expect(paper).toHaveTextContent("Q3 operating review — https://hanul.example/q3");
    // The AI notice is the last line, after the footnotes.
    expect(paper.textContent?.trim().endsWith("AI can be wrong. Not a licensed financial adviser.")).toBe(true);
    expect(paper.textContent?.match(/AI can be wrong/g)).toHaveLength(1);
    expect(paper).not.toHaveTextContent("The app itself");
    // Print media shows only the sheet; the PDF is named after the chat.
    expect(document.documentElement).toHaveAttribute("data-printing");
    expect(document.title).toBe("Margin decline");
    act(() => { window.dispatchEvent(new Event("afterprint")); });
    expect(sheet()).toBeNull();
    expect(document.documentElement).not.toHaveAttribute("data-printing");
    expect(document.title).toBe("Loupist");
    app.remove();
  });

  it("keeps the sheet while the print dialog is open and removes it at the next press in the app", async () => {
    await act(async () => { await printChat(thread, "Quality Compounder"); });
    expect(sheet()).not.toBeNull();
    fireEvent.pointerDown(document.body);
    expect(sheet()).toBeNull();
  });

  it("gives the page back when the print dialog cannot open", async () => {
    print.mockImplementation(() => { throw new Error("print is not allowed"); });
    await expect(printChat(thread, "Quality Compounder")).rejects.toThrow("print is not allowed");
    expect(sheet()).toBeNull();
    expect(document.title).toBe("Loupist");
  });

  it("copies a reader for paper: controls out, folded parts open, buttons kept as words, charts frozen", () => {
    const reader = document.createElement("article");
    reader.id = "reader";
    reader.innerHTML = `<button data-print="omit">Back</button><h2 id="title">Morning briefing</h2>
      <button class="briefing-row-link">To check by Nov 4: Inventory days below 50</button>
      <section class="chat-progress">Worked 0:12</section><div class="message-actions"><button aria-label="Copy"></button></div>
      <details><summary>What was checked</summary><p>Companies: NVDA</p></details><canvas width="10" height="10"></canvas>`;
    const toDataURL = vi.spyOn(HTMLCanvasElement.prototype, "toDataURL").mockReturnValue("data:image/png;base64,AA==");
    const copy = printableCopy(reader);
    toDataURL.mockRestore();
    expect(copy).not.toHaveTextContent("Back");
    expect(copy).not.toHaveTextContent("Worked");
    expect(copy.querySelector("button")).toBeNull();
    expect(copy).toHaveTextContent("To check by Nov 4: Inventory days below 50");
    expect(copy.querySelector("details")!.open).toBe(true);
    expect(copy.querySelector("canvas")).toBeNull();
    expect(copy.querySelector("img.print-canvas")).toHaveAttribute("src", "data:image/png;base64,AA==");
    expect(copy.querySelector("[id]")).toBeNull();
    // The reader on screen is untouched.
    expect(reader.querySelector("button[data-print=\"omit\"]")).not.toBeNull();
  });
});
