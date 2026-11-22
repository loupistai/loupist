/** Preview scenarios, in the capture order `src/preview/scenarios.ts` assembles. */
import { prepareFixture, previewKo, type PreviewBridge } from "../fixtures";
import { semiconductorTrackingView } from "../../bridge/mock/worldSchema";
import { base, click, hasLayout, openLoupistMenu, openLoupistSettings, openNavigation, select, settings, toggleSidebar, until, type PreviewScenario } from "./kit";

const buildingMapRow = '[data-sidebar-place="map"] button:has(.thread-running)';
const runningScheduleRow = '[data-sidebar-place="schedule"] button:has(.thread-running)';

function runningScheduleLandmark(container = "") {
  const row = select(`${container}${runningScheduleRow}`);
  return row && !row.querySelector(".shell-unread") ? row : null;
}

async function runningSchedule(bridge: PreviewBridge) {
  await prepareFixture(bridge);
  const [lastRun] = await bridge.scheduledTaskRuns({ loupist_id: "loupist-quality", task_id: "preview-task-0" });
  if (!lastRun) throw new Error("Schedule preview run is missing");
  bridge.scheduledTasksActivity = async ({ loupist_id }) => loupist_id === lastRun.loupist_id ? [{
    ...lastRun, id: "preview-running-task", run_id: "preview-running-task",
    status: "running", finished_at_ms: null, message_id: null,
  }] : [];
}

async function buildingMap(bridge: PreviewBridge) {
  await prepareFixture(bridge);
  const graphSettings = bridge.knowledgeGraphSettingsGet.bind(bridge);
  bridge.knowledgeGraphSettingsGet = async request => {
    const view = await graphSettings(request);
    return { ...view, readiness: { ...view.readiness, status: "ready", note: null } };
  };
  const read = bridge.knowledgeCatalog.bind(bridge);
  bridge.knowledgeCatalog = async request => {
    const catalog = await read(request);
    let index = 0;
    return { ...catalog, sources: catalog.sources.map(source => ({
      ...source,
      objects: source.objects.map(object => {
        const status = ["running", "queued"][index++];
        return status ? { ...object, processing: { jobId: object.id, status, error: null } } : object;
      }),
    })) };
  };
}

