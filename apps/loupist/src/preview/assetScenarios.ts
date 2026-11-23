/**
 * Asset page scenarios after the thin host: the replayed price with its source and
 * date, a kept price after a failed or disconnected read, no price yet, Check now and
 * the thesis. The shared helpers come from `scenarios.ts`; every scenario reaches its
 * screen through the app's own controls.
 */
import { failedFixture, HANUL, HANUL_ID, hanulFixture, installAsset, nameOnlyFixture, needsConnectionFixture, noCurrencyFixture, noThesisFixture, nvidiaFixture, PREVIEW_LOUPIST, previewLineAlert, type AssetFixture } from "./assets";
import { prepareFixture, type PreviewBridge } from "./fixtures";
import { previewRates, showInDisplayCurrency } from "./displayCurrency";
import { prepareMorningFixture } from "./fixtures/morning";
import type { PreviewScenario } from "./scenarios";
import { asSession } from "./scenarios/kit";

type Helpers = {
  base: (id: string, title: string, landmark: PreviewScenario["landmark"], enter?: PreviewScenario["enter"]) => PreviewScenario;
  select: (selector: string, root?: ParentNode) => Element | null;
  until: <T>(read: () => T | null | false | undefined, reason: string, timeout?: number) => Promise<T>;
  click: (selector: string, root?: ParentNode) => Promise<void>;
  hasLayout: () => boolean;
  centerOn: (selector: string, block?: ScrollLogicalPosition, readySelector?: string) => () => Promise<void>;
};

/**
 * Puts Hanul Precision (한울정밀) on the Map as the entity its asset page matched,
 * as the morning fixture does, so the page and the asset card agree.
 */
function hanulOnMap(bridge: PreviewBridge) {
  const read = bridge.knowledgeMap.bind(bridge);
  bridge.knowledgeMap = async request => {
    const map = await read(request);
    const node = map.nodes.find(item => item.id === HANUL.entityId);
    if (node) { node.label = HANUL.name; node.localNames = ["한울정밀"]; }
    return map;
  };
}

