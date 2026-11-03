/**
 * Results before and after (finance-earnings-snapshot.md §8), local watchers
 * (finance-watchers.md §8) and class watchers (finance-crypto-and-templates.md
 * §8). Fictional names; every scenario reaches its screen through the app's
 * own controls.
 */
import type { ChatEvidenceRef, InboxItem, ScheduledRun } from "../types";
import { cryptoFixture, hanulFixture, installAsset, noThesisFixture, PREVIEW_LOUPIST } from "./assets";
import { FIXTURE_TIME, prepareFixture, prepareMorningFixture, type PreviewBridge } from "./fixtures";
import {
  installWatch, MORROW, morrowSnapshot, previewBriefingEarnings, previewCommodityWatchEvents, previewCommodityWatchSettings,
  previewCryptoWatchEvents, previewCryptoWatchSettings, previewEarningsMessage, previewEarningsTask, previewWatchEvents,
  previewWatchMessage, previewWatchSettings, SOLSUP, solsupFixture, solsupSnapshot, type MorrowState,
} from "./finance";
import type { PreviewScenario } from "./scenarios";

type Helpers = {
  base: (id: string, title: string, landmark: PreviewScenario["landmark"], enter?: PreviewScenario["enter"]) => PreviewScenario;
  select: (selector: string, root?: ParentNode) => Element | null;
  until: <T>(read: () => T | null | false | undefined, reason: string, timeout?: number) => Promise<T>;
  click: (selector: string, root?: ParentNode) => Promise<void>;
  hasLayout: () => boolean;
  navigate: (root: HTMLElement, tab: "map" | "schedule" | "settings") => Promise<void>;
  centerOn: (selector: string, block?: ScrollLogicalPosition, readySelector?: string) => () => Promise<void>;
};

/** Adds a host-posted message to the home Thread, as Rust posts it. */
function postToThread(bridge: PreviewBridge, message: () => import("../types").ChatMessage) {
  const original = bridge.loupistSelect.bind(bridge);
  bridge.loupistSelect = async loupistId => {
    const workspace = await original(loupistId);
    const home = workspace.threads.find(thread => thread.home);
    if (home && !home.messages.some(item => item.id === message().id)) home.messages.push(message());
    return workspace;
  };
}

/** Puts results rows or alerts into the preview briefing's host-computed context. */
function briefingContext(bridge: PreviewBridge, extra: Partial<NonNullable<InboxItem["briefingContext"]>>) {
  const withExtra = <T extends { briefingContext?: InboxItem["briefingContext"] }>(item: T) => item.briefingContext ? { ...item, briefingContext: { ...item.briefingContext, ...extra } } : item;
  const inbox = bridge.inboxList.bind(bridge);
  bridge.inboxList = async request => { const page = await inbox(request); return { ...page, items: page.items.map(withExtra) }; };
  const runs = bridge.scheduledTaskRuns.bind(bridge);
  bridge.scheduledTaskRuns = async request => (await runs(request)).map((run: ScheduledRun) => run.briefing_context ? { ...run, briefing_context: { ...run.briefing_context, ...extra } } : run);
}

