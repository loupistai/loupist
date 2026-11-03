/**
 * Korean asset-page depth and the sensitivity card (finance-korea-retail-disagreement.md
 * §8). Fictional names; every scenario reaches its screen through the app's own
 * controls.
 */
import type { ChatMessage } from "../types";
import { installAsset, noThesisFixture, PREVIEW_LOUPIST, type AssetFixture } from "./assets";
import { MORROW } from "./finance";
import { prepareFixture, previewKo, type PreviewBridge } from "./fixtures";
import { capitalRowsFixture, koreaDepthFixture, koreaNeedsKeyFixture, morrowWithGrid, previewSensitivityAnswer, previewSensitivityQuestion, type SensitivityVariant } from "./korea";
import type { PreviewScenario } from "./scenarios";

type Helpers = {
  base: (id: string, title: string, landmark: PreviewScenario["landmark"], enter?: PreviewScenario["enter"]) => PreviewScenario;
  select: (selector: string, root?: ParentNode) => Element | null;
  until: <T>(read: () => T | null | false | undefined, reason: string, timeout?: number) => Promise<T>;
  click: (selector: string, root?: ParentNode) => Promise<void>;
  hasLayout: () => boolean;
};

/** The home Thread holds one question and its answer. */
function installThread(bridge: PreviewBridge, messages: () => ChatMessage[]) {
  const select = bridge.loupistSelect.bind(bridge);
  bridge.loupistSelect = async loupistId => {
    const workspace = await select(loupistId);
    const home = workspace.threads.find(thread => thread.home) ?? workspace.threads[0];
    if (home) {
      home.messages = messages();
      home.title = previewKo() ? "솔숲식품 주가에 담긴 기대" : "What Solsup’s price implies";
    }
    return workspace;
  };
}

export function koreaScenarios({ base, select, until, click, hasLayout }: Helpers): PreviewScenario[] {
  /** ⌘K, the asset's symbol, then its row in "What you follow". */
  const openAsset = async (root: HTMLElement, symbol: string, market: string) => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true, ctrlKey: !navigator.userAgent.includes("Mac"), bubbles: true }));
    const input = await until(() => select(".command-palette-input") as HTMLInputElement | null, "Palette did not open");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, symbol);
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await click(`[data-palette-row="following:${symbol}:${market}"]`);
    await until(() => select(".asset-page[aria-label]"), "Asset page did not open");
  };
  const reveal = async (selector: string, reason: string) => {
    const element = await until(() => select(selector), reason);
    if (hasLayout()) element.scrollIntoView({ block: "center" });
  };
  const page = (id: string, title: string, landmark: string, fixture: () => AssetFixture): PreviewScenario => {
    const { symbol, market } = fixture().page;
    return {
      ...base(id, title, () => select(landmark), async root => { await openAsset(root, symbol, market); await reveal(landmark, "The Korean section did not appear"); }),
      setup: async bridge => { await prepareFixture(bridge); await installAsset(bridge, fixture(), 3); },
    };
  };
  const answer = (id: string, title: string, landmark: string, variant: SensitivityVariant): PreviewScenario => ({
    ...base(id, title, () => select(landmark), async root => {
      await until(() => root.querySelector("article.message.assistant.complete"), "Answer did not load");
      await until(() => select(landmark), "The sensitivity card did not appear in the answer");
      // The card's end, with Change the rates, stops above the Jump to latest pill (the thread's scroll padding).
      if (hasLayout()) select(".sensitivity-card")?.scrollIntoView({ block: "end" });
    }),
    setup: async bridge => {
      await prepareFixture(bridge, "answer");
      installThread(bridge, () => [previewSensitivityQuestion(), previewSensitivityAnswer(variant)]);
    },
  });
  return [
    // W16.4: Korean depth on the asset page.
    page("asset-page-korea-depth", "Asset page: a Korean stock's investor flows, record date and capital decision", ".asset-flows .asset-flows-rows", koreaDepthFixture),
    page("asset-page-korea-needs-key", "Asset page: investor flows need Korea Investment", ".asset-flows .asset-flows-needs-key", koreaNeedsKeyFixture),
    page("asset-page-capital-rows", "Asset page: capital raises, convertible bonds and a buyback in plain words", '.asset-changes .asset-change[data-kind="capital"]', capitalRowsFixture),

    // W16.8: the sensitivity card in answers and behind PRICE IMPLIED.
    answer("chat-sensitivity-card", "An answer shows what the price implies as a range, shaded against the last five years", '.sensitivity-card[data-reference] .sensitivity-cell[data-above]', "reference"),
    answer("chat-sensitivity-no-reference", "A range card without a reference: no shading, no legend", '.sensitivity-card[data-operation="reverse_dcf"]:not([data-reference]) .sensitivity-cell[data-base]', "none"),
    answer("chat-sensitivity-null-cells", "A range card with cells outside what the tool solves", ".sensitivity-card .sensitivity-null", "nulls"),
    { ...base("asset-earnings-open-grid", "Results card: Open the grid shows the whole reverse-DCF grid", () => select('[role="dialog"].sensitivity-dialog .sensitivity-card'), async root => {
      await openAsset(root, MORROW.symbol, MORROW.market);
      await reveal(".earnings-grid-link", "Open the grid did not appear");
      await click(".earnings-grid-link");
      await until(() => select('[role="dialog"].sensitivity-dialog .sensitivity-card'), "The grid dialog did not open");
    }), setup: async bridge => {
      await prepareFixture(bridge);
      await installAsset(bridge, noThesisFixture(), 3);
      bridge.earnings.install(PREVIEW_LOUPIST, morrowWithGrid());
    } },
  ];
}
