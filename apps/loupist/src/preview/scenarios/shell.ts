/** Preview scenarios, in the capture order `src/preview/scenarios.ts` assembles. */
import { prepareFixture, previewKo } from "../fixtures";
import { semiconductorTrackingView } from "../../bridge/mock/worldSchema";
import { base, click, hasLayout, openLoupistSettings, openNavigation, select, settings, until, type PreviewScenario } from "./kit";

/** Loupists, chats, the title bar and the sidebar. */
export const shellScenarios: PreviewScenario[] = [
  { ...base("chat-topic-empty", "A new topic Chat offers only questions", () => select(".morning-ask-only .morning-question"), async root => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    // ⌘N starts a topic Chat at every width; the sidebar's row control is hover-only and hidden when narrow.
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "n", metaKey: true, ctrlKey: !navigator.userAgent.includes("Mac"), bubbles: true }));
  }) },
  base("loupists", "Loupists in Settings", () => select(".settings-loupists"), (root) => settings(root, "loupists")),
  base("loupist-settings", "Editable Loupist profile", () => select(".loupist-settings-page #loupist-profile-focus"), async () => { await openLoupistSettings(); await until(() => select("#loupist-profile-name"), "Profile editor did not open"); }),
  { ...base("loupist-settings-tracking", "What the map tracks", () => select(".loupist-tracking-changes"), async () => {
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
    await openNavigation('[data-thread-id="preview-chat"]');
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true, bubbles: true }));
    const search = await until(() => select(".command-palette-input") as HTMLInputElement | null, "Search did not open");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(search, "Capital expenditure plans");
    search.dispatchEvent(new Event("input", { bubbles: true }));
    await until(() => select('[data-palette-row="chat:loupist-quality:preview-chat-6"]'), "An older session did not appear in search");
    search.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await until(() => !select(".command-palette"), "Search did not close");
  }), setup: async bridge => { await prepareFixture(bridge); bridge.manyChats = true; } },
  base("shell-titlebar-grown", "A taller titlebar keeps the sidebar and composer inside the remaining body", () => select(".app-titlebar"), async () => {
    await until(() => select('textarea[data-chat-composer]'), "Chat did not initialize");
    // Exercise the height contract independently of the current platform's titlebar size.
    (select(".app-titlebar") as HTMLElement).style.minHeight = "72px";
  }),
  base("shell-bot-rail", "Collapsed navigation with centered icons, or the navigation sheet in a narrow window", () => select(hasLayout() && window.matchMedia("(max-width: 899px)").matches ? '[data-mobile="true"] .navigation-search' : '[data-collapsible="icon"] .navigation-search'), async () => {
    await until(() => select("textarea[data-chat-composer]"), "Bot did not initialize");
    if (!select('[data-collapsible="icon"] .navigation-search')) await click('[data-slot="sidebar-trigger"]');
  }),
  base("shell-sidebar-home", "Selected Loupist and Bot", () => select(".loupist-header-identity"), async () => {
    await until(() => select("textarea[data-chat-composer]"), "Bot did not initialize");
  }),
  { ...base("shell-sidebar-many", "Six Loupists with activity and unread briefings", () => select(".loupist-header-profiles"), async () => {
    await until(() => select('textarea[data-chat-composer]'), "Chat did not initialize");
    await until(() => document.querySelectorAll(".loupist-header-profiles .loupist-header-identity").length === 6, "Loupist list did not load");
    // The visual dot is presentational; its live status is a separate sibling.
    await until(() => { const dot = document.querySelector('.loupist-header-identity:nth-of-type(2) .loupist-run-indicator'); return dot && (!hasLayout() || dot.getBoundingClientRect().width > 0); }, "Background activity did not reach the header");
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
  base("connections", "Connections", () => select("#marketplace-title"), (root) => settings(root, "connections")),
];
