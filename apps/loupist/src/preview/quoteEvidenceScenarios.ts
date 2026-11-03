/**
 * Saved equity quotes. The nested `content[].text` basket and a cut sibling
 * are fixture data for the inline receipt; nothing is fetched.
 */
import type { ChatMessage } from "../types";
import { evidenceFixture } from "../test/evidenceFixtures";
import { prepareFixture, previewKo, type PreviewBridge } from "./fixtures";
import type { PreviewScenario } from "./scenarios";

const quotes = [
  { symbol: "AAPL", name: "Apple", currency: "USD", last_price: 271.4, last_timestamp: "2026-10-11T14:30:00Z" },
  { symbol: "MSFT", name: "Microsoft", currency: "USD", last_price: 415.2 },
  { symbol: "NVDA", name: "NVIDIA", currency: "USD", last_price: 131.28 },
];

const nestedExcerpt = JSON.stringify({
  content: [{ type: "text", text: JSON.stringify({ results: quotes.slice(0, 2) }) }],
}, null, 2);

const truncatedExcerpt = JSON.stringify({
  content: [{ type: "text", text: JSON.stringify({ results: quotes }) }],
}).slice(0, 220);

const complete = evidenceFixture({
  id: "preview-quote",
  title: "Equity price quote",
  tool_name: "mcp__openbb__equity_price_quote",
  provider: "Yahoo Finance",
  url: "https://finance.example/quotes",
  retrieved_at: "2026-10-11T15:04:00Z",
  published_at: undefined,
  excerpt: nestedExcerpt,
});

const truncated = evidenceFixture({
  id: "preview-quote-cut",
  title: "Equity price quote, cut off",
  tool_name: "equity_price_quote",
  provider: "Yahoo Finance",
  url: undefined,
  retrieved_at: "2026-10-11T15:04:00Z",
  published_at: undefined,
  excerpt: truncatedExcerpt,
  excerpt_truncated: true,
});

type Helpers = {
  base: (id: string, title: string, landmark: PreviewScenario["landmark"], enter?: PreviewScenario["enter"]) => PreviewScenario;
  select: (selector: string, root?: ParentNode) => Element | null;
  until: <T>(read: () => T | null | false | undefined, reason: string, timeout?: number) => Promise<T>;
  click: (selector: string, root?: ParentNode) => Promise<void>;
};

export function quoteEvidenceScenarios({ base, select, until, click }: Helpers): PreviewScenario[] {
  return [{
    ...base("quote-evidence", "A price-quote basket opens as a compact receipt", () => select(".quote-evidence"), async (root) => {
      await until(() => root.querySelector("button.loupist-citation"), "Quote citation did not render");
      await click("button.loupist-citation");
      const dialog = await until(() => select(".quote-evidence"), "Quote receipt did not open");
      // jsdom has no layout. In the preview, the receipt has to sit in the window beside the citation.
      if (document.documentElement.clientWidth > 0 && dialog instanceof HTMLElement) {
        const source = root.querySelector("button.loupist-citation");
        let detail = "not measured";
        const besideCitation = () => {
          if (!(source instanceof HTMLElement)) return null;
          const box = dialog.getBoundingClientRect();
          const chip = source.getBoundingClientRect();
          const viewW = window.innerWidth;
          const viewH = window.innerHeight;
          const gap = box.bottom < chip.top ? chip.top - box.bottom : box.top > chip.bottom ? box.top - chip.bottom : 0;
          detail = `${viewW}×${viewH} top ${Math.round(box.top)} bottom ${Math.round(box.bottom)} gap ${Math.round(gap)}`;
          const inside = box.width >= 8 && box.height >= 8 && box.top >= -1 && box.left >= -1 && box.bottom <= viewH + 1 && box.right <= viewW + 1;
          const composer = root.querySelector(".composer-shell");
          const clearOfComposer = !(composer instanceof HTMLElement) || box.bottom <= composer.getBoundingClientRect().top + 1;
          return inside && gap <= 24 && clearOfComposer ? dialog : null;
        };
        try {
          await until(besideCitation, "Quote receipt was not placed");
        } catch (error) {
          throw new Error(`Quote receipt is not inside the window beside its citation (${detail})`, { cause: error });
        }
        const scroller = source instanceof HTMLElement ? source.closest(".message-scroll") : null;
        if (scroller instanceof HTMLElement && source instanceof HTMLElement && scroller.scrollHeight > scroller.clientHeight + 8) {
          const before = source.getBoundingClientRect().top;
          scroller.scrollTop += 48;
          await until(() => Math.abs(source.getBoundingClientRect().top - before) > 1 ? source : null, "Quote thread did not scroll");
          try {
            await until(besideCitation, "Quote receipt did not follow its citation");
          } catch (error) {
            throw new Error(`Quote receipt left its citation after scrolling (${detail})`, { cause: error });
          }
        }
      }
    }),
    setup: async (bridge: PreviewBridge) => {
      await prepareFixture(bridge);
      const selectLoupist = bridge.loupistSelect.bind(bridge);
      bridge.loupistSelect = async (loupistId) => {
        const workspace = await selectLoupist(loupistId);
        const home = workspace.threads.find((thread) => thread.home) ?? workspace.threads[0];
        if (home && !home.messages.some((item) => item.id === "preview-quote-answer")) {
          const message: ChatMessage = {
            id: "preview-quote-answer",
            role: "assistant",
            status: "complete",
            created_at: "2026-10-11T15:05:00Z",
            content: previewKo()
              ? `저장한 시세는 [가격 시세](#evidence-${complete.id})이고, 잘린 원문은 [잘린 시세](#evidence-${truncated.id})예요.`
              : `The saved quotes are the [price quote](#evidence-${complete.id}). The cut original is the [truncated quote](#evidence-${truncated.id}).`,
            evidence_refs: [complete, truncated],
          };
          home.messages.push(message);
        }
        return workspace;
      };
    },
  }];
}
