import i18n from "@/i18n";
import type { ChatThread } from "../../types";
import { chatMarkdown } from "../../chat/exportChat";
import { printView } from "./printView";
import { OpenProvider } from "../navigation/Open";

/** Paper keeps every link's words; nothing on it opens. */
const nothingOpens = async () => "unavailable" as const;

/**
 * A Chat as paper: the same transcript as the Markdown export (title line,
 * models, each message with its role and time, citations as footnotes, the AI
 * notice last), drawn by the answer renderer (shell-briefings §2.6, §7).
 */
export async function printChat(thread: ChatThread, loupistName: string) {
  const { MessageResponse } = await import("@/components/ai-elements/message-response");
  await printView(thread.title, <article className="print-document" lang={i18n.language}>
    <div className="message assistant"><div className="message-content">
      <OpenProvider open={nothingOpens}><MessageResponse>{chatMarkdown(thread, loupistName, Date.now(), { notice: false })}</MessageResponse></OpenProvider>
    </div></div>
    {/* The renderer gathers footnotes at the end, so the notice follows them here. */}
    <p className="print-notice">{i18n.t("chat:chatExport.notice")}</p>
  </article>);
}

/** Controls that mean nothing on paper; everything else keeps its words. */
const SCREEN_ONLY = [
  "[data-print=\"omit\"]",
  ".chat-progress",
  ".message-actions",
  ".message-details-trigger",
  ".answer-feedback",
  ".chat-artifact-block-actions",
  ".next-step-chips",
].join(", ");

/**
 * A paper copy of a reader as it is drawn now (a briefing): controls removed,
 * folded sections opened, buttons kept as their words, and canvases (charts)
 * frozen into pictures, since a copied canvas is blank.
 */
export function printableCopy(source: HTMLElement) {
  const copy = source.cloneNode(true) as HTMLElement;
  const canvases = [...source.querySelectorAll("canvas")];
  copy.querySelectorAll("canvas").forEach((canvas, index) => {
    const picture = document.createElement("img");
    picture.alt = "";
    picture.className = "print-canvas";
    try { picture.src = canvases[index]?.toDataURL("image/png") ?? ""; } catch { /* A canvas the page cannot read stays out. */ }
    picture.style.width = `${canvases[index]?.clientWidth ?? 0}px`;
    if (picture.src) canvas.replaceWith(picture);
    else canvas.remove();
  });
  copy.querySelectorAll(SCREEN_ONLY).forEach((node) => node.remove());
  copy.querySelectorAll("details").forEach((details) => { details.open = true; });
  copy.querySelectorAll("button").forEach((button) => {
    const words = document.createElement("span");
    words.className = button.className;
    words.dataset.printWords = "";
    words.append(...button.childNodes);
    button.replaceWith(words);
  });
  // The copy lives beside the original; repeated ids would confuse labels and anchors.
  copy.querySelectorAll("[id]").forEach((node) => node.removeAttribute("id"));
  copy.removeAttribute("id");
  copy.classList.add("print-document");
  return copy;
}

/** Prints what a reader shows now, under `title`. */
export function printReader(source: HTMLElement, title: string) {
  return printView(title, printableCopy(source));
}
