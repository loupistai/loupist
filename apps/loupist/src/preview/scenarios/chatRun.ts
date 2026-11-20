/** Preview scenarios, in the capture order `src/preview/scenarios.ts` assembles. */
import { advancePreviewClock, prepareFixture, PreviewBridge, previewKo } from "../fixtures";
import i18n from "../../i18n";
import type { ChatRunPhase } from "../../contracts/chat";
import { approvalFor, approvalRun, approvalSteps, asSession, base, centerOn, click, hasLayout, longThread, openNavigation, prepareAttention, pressMenu, runningHeader, select, submitPrompt, until, visible, type PreviewScenario } from "./kit";

/** A reply that stays in flight on a running step; `phase: "tool"` sends no answer text. */
function liveRun(bridge: PreviewBridge, phase: ChatRunPhase) {
  bridge.chatSend = async (request, observer) => {
    const run_id = request.run_id;
    observer({ type: "started", run_id });
    observer({ type: "progress", run_id, sequence: 1, progress: { startedAtMs: Date.now() - 12_000, items: [
      { id: "preview-read", kind: "tool", toolName: "world_read", category: "memory", operation: "read", action: "Read saved notes", target: "Hanul Precision operating report", status: "succeeded", href: null, startedAtMs: 0, finishedAtMs: null },
      { id: "preview-page", kind: "tool", toolName: "web_fetch", category: "web", operation: "read", action: "Read a web source", running: "Reading hanul.example", target: "Q3 operating review · hanul.example", status: "running", href: null, startedAtMs: 0, finishedAtMs: null },
    ], finishedAtMs: null } });
    observer({ type: "phase", run_id, phase });
    if (phase === "writing") observer({ type: "delta", run_id, text: "Hanul Precision’s revenue grew 11%, while operating margin declined. I’m checking the new line’s utilization…" });
    return { run_id };
  };
}

/** A reply that stays in flight while the Loupist publishes a chart, so the answer holds its reserved card. */
function chartRun(bridge: PreviewBridge) {
  bridge.chatSend = async (request, observer) => {
    const run_id = request.run_id;
    observer({ type: "started", run_id });
    observer({ type: "phase", run_id, phase: "writing" });
    observer({ type: "delta", run_id, text: "Utilization rose through the quarter while operating margin fell. The chart below puts the two side by side." });
    observer({ type: "progress", run_id, sequence: 1, progress: { startedAtMs: Date.now() - 9_000, items: [
      { id: "preview-read", kind: "tool", toolName: "world_read", category: "memory", operation: "read", action: "Read saved notes", target: "Hanul Precision operating report", status: "succeeded", href: null, startedAtMs: 0, finishedAtMs: null },
      { id: "preview-chart", kind: "tool", toolName: "chart_publish", category: "artifact", operation: "publish", action: "Made a chart", running: "Making a chart", target: "Margin vs utilization", status: "running", href: null, startedAtMs: 0, finishedAtMs: null },
    ], finishedAtMs: null } });
    observer({ type: "phase", run_id, phase: "tool" });
    return { run_id };
  };
}