export function financeScenarios({ base, select, until, click, hasLayout, navigate, centerOn }: Helpers): PreviewScenario[] {
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
  const reveal = async (selector: string, reason: string, block: ScrollLogicalPosition = "center") => {
    const element = await until(() => select(selector), reason);
    if (hasLayout()) element.scrollIntoView({ block });
  };
  const earnings = (id: string, title: string, state: MorrowState, landmark: string, extra: (bridge: PreviewBridge) => void = () => {}): PreviewScenario => ({
    ...base(id, title, () => select(landmark), async root => { await openAsset(root, MORROW.symbol, MORROW.market); await reveal(landmark, "The results card did not appear"); }),
    setup: async bridge => {
      await prepareFixture(bridge);
      await installAsset(bridge, noThesisFixture(), 3);
      bridge.earnings.install(PREVIEW_LOUPIST, morrowSnapshot(state));
      extra(bridge);
    },
  });
  const openSchedule = async (root: HTMLElement, task: string) => {
    await navigate(root, "schedule");
    await click(`[data-schedule-task="${task}"] .schedule-task-summary`, root);
  };
  const watching = (id: string, title: string, landmark: string, settings: () => ReturnType<typeof previewWatchSettings>, events: () => ReturnType<typeof previewWatchEvents>): PreviewScenario => ({
    // Watching opens at its header: title, status, Check now or Resume, and the privacy line above the matrix.
    ...base(id, title, () => select(landmark) && select(".watching-head"), async root => {
      await openSchedule(root, "watching");
      await until(() => select(landmark), "Watching did not open");
      await reveal(".watching-reader", "Watching did not open", "start");
    }),
    setup: async bridge => {
      await prepareFixture(bridge, "answer");
      for (const fixture of [hanulFixture(), noThesisFixture(), solsupFixture()]) await installAsset(bridge, fixture);
      installWatch(bridge, settings(), events());
    },
  });
  const briefing = (id: string, title: string, landmark: string, extra: () => Partial<NonNullable<InboxItem["briefingContext"]>>): PreviewScenario => ({
    ...base(id, title, () => select(landmark), async root => {
      await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
      await openSchedule(root, "preview-briefing");
      await click('[data-schedule-run="preview-briefing-1"]', root);
      await reveal(landmark, "The briefing rows did not appear");
    }),
    setup: async bridge => {
      await prepareFixture(bridge, "answer");
      bridge.includeBriefingBody = true;
      await bridge.briefingSettingsSet({ loupistId: PREVIEW_LOUPIST, enabled: true });
      // Built at setup, after the preview clock is frozen.
      briefingContext(bridge, extra());
    },
  });
  return [
    // W16.6: results before and after.
    earnings("asset-earnings-upcoming", "Results card: results in five days, expectations not captured yet", "upcoming", '.earnings-card[data-status="upcoming"] .earnings-prepare'),
    earnings("asset-earnings-preparing", "Results card: capturing what's expected", "preparing", '.earnings-prepare[aria-busy="true"]'),
    earnings("asset-earnings-before", "Results card: expectations captured the day before", "before", '.earnings-card[data-status="before_captured"] .earnings-table, .earnings-card[data-status="before_captured"] .earnings-stack'),
    earnings("asset-earnings-after", "Results card: expected and reported side by side, with the price reaction and the call's tone", "after", '.earnings-card[data-status="final"] .earnings-subrows [data-row="tone"]'),
    earnings("asset-earnings-revised", "Results card: the final report revised a preliminary number", "revised", ".earnings-card .earnings-revision"),
    earnings("asset-earnings-late-capture", "Results card: expectations captured after the fact", "late", ".earnings-card .earnings-note"),
    earnings("asset-earnings-no-expectations", "Results card: no company guidance and no consensus key", "none", ".earnings-card .earnings-none"),
    { ...base("asset-earnings-korea-chain", "Results card: 잠정실적 filed, the quarterly report due by its legal deadline", () => select(".earnings-chain li:not([data-filed])"), async root => {
      await openAsset(root, SOLSUP.symbol, SOLSUP.market);
      await reveal(".earnings-chain li:not([data-filed])", "The filing chain did not appear");
    }), setup: async bridge => {
      await prepareFixture(bridge);
      await installAsset(bridge, solsupFixture(), 3);
      bridge.earnings.install(PREVIEW_LOUPIST, solsupSnapshot());
    } },
    { ...base("thread-earnings-reported", "The Thread says a followed company reported results", () => select('.thread-earnings[data-outcome="reported"] .earnings-mini'),
      centerOn('.thread-earnings[data-outcome="reported"]', "center", '.morning-message[aria-busy="false"]')), setup: async bridge => {
      await prepareMorningFixture(bridge, "nothing-changed");
      await installAsset(bridge, noThesisFixture());
      bridge.earnings.install(PREVIEW_LOUPIST, morrowSnapshot("after"));
      postToThread(bridge, () => previewEarningsMessage("reported"));
    } },
    briefing("briefing-earnings-row", "A briefing's results rows and the results ahead", ".briefing-earnings", () => ({ earnings: previewBriefingEarnings() })),
    { ...base("chat-earnings-card", "An answer shows the results card instead of a bare link", () => select(".earnings-chat-card .earnings-card"), async root => {
      await until(() => root.querySelector("article.message.assistant.complete"), "Answer did not load");
      await reveal(".earnings-chat-card .earnings-card", "The results card did not appear in the answer");
    }), setup: async bridge => {
      await prepareFixture(bridge, "answer");
      await installAsset(bridge, noThesisFixture());
      const snapshot = morrowSnapshot("after");
      bridge.earnings.install(PREVIEW_LOUPIST, snapshot);
      const evidence: ChatEvidenceRef = {
        id: `ev_${"e".repeat(32)}`, title: "Results · Q3 2026 · Morrow Components", kind: "saved_context", tool_name: "earnings_snapshot",
        read_ref: `earnings:${snapshot.assetId}:${snapshot.eventId}`, retrieved_at: new Date(FIXTURE_TIME).toISOString(),
        excerpt: "", excerpt_truncated: false, content_sha256: "a".repeat(64), warnings: [],
      };
      const original = bridge.loupistSelect.bind(bridge);
      bridge.loupistSelect = async loupistId => {
        const workspace = await original(loupistId);
        const answer = workspace.threads[0]?.messages.find(item => item.id === "preview-answer");
        if (answer && !answer.evidence_refs?.some(item => item.id === evidence.id)) {
          answer.content = `${answer.content}\n\n[Results · Q3 2026](#evidence-${evidence.id})`;
          answer.evidence_refs = [...answer.evidence_refs ?? [], evidence];
        }
        return workspace;
      };
    } },
    { ...base("schedule-earnings-task", "Schedule: the automatic results task with its captures", () => select(".schedule-task-detail .schedule-history") && select(".schedule-task-detail .briefings-back"), async root => {
      await openSchedule(root, "preview-earnings");
      await reveal(".schedule-task-detail .schedule-history", "The results task did not open");
    }), setup: async bridge => {
      await prepareFixture(bridge, "answer");
      // The preview's Schedule lists its own tasks; the results task Rust created joins them.
      const { task, runs } = previewEarningsTask();
      const list = bridge.scheduledTasksList.bind(bridge);
      bridge.scheduledTasksList = async request => [...await list(request), { ...task, latest_run: runs[0] ?? null }];
      const history = bridge.scheduledTaskRuns.bind(bridge);
      bridge.scheduledTaskRuns = async request => request.task_id === task.id ? runs : history(request);
    } },

    // W16.7: local watchers.
    { ...base("asset-page-watch", "Asset page: what to watch for, one switch per kind", () => select(".watch-for .watch-switch"), async root => {
      await openAsset(root, HANUL_SYMBOL, HANUL_MARKET);
      await reveal(".watch-for", "Watch for did not appear");
    }), setup: async bridge => {
      await prepareFixture(bridge);
      await installAsset(bridge, hanulFixture(), 3);
      installWatch(bridge, { ...previewWatchSettings(), watched: previewWatchSettings().watched!.map(item => ({ ...item, kinds: item.kinds.map(kind => ({ ...kind, needsConnection: undefined })) })) });
    } },
    { ...base("asset-page-watch-needs-key", "Asset page: a watch kind needs Korea Investment", () => select(".watch-for .watch-for-reason"), async root => {
      await openAsset(root, HANUL_SYMBOL, HANUL_MARKET);
      await reveal(".watch-for .watch-for-reason", "The connection reason did not appear");
    }), setup: async bridge => {
      await prepareFixture(bridge);
      await installAsset(bridge, hanulFixture(), 3);
      installWatch(bridge);
    } },
    watching("schedule-watching", "Schedule: Watching, with the matrix and recent alerts", ".watching-matrix", () => previewWatchSettings(), () => previewWatchEvents()),
    watching("schedule-watching-paused", "Schedule: Watching is paused", ".watching-matrix[data-paused]", () => previewWatchSettings({ paused: true }), () => previewWatchEvents()),
    watching("schedule-watching-empty", "Schedule: Watching before the first alert", "[data-watching-empty]", () => previewWatchSettings(), () => []),
    { ...base("thread-watch-alerts", "The Thread lists what the watchers found", () => select(".thread-watch .thread-watch-events"), async () => {
      await reveal(".thread-watch", "The watcher message did not appear");
    }), setup: async bridge => {
      await prepareMorningFixture(bridge, "nothing-changed");
      installWatch(bridge);
      postToThread(bridge, previewWatchMessage);
    } },
    { ...base("thread-morning-watch", "Morning message: alerts from the first check of the day", () => select('.morning-message[aria-busy="false"] .morning-followed .morning-alert'), async () => {
      await reveal(".morning-followed", "The morning alerts did not appear");
    }), setup: async bridge => {
      await prepareMorningFixture(bridge, "morning");
      installWatch(bridge, previewWatchSettings(), previewWatchEvents({ deliveredIn: "morning", detectedAtMs: FIXTURE_TIME - 5 * 3_600_000 }).slice(0, 3));
    } },
    { ...base("shell-needs-attention-watch", "The sidebar counts new alerts", () => select('.needs-attention-row[data-kind="watchAlerts"]'), async () => {
      if (hasLayout() && document.documentElement.clientWidth < 768 && !select(".needs-attention")) await click('[data-slot="sidebar-trigger"]');
    }), setup: async bridge => {
      await prepareFixture(bridge, "answer");
      bridge.attentionItems = [{ kind: "watchAlerts", loupistId: PREVIEW_LOUPIST, loupistName: "Quality Compounder", count: 3, title: "", atMs: Date.now() - 20 * 60_000 }];
    } },
    briefing("briefing-watch-rows", "A briefing's What you follow with the alerts since the last one", ".briefing-watch-rows", () => ({ watchEvents: previewWatchEvents() })),

    // W16.12: class watchers.
    { ...base("asset-page-crypto-watch", "Asset page: a coin's watchers, value locked waiting for DefiLlama", () => select('.watch-for .watch-switch[data-kind="tvl"]'), async root => {
      await openAsset(root, "CRVD", "CRYPTO");
      await reveal(".watch-for", "Watch for did not appear");
    }), setup: async bridge => {
      await prepareFixture(bridge);
      await installAsset(bridge, cryptoFixture(), 3);
      installWatch(bridge, previewCryptoWatchSettings(), previewCryptoWatchEvents());
    } },
    watching("schedule-watching-commodities", "Schedule: Watching a Commodities Loupist's futures", '.watching-matrix-scroll[data-group="commodity"]', () => previewCommodityWatchSettings(), () => previewCommodityWatchEvents()),
  ];
}

const HANUL_SYMBOL = "HANUL";
const HANUL_MARKET = "KRX";
