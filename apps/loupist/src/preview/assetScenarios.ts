/**
 * Asset page and "What changed" scenarios (finance-asset-page.md §8). The shared
 * scenario helpers come from `scenarios.ts`; every scenario reaches its screen
 * through the app's own controls.
 */
import { mockAssetId } from "../bridge/mock/assets";
import type { InboxItem, ScheduledRun } from "../contracts/scheduled";
import { cryptoFixture, HANUL, hanulFixture, installAsset, noThesisFixture, partialFixture, PREVIEW_LOUPIST, previewLineAlert, privateFixture, type AssetFixture } from "./assets";
import { prepareFixture, type PreviewBridge } from "./fixtures";
import { prepareMorningFixture } from "./fixtures/morning";
import type { PreviewScenario } from "./scenarios";

type Helpers = {
  base: (id: string, title: string, landmark: PreviewScenario["landmark"], enter?: PreviewScenario["enter"]) => PreviewScenario;
  select: (selector: string, root?: ParentNode) => Element | null;
  until: <T>(read: () => T | null | false | undefined, reason: string, timeout?: number) => Promise<T>;
  click: (selector: string, root?: ParentNode) => Promise<void>;
  enterMap: (root: HTMLElement) => Promise<Element>;
  hasLayout: () => boolean;
  centerOn: (selector: string, block?: ScrollLogicalPosition, readySelector?: string) => () => Promise<void>;
};

/**
 * Puts Hanul Precision (한울정밀) on the Map as the entity its asset page matched,
 * as the morning fixture does, so the page, the asset card and the inspector agree.
 */
function hanulOnMap(bridge: PreviewBridge) {
  const read = bridge.knowledgeMap.bind(bridge);
  bridge.knowledgeMap = async request => {
    const map = await read(request);
    const node = map.nodes.find(item => item.id === HANUL.entityId);
    if (node) { node.label = "Hanul Precision"; node.localNames = ["한울정밀"]; }
    return map;
  };
}

