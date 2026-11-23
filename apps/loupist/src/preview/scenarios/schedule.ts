/** Preview scenarios, in the capture order `src/preview/scenarios.ts` assembles. */
import { prepareFixture, previewDelivery, previewKo } from "../fixtures";
import { prepareMorningFixture, writtenEarlier } from "../fixtures/morning";
import { scheduledRunFixture } from "../../test/scheduledFixtures";
import { base, click, contextTitleLandmark, hasLayout, navigate, openNavigation, openScheduleRun, select, until, type PreviewScenario } from "./kit";

/** The Schedule tab and the home Thread. */
export const scheduleScenarios: PreviewScenario[] = [
  ...(["continued-schedule-origin", "bot-schedule-origin"] as const).map((id): PreviewScenario => {
    const session = id === "continued-schedule-origin";
    return { ...base(id, "A long scheduled task name gives space to its date", () => contextTitleLandmark(session ? ".scheduled-session-origin" : ".thread-delivery .bot-card-tag"), async () => {
      if (!session) return;
      await openNavigation('[data-thread-id="preview-continued-run"]');
      await click('[data-thread-id="preview-continued-run"]');
    }), setup: async bridge => {
      await prepareFixture(bridge, "answer");
      const loupist_id = "loupist-quality";
      const title = previewKo() ? "반도체 공급업체의 설비 투자와 연간 수요 전망 분석"
        : "Review semiconductor suppliers' capital spending and annual demand outlook";
      const task = await bridge.proposeScheduledTask({ loupist_id, title, prompt: title, schedule: { kind: "once", at_ms: 1000 },
        target: { kind: "new_chat" }, model_profile_id: "", thinking_level: "", run_options: {}, use_memory: true, update_memory: true, status: "active" });
      const run = scheduledRunFixture({ loupist_id, task_id: task.id, task_title: title, thread_id: "preview-continued-run" });
      const get = bridge.scheduledTaskGet.bind(bridge);
      const runs = bridge.scheduledTaskRuns.bind(bridge);
      bridge.scheduledTaskGet = request => request.task_id === task.id ? Promise.resolve(task) : get(request);
      bridge.scheduledTaskRuns = request => request.task_id === task.id ? Promise.resolve([run]) : runs(request);
      const select = bridge.loupistSelect.bind(bridge);
      bridge.loupistSelect = async id => {
        const workspace = await select(id);
        const home = workspace.threads.find(thread => thread.home);
        if (!home) return workspace;
        if (session) {
          const [question, answer] = home.messages;
          if (question && answer) workspace.threads.push({ ...home, id: run.thread_id!, title, home: false,
            messages: [{ ...question, id: "scheduled-request", scheduled: { run_id: run.run_id, task_id: task.id, task_title: title, task_kind: "custom" } }, answer, question] });
        } else {
          const post = previewDelivery();
          home.messages.push({ ...post, delivery: { ...post.delivery!, task_title: title, task_kind: "custom" } });
        }
        return workspace;
      };
    } };
  }),
  // W15.23 and W15.6c: the Schedule tab manages scheduled work; results arrive in the home Thread.
  ...([
    ["schedule-list", "Scheduled tasks with saved names, cadence and a task menu", ".schedule-list"],
    ["schedule-empty", "Nothing scheduled yet", ".schedule-list .schedule-empty"],
    ["schedule-create", "Request scheduled work in Bot", ".composer-shell textarea"],
    ["schedule-active-filter", "Active filter hides a paused task", "[data-schedule-filter='active']"],
    ["schedule-briefing-off", "Custom tasks and the built-in briefing, still off, with Turn on in its row", "[data-schedule-briefing='off']"],
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
        return list && !list.closest(".briefings-layout")?.hasAttribute("data-reader-open") && list.querySelector(".schedule-task") ? list : null;
      }
      if (id === "schedule-active-filter") {
        const rows = [...document.querySelectorAll(".schedule-task")];
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
      // The briefing names the assets the user follows by the names they were followed by, in the cell's language.
      const ko = previewKo();
      for (const [ticker, exchange, name] of [["005930", "KS", ko ? "삼성전자" : "Samsung Electronics"], ["000660", "KS", ko ? "SK하이닉스" : "SK hynix"], ["AAPL", "US", "Apple"]] as const) {
        bridge.assets.installAsset({ name, ticker, exchange });
      }
      const map = await bridge.knowledgeMap({ loupistId: "loupist-quality", maxNodes: 1000 });
      const entity = (node: { id: string }, index: number) => ({ eventId: `briefing-growth-${index}`, loupistId: "loupist-quality", originChatId: "preview-chat", originMessageId: "preview-answer", atMs: Date.now(), kind: "entity_added" as const, subjectId: node.id, recordId: "preview-world", actor: "loupist" as const, summary: "", beforeSummary: null, afterSummary: null });
      const disputed = id === "briefing-suggested" ? [{ ...entity({ id: "map-claim-28" }, 9), kind: "claim_disputed" as const, subjectId: "map-claim-28", atMs: Date.now() + 3_600_000 }] : [];
      bridge.knowledgeGrowthEvents = async () => ({ events: [...map.nodes.filter(node => node.kind === "entity").slice(0, 3).map(entity), ...disputed], nextCursor: null });
      if (id === "schedule-briefing-off") {
        const list = bridge.scheduledTasksList.bind(bridge);
        bridge.scheduledTasksList = async request => (await list(request)).filter(task => task.kind !== "briefing");
      }
      if (id === "schedule-task-failed") {
        const list = bridge.scheduledTasksList.bind(bridge);
        bridge.scheduledTasksList = async request => (await list(request)).map(task => task.id === "preview-task-0" && task.latest_run
          ? { ...task, latest_run: { ...task.latest_run, status: "failed" as const, error: "provider_auth: 401 unauthorized" } } : task);
      }
    },
  })),
  { ...base("thread-home", "The home Thread: a delivered result and the morning news", () => select(".thread-delivery") && select('.morning-message'), async () => {
    const news = await until(() => select('.morning-message'), "Morning news did not appear");
    if (hasLayout()) news.scrollIntoView({ block: "end" });
  }), setup: async bridge => {
    await prepareMorningFixture(bridge, "morning");
    bridge.conversation = "answer";
    // The person last wrote before the last visit; the delivered result arrived since.
    writtenEarlier(bridge, 3);
    const select = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await select(loupistId);
      workspace.threads.find(thread => thread.home)?.messages.push(previewDelivery());
      return workspace;
    };
  } },
];