/** A research-sized reply mid-plan: one item done, one active with two steps (one building a chart), one pending. */
function planRun(bridge: PreviewBridge) {
  bridge.chatSend = async (request, observer) => {
    const run_id = request.run_id;
    const ko = document.documentElement.lang.startsWith("ko");
    const start = Date.now() - 14_000;
    const labels = ko
      ? ["3분기 영업 보고서 읽기", "가동률과 재고일수 비교하기", "원칙과 대조하기"]
      : ["Read the Q3 operating report", "Compare utilization with inventory days", "Check against your rules"];
    observer({ type: "started", run_id });
    observer({ type: "phase", run_id, phase: "writing" });
    observer({ type: "delta", run_id, text: ko
      ? "한울정밀의 매출은 11% 늘었지만 영업이익률은 낮아졌습니다. 지금 가동률과 재고일수를 비교하고 있습니다…"
      : "Hanul Precision’s revenue grew 11%, while operating margin declined. I’m comparing utilization with inventory days…" });
    observer({ type: "progress", run_id, sequence: 1, progress: {
      startedAtMs: start,
      plan: { items: [
        { id: "report", label: labels[0]!, status: "done", activatedAtMs: start + 500 },
        { id: "compare", label: labels[1]!, status: "active", activatedAtMs: start + 6_000 },
        { id: "principles", label: labels[2]!, status: "pending" },
      ] },
      items: [
        { id: "preview-read", kind: "tool", toolName: "world_read", category: "memory", operation: "read", action: "Read saved notes", target: "Hanul Precision operating report", status: "succeeded", startedAtMs: start + 1_000, finishedAtMs: start + 3_000, href: null },
        { id: "preview-search", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", target: "Hanul Precision inventory days", status: "succeeded", startedAtMs: start + 6_500, finishedAtMs: start + 8_000, href: null },
        { id: "preview-chart", kind: "tool", toolName: "chart_publish", category: "artifact", operation: "publish", action: "Made a chart", running: "Making a chart", target: "Margin vs utilization", status: "running", startedAtMs: start + 9_000, href: null, finishedAtMs: null },
      ],
      finishedAtMs: null,
    } });
    observer({ type: "phase", run_id, phase: "tool" });
    return { run_id };
  };
}

/**
 * A research reply between steps: six web reads finished and no step runs. With `note`, a note
 * followed the reads: the reply shows it on the page and its own line reads Thinking, while
 * the status pill names its last sentence. Otherwise the line names the batch.
 */
function betweenStepsRun(bridge: PreviewBridge, note: boolean) {
  bridge.chatSend = async (request, observer) => {
    const run_id = request.run_id;
    const ko = previewKo();
    const start = Date.now() - 74_000;
    const done = (id: string, toolName: string, category: "memory" | "web", operation: "read" | "search", action: string, target: string, at: number) =>
      ({ id, kind: "tool" as const, toolName, category, operation, action, target, status: "succeeded" as const, href: null, startedAtMs: start + at, finishedAtMs: start + at + 1_500 });
    const sites = ["hanul.example", "morrow.example", "dart.fss.or.kr", "krx.co.kr", "hanul.example", "morrow.example"];
    observer({ type: "started", run_id });
    observer({ type: "progress", run_id, sequence: 1, progress: { startedAtMs: start, items: [
      done("preview-notes", "world_read", "memory", "read", "Read saved notes", ko ? "한울정밀 영업 보고서" : "Hanul Precision operating report", 2_000),
      done("preview-search", "web_search", "web", "search", "Searched the web", ko ? "한울정밀 재고일수" : "Hanul Precision inventory days", 6_000),
      ...sites.map((site, index) => done(`preview-page-${index}`, "web_fetch", "web", "read", "Read a web source", site, 10_000 + index * 4_000)),
      ...(note ? [{ id: "preview-note", kind: "commentary" as const, text: ko
        ? "두 회사의 공시와 보고서에서 매출 수치가 일치합니다. 다음으로 최근 4개 분기의 가동률과 재고일수를 나란히 비교하겠습니다."
        : "The filings and the reviews agree on both companies’ revenue. Next I’ll compare utilization with inventory days over the last four quarters." }] : []),
    ], finishedAtMs: null } });
    observer({ type: "phase", run_id, phase: "thinking" });
    return { run_id };
  };
}

/** The live line of the streaming reply, once it names `kind`. */
const liveLineOf = (kind: "batch" | "thinking") => select(`.message.assistant.streaming .turn-live[data-kind="${kind}"]`);

/** Sends the question and lets the line's dwell pass on the frozen preview clock. */
const betweenSteps = (kind: "batch" | "thinking") => async (root: HTMLElement) => {
  await submitPrompt(root, previewKo() ? "한울정밀과 모로우부품의 가동률과 재고일수를 비교해 줘." : "Compare utilization and inventory days at Hanul Precision and Morrow Components.");
  await until(runningHeader, "Run header did not appear");
  if (hasLayout()) advancePreviewClock(1_000);
  await until(() => liveLineOf(kind), "The live line did not name the last action");
};

const stalledHeader = () => { const header = runningHeader(); return header?.textContent?.includes(i18n.t("chat:turn.stillWorking")) ? header : null; };

async function scrollThreadUp() {
  const thread = await until(() => select(".message-scroll"), "Thread did not appear");
  if (!hasLayout()) return;
  await until(() => thread.scrollHeight - thread.clientHeight > 120, "Thread did not overflow");
  // The reader's own wheel: it ends the send glide, which would otherwise carry a bare scrollTop back.
  thread.dispatchEvent(new WheelEvent("wheel", { deltaY: -120, bubbles: true }));
  thread.scrollTop = 0;
  thread.dispatchEvent(new Event("scroll"));
  // Let the pill's 180 ms entrance finish before the capture.
  await new Promise((resolve) => setTimeout(resolve, 300));
}

/** A reply in flight: status, plans, approvals, questions and the composer's menus. */
export const chatRunScenarios: PreviewScenario[] = [
  { ...base("shell-needs-attention", "The sidebar lists what waits for the user", () => select('.needs-attention .needs-attention-row[data-kind="approval"]'), async () => {
    await until(() => select("textarea[data-chat-composer]"), "Chat did not initialize");
    if (hasLayout() && document.documentElement.clientWidth < 768) await openNavigation(".needs-attention");
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    prepareAttention(bridge);
  } },
  { ...base("chat-heartbeat", "Run silent for 16 seconds", () => stalledHeader() ?? (hasLayout() ? null : runningHeader()), async (root) => {
    await submitPrompt(root);
    await until(runningHeader, "Run header did not appear");
    // The standalone preview freezes the clock; jsdom runs keep real time and only check the run header.
    if (!hasLayout()) return;
    advancePreviewClock(16_000);
    await until(stalledHeader, "Heartbeat did not appear", 3000);
  }), setup: async (bridge) => { await prepareFixture(bridge, "streaming"); liveRun(bridge, "tool"); } },
  { ...base("chat-status-pill", "Scrolled up while a reply runs", () => select(".chat-status-pill[data-running]") ?? (hasLayout() ? null : runningHeader()), async (root) => {
    await submitPrompt(root);
    await until(runningHeader, "Run header did not appear");
    await scrollThreadUp();
  }), setup: async (bridge) => { await prepareFixture(bridge, "answer"); longThread(bridge); liveRun(bridge, "writing"); } },
  { ...base("chat-status-pill-idle", "Scrolled up after the reply", () => select(".chat-status-pill:not([data-running])") ?? (hasLayout() ? null : select(".message-scroll .message.assistant")), scrollThreadUp), setup: async (bridge) => { await prepareFixture(bridge, "answer"); longThread(bridge); } },
  { ...base("chat-chart-building", "A chart reserves its card while it is built", () => select('.chat-artifact-block[data-state="building"]'), root => submitPrompt(root, "Chart Hanul Precision’s margin against utilization.")), setup: async (bridge) => { await prepareFixture(bridge, "answer"); chartRun(bridge); } },
  { ...base("chat-chart-failed", "A failed chart leaves a compact explanation below the answer", () => select('.chat-artifact-block[data-state="failed"] [role="alert"]'), centerOn('.chat-artifact-block[data-state="failed"]')), setup: async bridge => {
    await prepareFixture(bridge, "answer");
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      const answer = workspace.threads[0]?.messages.find(message => message.role === "assistant");
      if (answer) answer.progress = { startedAtMs: Date.now() - 4_000, finishedAtMs: Date.now(), items: [
        { id: "failed-chart", kind: "tool", toolName: "chart_publish", category: "artifact", operation: "publish", action: "Made a chart", running: "Making a chart", target: previewKo() ? "한울정밀 · 최근 3년 일봉" : "Hanul Precision · three years of daily prices", status: "failed", href: null, startedAtMs: 0, finishedAtMs: null },
      ] };
      return workspace;
    };
  } },
  { ...base("chat-plan-running", "A multi-step reply works through its plan", () => {
    const active = select('.chat-plan [aria-current="step"]');
    return active && select('.turn-ledger[data-live] .chat-progress-toggle') && select('.chat-artifact-block[data-state="building"]') ? select(".chat-plan") : null;
  }, async (root) => {
    await submitPrompt(root, "Why did Hanul Precision’s margin fall while revenue grew?");
    await click("button[data-research-summary]", root);
    await until(() => select(".chat-plan"), "Plan did not appear");
    // A short window follows the reserved chart card; scroll back to the plan, which shows the status pill.
    const thread = select(".message-scroll");
    if (!hasLayout() || !thread || thread.scrollHeight - thread.clientHeight <= 120) return;
    thread.scrollTop = 0;
    thread.dispatchEvent(new Event("scroll"));
    await new Promise((resolve) => setTimeout(resolve, 300));
  }), setup: async (bridge) => { await prepareFixture(bridge, "streaming"); planRun(bridge); asSession(bridge); } },
  // Between steps the live line names the last finished batch, or the note that came after it, with the timer.
  { ...base("chat-live-batch", "Between steps, the live line names the last finished batch", () => liveLineOf("batch"), betweenSteps("batch")),
    setup: async (bridge) => { await prepareFixture(bridge, "streaming"); betweenStepsRun(bridge, false); asSession(bridge); } },
  // The note is on the page right above the line, so the reply's own line does not repeat it.
  { ...base("chat-live-note", "Between steps, a note stays on the page and the line under it reads Thinking", () =>
      select('.message.assistant.streaming [data-turn-item="prose"]') ? liveLineOf("thinking") : null, betweenSteps("thinking")),
    setup: async (bridge) => { await prepareFixture(bridge, "streaming"); betweenStepsRun(bridge, true); asSession(bridge); } },
  // Scrolled up, the note is out of view: the status pill names its last sentence and keeps the clock.
  { ...base("chat-live-note-pill", "Scrolled up between steps, the status pill names the latest note's sentence", () =>
      select(".chat-status-pill[data-running]:has(.chat-status-pill-text)") ?? (hasLayout() ? null : runningHeader()), async (root) => {
    await betweenSteps("thinking")(root);
    await scrollThreadUp();
  }), setup: async (bridge) => { await prepareFixture(bridge, "answer"); longThread(bridge); betweenStepsRun(bridge, true); asSession(bridge); } },
  // A clear scheduling request produces an active saved receipt in Bot.
  { ...base("chat-routine-proposed", "A routine saved from a clear request shows its next run", () => select(".message-schedule"), async () => {
    await until(() => select(".message-schedule-open"), "The saved routine did not render");
    if (hasLayout()) select(".message-schedules")?.scrollIntoView({ block: "center" });
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const schedule = { kind: "weekly" as const, days: [1], hour: 8, minute: 0, timezone: "Asia/Seoul" };
    const task = { id: "preview-routine", revision: 1, loupist_id: "loupist-quality", kind: "custom" as const, title: "Compare inventory days", prompt: "Compare inventory days across the assets I follow and flag any above 60 days.",
      schedule, target: { kind: "new_chat" as const }, model_profile_id: "preview", thinking_level: "medium", run_options: {}, use_memory: true, update_memory: false, status: "active" as const,
      created_at_ms: Date.now(), updated_at_ms: Date.now(), next_run_at_ms: Date.now() + 86_400_000, last_run_at_ms: null, last_error: null, session_thread_id: null, awaiting_confirmation: false };
    bridge.installScheduledTask(task);
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      const [question, answer] = workspace.threads[0].messages;
      if (question?.role === "user") question.content = "Every Monday at 8, compare inventory days for what I follow.";
      if (answer?.role === "assistant") {
        Object.assign(answer, { content: "Saved **Compare inventory days** for every Monday at 8:00, Seoul time. The app must be open for it to run.", evidence_refs: [], memory_update: undefined });
        answer.schedule_receipts = [{ task_id: task.id, title: task.title, revision: task.revision, action: "created", status: "active", schedule, target: { kind: "new_chat" }, next_run_at_ms: task.next_run_at_ms, use_memory: true, update_memory: false, awaiting_confirmation: false }];
      }
      return workspace;
    };
  } },
  // W17.3: the run-profile menu names the plan behind the answer under its model list.
  { ...base("composer-plan-line", "The model menu says which plan pays", () => select(".composer-model-menu-panel [data-plan-line]"), async root => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    const trigger = await until(() => select(".composer-model-menu"), "The run-profile control did not render");
    trigger.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 }));
  }) },
  { ...base("composer-model-menu", "The composer shows Auto and its menu offers a model", () => select(".composer-model-menu-panel"), async root => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    // Radix menus open on pointer down, not on click.
    const trigger = await until(() => select(".composer-model-menu"), "The run-profile control did not render");
    trigger.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 }));
  }) },
  { ...base("composer-command-menu", "/ offers finance commands, then Skills", () => select('.composer-assist[data-symbol="/"]'), async root => {
    const composer = await until(() => root.querySelector<HTMLTextAreaElement>("textarea[data-chat-composer]"), "Composer did not appear");
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(composer, "/");
    composer.setSelectionRange(composer.value.length, composer.value.length);
    composer.dispatchEvent(new Event("input", { bubbles: true }));
  }), setup: bridge => prepareFixture(bridge, "answer") },
  { ...base("chat-ticker-text", "Historical ticker references remain ordinary message text", () =>
      [...document.querySelectorAll(".message-user-text")].find(message => visible(message) && message.textContent?.includes("$AAPL:NASDAQ (Apple)")) ?? null),
    setup: async bridge => {
      await prepareFixture(bridge, "answer");
      const original = bridge.loupistSelect.bind(bridge);
      bridge.loupistSelect = async loupistId => {
        const workspace = await original(loupistId);
        const question = workspace.threads[0].messages.find(item => item.role === "user");
        if (question) question.content = previewKo() ? "$AAPL:NASDAQ (Apple) 뭐가 달라졌어? 비용은 $5였어." : "$AAPL:NASDAQ (Apple) What changed? It cost $5.";
        const answer = workspace.threads[0].messages.find(item => item.role === "assistant");
        if (answer) Object.assign(answer, { content: previewKo() ? "애플의 영업이익률이 올랐습니다." : "Apple's operating margin increased.", evidence_refs: [], memory_update: undefined });
        return workspace;
      };
    },
  },
  // A pending decision remains actionable with the activity summary folded.
  {
    ...base("chat-approval-card", "A read-only connection waits for the user's OK", () => select(".turn-block .approval-card"),
      async root => { await submitPrompt(root, "Read Hanul Precision’s Q3 filings and tell me what changed."); await centerOn(".approval-card")(); }),
    setup: async (bridge) => { await prepareFixture(bridge, "answer"); approvalRun(bridge); },
  },
  {
    ...base("chat-question-card", "The agent asks one question mid-reply and waits for the user", () => select(".turn-block .question-card"),
      async root => { await submitPrompt(root, "Compare Hanul Precision with its peers."); await centerOn(".question-card")(); }),
    setup: async (bridge) => {
      await prepareFixture(bridge, "answer");
      bridge.chatSend = async (request, observer) => {
        const run_id = request.run_id;
        const at = Date.now();
        observer({ type: "started", run_id });
        observer({ type: "progress", run_id, sequence: 1, progress: {
          startedAtMs: at - 8_000,
          items: [{ id: "preview-ask", kind: "tool", toolName: "ask_user", category: "capability", operation: "generic", action: "Asked you a question", running: "Asking you a question", status: "running", startedAtMs: at - 6_000, target: null, href: null, finishedAtMs: null }],
          questions: [{
            requestId: "preview-question", itemId: "preview-ask", question: "Which period should the comparison use?",
            options: [
              { label: "Last 4 quarters", description: "Trailing twelve months, closest to today" },
              { label: "Last 3 fiscal years", description: "Smooths out one-off quarters" },
            ],
            recommended: 0, state: "waiting", requestedAtMs: at - 6_000,
          }],
          finishedAtMs: null,
        } });
        observer({ type: "phase", run_id, phase: "tool" });
        return { run_id };
      };
    },
  },
  {
    ...base("chat-approval-decided", "Inspect a settled approval", () => select(".approval-line"), async root => {
      await click("button[data-research-summary]", root);
      await centerOn(".approval-line")();
    }),
    setup: async (bridge) => {
      await prepareFixture(bridge, "answer");
      const original = bridge.loupistSelect.bind(bridge);
      bridge.loupistSelect = async loupistId => {
        const workspace = await original(loupistId);
        const answer = workspace.threads[0].messages.find(item => item.id === "preview-answer");
        if (answer) {
          const at = Date.now();
          answer.progress = { startedAtMs: at - 30_000, finishedAtMs: at, items: approvalSteps(at, "succeeded"), approvals: [approvalFor("allowed_once", at)] };
        }
        return workspace;
      };
      asSession(bridge);
    },
  },
  { ...base("chat-private-marker", "A private chat stays marked in the sidebar", () => select(".loupist-private-mark"), async () => {
    await until(() => select("textarea[data-chat-composer]"), "Chat did not initialize");
    await openNavigation(".loupist-private-mark");
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      Object.assign(workspace.threads[0], { use_memory: false, update_memory: false });
      // Rust refuses Memory writes in a private chat, so its answers carry no saved items.
      for (const message of workspace.threads[0].messages) delete message.memory_update;
      return workspace;
    };
  } },
  { ...base("chat-title-editing", "A session's menu opens its rename dialog", () => select("#rename-thread-name"), async () => {
    await until(() => select("textarea[data-chat-composer]"), "Chat did not initialize");
    await openNavigation('[data-thread-id="preview-chat"]');
    const row = await until(() => document.querySelector('[data-thread-id="preview-chat"]')?.closest(".loupist-chat-row"), "Session did not initialize");
    const trigger = row.querySelector<HTMLButtonElement>(".loupist-chat-menu")!;
    trigger.focus();
    pressMenu(trigger);
    const rename = await until(() => [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find(item => item.textContent?.trim() === i18n.t("app:loupistChatItem.rename")), "Rename action did not appear");
    rename.click();
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    // The home Thread keeps its name; this scenario opens the answer as an ordinary topic Chat.
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      Object.assign(workspace.threads[0], { home: false });
      return workspace;
    };
  } },
  { ...base("shell-unsent-draft", "A chat with unsent text shows a dot in the sidebar", () => select(".loupist-draft-dot"), async root => {
    const composer = await until(() => root.querySelector<HTMLTextAreaElement>("textarea[data-chat-composer]"), "Composer did not appear");
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(composer, "Check inventory days against utilization next");
    composer.dispatchEvent(new Event("input", { bubbles: true }));
    // Narrow windows keep the sidebar behind its toggle.
    if (hasLayout() && document.documentElement.clientWidth < 768) await openNavigation(".loupist-draft-dot");
  }), setup: (bridge) => prepareFixture(bridge, "answer") },
  // While a reply runs, typing offers its three controls and the line under the message says what each key does.
  { ...base("composer-run-typed", "While a reply runs, typed text offers Send next, Guide and Stop, and says what each does",
    () => select(".composer-run-help") && select("button.composer-queue-action") && select("button.composer-steer-action") && select("button.composer-stop-action"), async root => {
      await submitPrompt(root);
      await until(runningHeader, "Run header did not appear");
      await typeInComposer(root, previewKo() ? "새 생산라인만 봐 줘" : "Focus on the new line only");
    }), setup: (bridge) => prepareFixture(bridge, "streaming") },
  // Right after Stop: the reply says it stopped and the message queued behind it is one press away.
  { ...base("chat-stopped-queue", "After Stop, the kept message waits and Send next now sends it", () => select(".chat-pending-resume") && select("button.composer-tile[data-state='send']"), async root => {
    await submitPrompt(root);
    await until(runningHeader, "Run header did not appear");
    await typeInComposer(root, previewKo() ? "그다음 모로우부품과 비교해 줘" : "Then compare it with Morrow Components");
    await click("button.composer-queue-action", root);
    await until(() => select(".chat-pending-card"), "The queued row did not appear");
    // The host's confirmed stop, as after the Stop tile: the frozen preview clock never ends the renderer's own wait for it.
    await stopPreviewRun?.();
    await until(() => select(".chat-pending-resume"), "The held queue did not offer the next message");
  }), setup: async (bridge) => { await prepareFixture(bridge, "streaming"); stopPreviewRun = () => bridge.chatAbort("preview"); } },
  // A queued message edited in place: Enter saves, Esc cancels, and its row keeps its place.
  { ...base("chat-queue-editing", "A queued message being edited in place", () => select(".chat-pending-editor"), async root => {
    await submitPrompt(root);
    await until(runningHeader, "Run header did not appear");
    await typeInComposer(root, previewKo() ? "그다음 모로우부품과 비교해 줘" : "Then compare it with Morrow Components");
    await click("button.composer-queue-action", root);
    const actions = await until(() => select(".chat-pending-more"), "The queued row did not appear");
    pressMenu(actions);
    const edit = await until(() => [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find(item => item.textContent?.trim() === i18n.t("chat:chatPendingQueue.editQueuedMessage")), "Edit did not appear");
    edit.click();
    await until(() => select(".chat-pending-editor"), "The editor did not open");
  }), setup: (bridge) => prepareFixture(bridge, "streaming") },
];

/** Ends the streaming preview reply as the host does once a stop is confirmed. */
let stopPreviewRun: (() => Promise<void>) | undefined;

/** Types into the composer as the user would, with the caret at the end. */
async function typeInComposer(root: HTMLElement, text: string) {
  const composer = await until(() => root.querySelector<HTMLTextAreaElement>("textarea[data-chat-composer]"), "Composer did not appear");
  composer.focus();
  Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(composer, text);
  composer.setSelectionRange(text.length, text.length);
  composer.dispatchEvent(new Event("input", { bubbles: true }));
}