/** Loupists, chats, the page header and the sidebar. */
export const shellScenarios: PreviewScenario[] = [
  { ...base("chat-topic-empty", "A new topic Chat offers only questions", () => select(".morning-ask-only .morning-question"), async root => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    // ⌘N starts a topic Chat at every width; the sidebar's row control is hover-only and hidden when narrow.
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "n", metaKey: true, ctrlKey: !navigator.userAgent.includes("Mac"), bubbles: true }));
  }) },
  base("loupists", "Loupists in Settings", () => select(".settings-loupists"), (root) => settings(root, "loupists")),
  base("loupist-settings", "Editable Loupist profile", () => select(".loupist-settings-page #loupist-profile-focus"), async () => { await openLoupistSettings(); await until(() => select("#loupist-profile-name"), "Profile editor did not open"); }),
  { ...base("loupist-settings-tracking", "What World tracks", () => select(".loupist-tracking-changes"), async () => {
    await openLoupistSettings();
    await click('[data-loupist-section="world"] > summary');
    await click("[data-loupist-tracking-advanced] > summary");
    await until(() => select(".loupist-tracking-list"), "Tracking list did not load");
    select(".loupist-tracking")?.scrollIntoView({ block: "start" });
  }), setup: async bridge => { await prepareFixture(bridge); bridge.installWorldSchema("loupist-quality", semiconductorTrackingView()); } },
  { ...base("chat-all-chats", "Five recent sessions stay alongside the current chat", () => {
    const sessions = select(".loupist-navigation");
    return sessions && sessions.querySelectorAll('.loupist-chat-row:not([data-home]):not([data-sidebar-place])').length === 5 ? sessions : null;
  }, async () => {
    await until(() => select("textarea[data-chat-composer]"), "Chat did not initialize");
    // The icon rail lists no sessions (only Bot, which is `preview-chat`): expand it to show them.
    await openNavigation(".loupist-sessions");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true, bubbles: true }));
    const search = await until(() => select(".command-palette-input") as HTMLInputElement | null, "Search did not open");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(search, "Capital expenditure plans");
    search.dispatchEvent(new Event("input", { bubbles: true }));
    await until(() => select('[data-palette-row="chat:loupist-quality:preview-chat-6"]'), "An older session did not appear in search");
    search.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await until(() => !select(".command-palette"), "Search did not close");
  }), setup: async bridge => { await prepareFixture(bridge); bridge.manyChats = true; } },
  base("shell-titlebar-grown", "A taller page header keeps the sidebar and composer inside the window", () => select(".app-header"), async () => {
    await until(() => select('textarea[data-chat-composer]'), "Chat did not initialize");
    // Exercise the height contract independently of the header's own height.
    (select(".app-header") as HTMLElement).style.height = "72px";
  }),
  base("shell-bot-rail", "The icon rail with centered icons, or the navigation sheet below the minimum window", () => select(hasLayout() && window.matchMedia("(max-width: 639px)").matches ? '[data-mobile="true"] .navigation-search' : '[data-collapsible="icon"] .navigation-search'), async () => {
    await until(() => select("textarea[data-chat-composer]"), "Bot did not initialize");
    await until(() => select(".navigation-search"), "Sidebar did not initialize");
    if (!select('[data-collapsible="icon"] .navigation-search')) {
      if (select('[data-slot="sidebar-trigger"]')) await click('[data-slot="sidebar-trigger"]');
      else toggleSidebar();
    }
  }),
  // The user's sidebar width, narrowest and widest (about a third of a wide window, at most 420 px).
  ...([["shell-sidebar-narrow", 208], ["shell-sidebar-wide", 420]] as const).map(([id, width]): PreviewScenario => ({
    ...base(id, `The sidebar resized to ${width} px, with the page reflowed beside it`, () => select(`[data-sidebar-resize][aria-valuenow="${Math.min(width, Math.max(208, Math.min(420, Math.floor(window.innerWidth / 3))))}"]`) ?? (window.innerWidth < 940 ? select('[data-collapsible="icon"]') : null), async () => {
      await until(() => select("textarea[data-chat-composer]"), "Bot did not initialize");
    }),
    setup: async bridge => { await prepareFixture(bridge); localStorage.setItem("loupist.sidebarWidth", String(width)); },
  })),
  base("shell-sidebar-home","The Loupist at the top of the sidebar, and Bot", () => select(".loupist-header-identity"), async () => {
    await until(() => select("textarea[data-chat-composer]"), "Bot did not initialize");
  }),
  { ...base("shell-sidebar-many", "Six Loupists with activity and unread briefings in the switcher", () => select(".loupist-switcher-menu"), async () => {
    await until(() => select('textarea[data-chat-composer]'), "Chat did not initialize");
    await openLoupistMenu();
    await until(() => document.querySelectorAll(".loupist-switcher-menu [data-loupist-option]").length === 6, "Loupist list did not load");
    // The face's ring is presentational; its live status is a separate sibling.
    await until(() => { const dot = document.querySelectorAll(".loupist-switcher-menu [data-loupist-option]")[1]?.querySelector(".loupist-face[data-busy]"); return dot && (!hasLayout() || dot.getBoundingClientRect().width > 0); }, "Background activity did not reach the switcher");
  }), setup: async bridge => {
    await prepareFixture(bridge);
    const names = ["Semiconductors — memory chips, foundries, advanced packaging", "US Big Tech", "Dividend Loupist", "Energy transition and industrial infrastructure", previewKo() ? "한국 소비재와 유통" : "Korean consumer goods and retail", "Global supply chains"];
    const [first] = await bridge.loupistList();
    await bridge.loupistProfileUpdate({ loupistId: first!.id, name: names[0], focus: "Memory chips & foundries" });
    for (const name of names.slice(1)) await bridge.loupistCreate({ name, focus: "Long-term research and primary evidence" });
    bridge.manyLoupists = true;
    const profiles = await bridge.loupistList();
    bridge.runActivityList = async () => [{ run_id: "preview-background-run", loupist_id: profiles[1]!.id, kind: "chat", target: "preview-chat", started_at_ms: Date.now() - 30_000 }];
  } },
  // Spinners on chats with a reply running: a session whose run the host reported on reload, the
  // open session Bot handed off (a long title), and Bot itself while another view is open.
  { ...base("shell-sidebar-running", "Research sessions and Bot with a reply running show a spinner", () => {
    const sidebar = select(".loupist-navigation");
    return sidebar && sidebar.querySelectorAll('[data-testid="loupist-chat-row"] .thread-running').length === 3 && select('.loupist-sessions [data-thread-id="preview-chat-2"][aria-current="page"]') ? sidebar : null;
  }, async () => {
    await until(() => select("textarea[data-chat-composer]"), "Bot did not initialize");
    await openNavigation(".loupist-sessions");
    await click('.loupist-sessions [data-thread-id="preview-chat-2"]');
  }), setup: runningSessions },
  // The icon rail keeps Bot's spinner in the button's corner while another page is open.
  { ...base("shell-sidebar-running-rail", "The icon rail with Bot's spinner while a hand-off runs", () => select('[data-collapsible="icon"] .loupist-chat-row[data-home] .thread-running'), async () => {
    await until(() => select("textarea[data-chat-composer]"), "Bot did not initialize");
    await click('[data-sidebar-place="following"] button');
    if (!select('[data-collapsible="icon"] .navigation-search')) toggleSidebar();
  }), setup: runningSessions },
  { ...base("shell-map-building", "World row shows building work while Bot is open", () => select(buildingMapRow), async () => {
    await openNavigation('[data-sidebar-place="map"] button');
  }), setup: buildingMap },
  { ...base("shell-map-building-rail", "World spinner sits in the icon rail's corner", () => select(`[data-collapsible="icon"] ${buildingMapRow}`), async () => {
    await openNavigation('[data-sidebar-place="map"] button');
    if (!select('[data-collapsible="icon"] [data-sidebar-place="map"] button')) toggleSidebar();
  }), setup: buildingMap },
  { ...base("shell-schedule-running", "The Schedule spinner replaces the unread count while a task runs", () => runningScheduleLandmark(), async () => {
    await openNavigation('[data-sidebar-place="schedule"] button');
  }), setup: runningSchedule },
  { ...base("shell-schedule-running-rail", "The Schedule spinner sits in the icon rail's corner", () => runningScheduleLandmark('[data-collapsible="icon"] '), async () => {
    await openNavigation('[data-sidebar-place="schedule"] button');
    if (!select('[data-collapsible="icon"] [data-sidebar-place="schedule"] button')) toggleSidebar();
  }), setup: runningSchedule },
  // The one-time line after holdings left Loupist, above Bot.
  ...([["exported", "Holdings were saved to a file: Show file, Save a copy… and Dismiss"], ["failed", "The holdings export has not succeeded yet: Try again and Dismiss"]] as const).map(([state, title]): PreviewScenario => ({
    ...base(`shell-legacy-holdings-${state}`, title, () => select(`[data-legacy-holdings="${state}"]`), async () => {
      await until(() => select("textarea[data-chat-composer]"), "Bot did not initialize");
    }),
    setup: async bridge => {
      await prepareFixture(bridge);
      bridge.legacyHoldings.setStatus(state === "exported"
        ? { state, dismissed: false, exportedAtMs: Date.UTC(2026, 10, 17, 9), counts: { wallets: 2, snapshots: 3, positions: 14 } }
        : { state, dismissed: false, counts: { wallets: 2, snapshots: 3, positions: 14 } });
    },
  })),
  base("connections", "Connections", () => select("#marketplace-title"), (root) => settings(root, "connections")),
  // ⌘K with a query that finds nothing: the status says so and what to try instead.
  { ...base("palette-no-results", "Search says when nothing matches and what to try", () => select('.command-palette-empty[role="status"]:not(:empty)'), async (root) => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true, ctrlKey: !navigator.userAgent.includes("Mac"), bubbles: true }));
    const input = await until(() => select(".command-palette-input") as HTMLInputElement | null, "Search did not open");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, previewKo() ? "없는회사" : "zzqx holdings");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  }), setup: (bridge) => prepareFixture(bridge, "answer") },
];

/**
 * Sessions with a reply running: the first after a reload while the host still runs it, the second
 * one Bot handed off (the host reports its run on the session; a long title). That run makes Bot's
 * own row spin too, though Bot posts nothing for it until it ends.
 */
async function runningSessions(bridge: PreviewBridge) {
  await prepareFixture(bridge);
  bridge.manyChats = true;
  const select = bridge.loupistSelect.bind(bridge);
  bridge.loupistSelect = async (loupistId) => {
    const workspace = await select(loupistId);
    const title = previewKo()
      ? "2027년에 HBM4를 대량으로 공급할 수 있는 다른 회사와 그 회사의 고객별 공급 조건"
      : "Who else can supply HBM4 at volume in 2027, and on what terms with which customers";
    const handoff_run = { handoff_id: "handoff-preview-running", run_id: "handoff-preview" };
    return { ...workspace, threads: workspace.threads.map(thread => thread.id === "preview-chat-2" ? { ...thread, title, handoff_run } : thread) };
  };
  const [profile] = await bridge.loupistList();
  bridge.runActivityList = async () => [{ run_id: "preview-reloaded-run", loupist_id: profile!.id, kind: "chat", target: "preview-chat-1", started_at_ms: Date.now() - 30_000 }];
}