export function assetScenarios({ base, select, until, click, enterMap, hasLayout, centerOn }: Helpers): PreviewScenario[] {
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
  const page = (id: string, title: string, landmark: string, fixture: () => AssetFixture, visited: number | undefined, extra: { setup?: (bridge: PreviewBridge) => void; enter?: (root: HTMLElement) => Promise<void> } = {}): PreviewScenario => {
    const { symbol, market } = fixture().page;
    return {
      ...base(id, title, () => select(landmark), async root => { await openAsset(root, symbol, market); await extra.enter?.(root); }),
      setup: async bridge => { await prepareFixture(bridge); hanulOnMap(bridge); await installAsset(bridge, fixture(), visited); extra.setup?.(bridge); },
    };
  };
  const checkNow = async () => {
    const button = await until(() => select(".asset-check") ?? select(".asset-page-actions .asset-action-icon:last-child button"), "Check now missing");
    (button as HTMLElement).click();
  };
  return [
    page("asset-page-equity", "Asset page: a weakening thesis with dated, sourced changes", ".asset-changes .asset-change", () => hanulFixture(), 3),
    page("asset-page-no-thesis", "Asset page: why the user follows it is not written yet", ".asset-no-thesis", noThesisFixture, undefined),
    page("asset-page-crypto", "Asset page: a crypto asset's price and drawdown", ".asset-glance", cryptoFixture, 2),
    page("asset-page-private", "Asset page: a private company followed through Sources", ".asset-sources-only", privateFixture, undefined),
    page("asset-page-partial", "Asset page: one source failed and one connection is off", ".asset-check-problem", partialFixture, 3),
    page("asset-page-checking", "Asset page: checking now", '.asset-page-actions [data-slot="spinner"]', () => hanulFixture(), 3, {
      setup: bridge => { bridge.assets.refreshHold = true; },
      enter: checkNow,
    }),
    page("asset-page-offline", "Asset page: offline, showing what was saved", ".asset-page-offline", () => hanulFixture(), 3, {
      setup: () => { Object.defineProperty(window.navigator, "onLine", { configurable: true, get: () => false }); },
    }),
    page("asset-page-line-crossed", "Asset page: a crossed line broke the thesis", '.asset-line[data-status="crossed"]', () => hanulFixture("crossed"), 3),

    { ...base("thread-morning-assets", "Morning message: what changed for what you follow", () => select('.morning-message[aria-busy="false"] .morning-followed'), async () => {
      const followed = await until(() => select(".morning-followed"), "Followed assets did not appear");
      if (hasLayout()) followed.scrollIntoView({ block: "center" });
    }), setup: async bridge => {
      await prepareMorningFixture(bridge, "morning");
      await installAsset(bridge, hanulFixture());
      await installAsset(bridge, cryptoFixture());
      await installAsset(bridge, noThesisFixture());
    } },
    { ...base("thread-red-line", "The Thread says a line the user set was crossed", () => select(".thread-alert"),
      centerOn(".thread-alert", "center", '.morning-message[aria-busy="false"]')), setup: async bridge => {
      await prepareMorningFixture(bridge, "nothing-changed");
      await installAsset(bridge, hanulFixture("crossed"));
      const original = bridge.loupistSelect.bind(bridge);
      bridge.loupistSelect = async loupistId => {
        const workspace = await original(loupistId);
        workspace.threads.find(thread => thread.home)?.messages.push(previewLineAlert());
        return workspace;
      };
    } },
    { ...base("briefing-assets", "A briefing's What you follow, with reads and sourced rows", () => select(".briefing-assets"), async root => {
      await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "4", metaKey: true, bubbles: true }));
      await click('[data-schedule-task="preview-briefing"] .schedule-task-summary', root);
      await click('[data-schedule-run="preview-briefing-1"]', root);
      const section = await until(() => select(".briefing-assets"), "Followed assets did not appear");
      if (hasLayout()) section.scrollIntoView({ block: "center" });
    }), setup: async bridge => {
      await prepareFixture(bridge, "answer");
      hanulOnMap(bridge);
      bridge.includeBriefingBody = true;
      await bridge.briefingSettingsSet({ loupistId: PREVIEW_LOUPIST, enabled: true });
      const assets = [hanulFixture(), cryptoFixture()].map(({ page, delta }) => ({
        assetId: page.assetId, symbol: page.symbol, market: page.market, name: page.name,
        verdict: page.read?.verdict ?? null, verdictSince: page.read?.since ?? null, rows: (delta?.rows ?? []).filter(row => row.material).slice(0, 3),
      }));
      const withAssets = <T extends { briefingContext?: InboxItem["briefingContext"] }>(item: T) => item.briefingContext ? { ...item, briefingContext: { ...item.briefingContext, assets } } : item;
      const inbox = bridge.inboxList.bind(bridge);
      bridge.inboxList = async request => { const page = await inbox(request); return { ...page, items: page.items.map(withAssets) }; };
      const runs = bridge.scheduledTaskRuns.bind(bridge);
      bridge.scheduledTaskRuns = async request => (await runs(request)).map((run: ScheduledRun) => run.briefing_context ? { ...run, briefing_context: { ...run.briefing_context, assets } } : run);
    } },
    { ...base("chat-receipt-thesis", "An answer's receipt names the read it moved", () => select('.chat-receipt [data-receipt^="thesis:"]'), async root => {
      await until(() => root.querySelector("article.message.assistant.complete"), "Answer did not load");
      const segment = await until(() => select('.chat-receipt [data-receipt^="thesis:"]'), "The moved read did not reach the receipt");
      if (hasLayout()) segment.scrollIntoView({ block: "center" });
    }), setup: async bridge => {
      await prepareFixture(bridge, "answer");
      hanulOnMap(bridge);
      await installAsset(bridge, hanulFixture());
      const assetId = mockAssetId(HANUL.symbol, HANUL.market);
      bridge.knowledgeGrowthEvents = async () => ({ events: [{
        eventId: "receipt-thesis", loupistId: PREVIEW_LOUPIST, originChatId: "preview-chat", originMessageId: "preview-answer", atMs: Date.now() - 60_000,
        kind: "thesis_moved", subjectId: assetId, recordId: assetId, actor: "loupist", summary: "Hanul Precision", beforeSummary: "holding", afterSummary: "weakening",
      }], nextCursor: null });
    } },
    { ...base("map-inspector-thesis", "The Map inspector leads with the user's thesis", () => select(".map-inspector .asset-thesis-summary"), async root => {
      const map = await enterMap(root);
      const input = await until(() => map.querySelector<HTMLInputElement>('[role="combobox"]'), "Map search did not load");
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "Hanul");
      input.dispatchEvent(new Event("input", { bubbles: true }));
      await click('[role="option"]', map);
      await until(() => select(".map-inspector .asset-thesis-summary"), "The thesis did not appear in the inspector");
    }), setup: async bridge => {
      await prepareFixture(bridge);
      hanulOnMap(bridge);
      await installAsset(bridge, hanulFixture());
    } },
  ];
}
