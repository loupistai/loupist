/** Preview scenarios, in the capture order `src/preview/scenarios.ts` assembles. */
import { prepareFixture, previewDelivery, previewKo } from "../fixtures";
import { prepareMorningFixture } from "../fixtures/morning";
import { base, click, hasLayout, navigate, openNavigation, openScheduleRun, select, until, type PreviewScenario } from "./kit";

/** The Schedule tab and the home Thread. */
export const scheduleScenarios: PreviewScenario[] = [
  // W15.23 and W15.6c: the Schedule tab manages scheduled work; results arrive in the home Thread.
  ...([
    ["schedule-list", "Scheduled tasks with saved names, cadence and a task menu", ".schedule-list"],
    ["schedule-watching-setup", "Schedule's Watching row opens Following with its status and controls", "[data-testid=following-page] .watching-status"],
    ["schedule-empty", "Nothing scheduled yet", ".schedule-list .schedule-empty"],
    ["schedule-create", "Request scheduled work in Bot", ".composer-shell textarea"],
    ["schedule-active-filter", "Active filter hides a paused task", "[data-schedule-filter='active']"],
    ["schedule-dialog", "Saved briefing cadence and run history", ".schedule-task-detail"],
    ["briefing-structured", "A briefing's sections above its answer", ".briefing-sections"],
    ["briefing-failed", "A briefing that could not finish", ".briefings-failure"],
    ["briefing-suggested", "A briefing keeps the changes that prompted it", ".briefing-sections"],
    ["schedule-task-active", "An active task's own controls: pause, run now, delete and change with Bot", ".schedule-task-controls [data-schedule-status='paused']"],
    ["schedule-task-paused", "A paused task offers Resume", ".schedule-task-controls [data-schedule-status='active']"],
    ["schedule-task-failed", "A task whose last run failed says why, beside its controls", ".schedule-task-error"],
  ] as const).map(([id, title, landmark]): PreviewScenario => ({
    ...base(id, title, () => {
      if (id === "schedule-list") {
        const list = select(".schedule-list");
        return list && !list.closest(".briefings-layout")?.hasAttribute("data-reader-open") && list.querySelector('[data-schedule-task="watching"][data-status="setup"]') ? list : null;
      }
      if (id === "schedule-active-filter") {
        const rows = [...document.querySelectorAll(".schedule-task:not(.watching-item)")];
        const paused = previewKo() ? "주간 공급 능력 비교" : "Weekly supplier capacity comparison";
        return rows.length > 0 && rows.every(row => !row.textContent?.includes(paused)) ? select(landmark) : null;
      }
      if (id === "schedule-create") return (select(landmark) as HTMLTextAreaElement | null)?.value ? select(landmark) : null;
      return select(landmark);
    }, async root => {
      if (id === "briefing-structured" || id === "briefing-failed" || id === "briefing-suggested") { await openScheduleRun(root, id === "briefing-failed" ? "preview-briefing-2" : "preview-briefing-1"); return; }
      if (id === "schedule-create") {
        await openNavigation(".loupist-chat-row[data-home]");
        await click(".loupist-chat-row[data-home] .loupist-chat-button");
        const composer = await until(() => select("textarea[data-chat-composer]") as HTMLTextAreaElement | null, "Bot composer did not initialize");
        Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(composer, previewKo() ? "매주 월요일 아침 8시에 재고일수를 비교해 줘." : "Every Monday at 8, compare inventory days for what I follow.");
        composer.dispatchEvent(new Event("input", { bubbles: true }));
        return;
      }
      await navigate(root, "schedule");
      if (id === "schedule-watching-setup") await click('[data-schedule-task="watching"] .schedule-task-summary');
      if (id === "schedule-dialog") await click('[data-schedule-task="preview-briefing"] .schedule-task-summary');
      if (id === "schedule-task-active" || id === "schedule-task-failed") await click('[data-schedule-task="preview-task-0"] .schedule-task-summary');
      if (id === "schedule-task-paused") {
        await click('[data-schedule-filter] [role="radio"][value="all"]');
        await click('[data-schedule-task="preview-task-1"] .schedule-task-summary');
      }
    }),
    setup: async bridge => {
      await prepareFixture(bridge, "answer");
      bridge.includeBriefingBody = true;
      if (id === "schedule-empty") { bridge.scheduleTasks = "none"; bridge.inboxList = async () => ({ items: [], nextCursor: null }); bridge.inboxUnreadCounts = async () => []; return; }
      await bridge.briefingSettingsSet({ loupistId: "loupist-quality", enabled: true });
      // The briefing follows the same assets the user follows, named in the cell's language.
      const ko = previewKo();
      const followed = [["005930", "KRX", ko ? "삼성전자" : "Samsung Electronics"], ["000660", "KRX", ko ? "SK하이닉스" : "SK hynix"], ["AAPL", "NASDAQ", "Apple"]] as const;
      bridge.watchlistList = async () => followed.map(([symbol, market, displayName]) => ({ symbol, market, displayName, addedAtMs: Date.now() - 30 * 86_400_000 }));
      const map = await bridge.knowledgeMap({ loupistId: "loupist-quality", maxNodes: 1000 });
      const entity = (node: { id: string }, index: number) => ({ eventId: `briefing-growth-${index}`, loupistId: "loupist-quality", originChatId: "preview-chat", originMessageId: "preview-answer", atMs: Date.now(), kind: "entity_added" as const, subjectId: node.id, recordId: "preview-world", actor: "loupist" as const, summary: "", beforeSummary: null, afterSummary: null });
      const disputed = id === "briefing-suggested" ? [{ ...entity({ id: "map-claim-28" }, 9), kind: "claim_disputed" as const, subjectId: "map-claim-28", atMs: Date.now() + 3_600_000 }] : [];
      bridge.knowledgeGrowthEvents = async () => ({ events: [...map.nodes.filter(node => node.kind === "entity").slice(0, 3).map(entity), ...disputed], nextCursor: null });
      if (id === "schedule-task-failed") {
        const list = bridge.scheduledTasksList.bind(bridge);
        bridge.scheduledTasksList = async request => (await list(request)).map(task => task.id === "preview-task-0" && task.latest_run
          ? { ...task, latest_run: { ...task.latest_run, status: "failed" as const, error: "provider_auth: 401 unauthorized" } } : task);
      }
    },
  })),
  { ...base("thread-home", "The home Thread: a delivered result and the morning news", () => select(".thread-delivery") && select('.morning-message[aria-busy="false"]'), async () => {
    const news = await until(() => select('.morning-message[aria-busy="false"]'), "Morning news did not appear");
    if (hasLayout()) news.scrollIntoView({ block: "end" });
  }), setup: async bridge => {
    await prepareMorningFixture(bridge, "morning");
    bridge.conversation = "answer";
    const select = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await select(loupistId);
      workspace.threads.find(thread => thread.home)?.messages.push(previewDelivery());
      return workspace;
    };
  } },
];