export function assetScenarios({ base, select, until, click, hasLayout, centerOn }: Helpers): PreviewScenario[] {
  /** ⌘K, part of the asset's name, then its row in "What you follow". */
  const openAsset = async (root: HTMLElement, query: string, assetId: string) => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true, ctrlKey: !navigator.userAgent.includes("Mac"), bubbles: true }));
    const input = await until(() => select(".command-palette-input") as HTMLInputElement | null, "Palette did not open");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, query);
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await click(`[data-palette-row="following:${assetId}"]`);
    await until(() => select(".asset-page[aria-label]"), "Asset page did not open");
  };
  const page = (id: string, title: string, landmark: string, fixture: () => AssetFixture, extra: { setup?: (bridge: PreviewBridge) => void | Promise<void>; enter?: (root: HTMLElement) => Promise<void> } = {}): PreviewScenario => {
    const { asset } = fixture();
    return {
      ...base(id, title, () => select(landmark), async root => {
        await openAsset(root, asset.name.slice(0, 6), asset.assetId);
        await extra.enter?.(root);
        if (hasLayout()) select(landmark)?.scrollIntoView({ block: "center" });
      }),
      setup: async bridge => { await prepareFixture(bridge); hanulOnMap(bridge); installAsset(bridge, fixture()); await extra.setup?.(bridge); },
    };
  };
  const checkNow = async () => {
    const button = await until(() => select(".asset-check") as HTMLButtonElement | null, "Check now missing");
    await until(() => !button.disabled, "Check now stayed disabled");
    button.click();
  };
  return [
    page("asset-page-equity", "Asset page: a won price as of its stated date with its source chip, the chart and a weakening thesis", '.asset-quote[data-price-read="ready"] .loupist-citation', () => hanulFixture()),
    page("asset-page-stated-time", "Asset page: a US-dollar price whose provider stated a time, a year of history points", '.asset-quote[data-price-read="ready"] [data-price-date*="T"]', nvidiaFixture),
    page("asset-page-no-price", "Asset page: just followed, no saved read yet, so no price and no chart", '.asset-quote [data-price-note="none"]', noThesisFixture),
    page("asset-page-price-failed", "Asset page: the last replay failed, the earlier price kept with its date", '.asset-quote [data-price-note="failed"]', failedFixture),
    page("asset-page-price-needs-connection", "Asset page: the price's connection is off, the earlier price kept, one way to turn it on", '.asset-quote [data-price-note="needs-connection"] button', needsConnectionFixture),
    page("asset-page-no-currency", "Asset page: neither the result nor the listing states a currency, so the line says it is unknown", '.asset-quote[data-price-read="ready"]', noCurrencyFixture,
      { setup: bridge => showInDisplayCurrency(bridge, previewRates("KRW")) }),
    page("asset-page-name-only", "Asset page: an asset followed by its long name alone, with one reason and no price yet", '.asset-quote [data-price-note="none"]', nameOnlyFixture),
    page("asset-page-checking", "Asset page: checking now", '.asset-page-actions [data-slot="spinner"]', nvidiaFixture, {
      setup: bridge => { bridge.assets.checkHold = true; },
      enter: checkNow,
    }),
    page("asset-page-check-setup", "Asset page: Check now without a saved read asks the Loupist to find a price", '.asset-quote [data-price-note="setup"]', noThesisFixture, { enter: checkNow }),
    page("asset-page-offline", "Asset page: offline, Check now unavailable, the stored price shown", ".asset-page-offline", () => hanulFixture(), {
      setup: () => { Object.defineProperty(window.navigator, "onLine", { configurable: true, get: () => false }); },
    }),
    page("asset-page-line-crossed", "Asset page: a crossed line broke the thesis", '.asset-line[data-status="crossed"]', () => hanulFixture("crossed")),
    // The display currency (currency design §5): one "≈" line under the price's source, or nothing.
    ...(["KRW", "CAD", "JPY"] as const).map(currency => page(`asset-page-display-${currency.toLowerCase()}`, `Asset page: a US-dollar price with an approximate amount in ${currency}`,
      `.asset-quote [data-converted-amount="${currency}"]`, nvidiaFixture, { setup: bridge => showInDisplayCurrency(bridge, previewRates(currency)) })),
    page("asset-page-display-same", "Asset page: the display currency is the price's own, so nothing is added", '.asset-quote[data-price-read="ready"]', nvidiaFixture,
      { setup: bridge => showInDisplayCurrency(bridge, previewRates("USD")) }),
    page("asset-page-display-behind", "Asset page: rates behind after a failed refresh, shown with the table's own date", '.asset-quote [data-converted-amount="KRW"]', nvidiaFixture,
      { setup: bridge => showInDisplayCurrency(bridge, previewRates("KRW", { behind: "provider_failed", date: "2026-11-13" })) }),

    { ...base("thread-morning-assets", "Bot greeting: questions about what you follow", () => select('.bot-greeting[aria-busy="false"] [data-ask="status"]'), async () => {
      const asks = await until(() => select(".bot-asks"), "The greeting's questions did not appear");
      if (hasLayout()) asks.scrollIntoView({ block: "center" });
    }), setup: async bridge => {
      await prepareMorningFixture(bridge, "morning");
      installAsset(bridge, hanulFixture());
      installAsset(bridge, nvidiaFixture());
      installAsset(bridge, noThesisFixture());
    } },
    { ...base("thread-red-line", "The Thread says a line the user set was crossed", () => select(".thread-alert"),
      centerOn(".thread-alert", "center", '.bot-greeting[aria-busy="false"]')), setup: async bridge => {
      await prepareMorningFixture(bridge, "nothing-changed");
      installAsset(bridge, hanulFixture("crossed"));
      const original = bridge.loupistSelect.bind(bridge);
      bridge.loupistSelect = async loupistId => {
        const workspace = await original(loupistId);
        workspace.threads.find(thread => thread.home)?.messages.push(previewLineAlert());
        return workspace;
      };
    } },
    { ...base("chat-receipt-thesis", "An answer's receipt names the read it moved", () => select('.chat-receipt [data-receipt^="thesis:"]'), async root => {
      await until(() => root.querySelector("article.message.assistant.complete"), "Answer did not load");
      const segment = await until(() => select('.chat-receipt [data-receipt^="thesis:"]'), "The moved read did not reach the receipt");
      if (hasLayout()) segment.scrollIntoView({ block: "center" });
    }), setup: async bridge => {
      await prepareFixture(bridge, "answer");
      hanulOnMap(bridge);
      installAsset(bridge, hanulFixture());
      bridge.knowledgeGrowthEvents = async () => ({ events: [{
        eventId: "receipt-thesis", loupistId: PREVIEW_LOUPIST, originChatId: "preview-chat", originMessageId: "preview-answer", atMs: Date.now() - 60_000,
        kind: "thesis_moved", subjectId: HANUL_ID, recordId: HANUL_ID, actor: "loupist", summary: HANUL.name, beforeSummary: "holding", afterSummary: "weakening",
      }], nextCursor: null });
      // A research session's receipt stays on the page; Bot's shows only under the pointer.
      asSession(bridge);
    } },
  ];
}
