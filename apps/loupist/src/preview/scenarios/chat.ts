/** Preview scenarios, in the capture order `src/preview/scenarios.ts` assembles. */
import { prepareFixture, previewKo } from "../fixtures";
import { prepareMorningFixture, writtenEarlier } from "../fixtures/morning";
import { base, click, hasLayout, messagesRendered, runningHeader, select, submitPrompt, until, type PreviewScenario } from "./kit";

/** The thread, answers, citations and the shapes a failed answer takes. */
export const chatScenarios: PreviewScenario[] = [
  base("chat-empty", "Empty Bot: the Loupist greets and offers questions", () => select('textarea[data-chat-composer]') && select('.bot-greeting[aria-busy="false"]')),
  base("composer-focus", "Composer keyboard focus is a focus-colored edge with a soft halo", () => {
    const field = select('.composer [data-slot="input-group"]:has(textarea:focus-visible)');
    if (!field) return null;
    if (!hasLayout()) return field;
    const style = getComputedStyle(field);
    const transparent = style.borderTopColor === "rgba(0, 0, 0, 0)" || style.borderTopColor === "transparent"; // token-exception: CSSOM serializes the transparent border in a preview assertion
    return style.outlineStyle === "none" && style.borderTopWidth === "1px" && !transparent && style.boxShadow !== "none" ? field : null;
  }, async () => {
    const prompt = await until(() => select("textarea[data-chat-composer]"), "Composer did not appear");
    prompt.focus({ focusVisible: true });
  }),
  // The empty composer is one row: +, the message, the model label and Send share a line in a pill under 50 px.
  base("composer-compact", "Empty composer: one row with +, the message, model and Send", () => {
    const field = select('.composer [data-slot="input-group"]');
    const row = field?.querySelector<HTMLElement>(".composer-footer");
    if (!field || !row?.querySelector("textarea[data-chat-composer]") || !row.querySelector(".composer-add-trigger") || !row.querySelector(".composer-model-menu") || !row.querySelector(".composer-tile")) return null;
    return !hasLayout() || field.getBoundingClientRect().height < 50 ? field : null;
  }),
  // The empty Bot's greeting a day after a visit, on a new Loupist's first day, and with nothing new.
  ...([["thread-morning", "morning", "Bot greeting: two days since the last visit"], ["thread-first-day", "first-day", "Bot greeting: a new Loupist's first day"], ["thread-nothing-changed", "nothing-changed", "Bot greeting: nothing new since yesterday, so no news sentence"]] as const).map(([id, state, title]): PreviewScenario => ({
    ...base(id, title, () => select('textarea[data-chat-composer]') && select('.bot-greeting[aria-busy="false"] .bot-asks')),
    setup: bridge => prepareMorningFixture(bridge, state),
  })),
  // First run: nothing followed and no briefing on, so the greeting's cards are the first moves.
  { ...base("thread-first-run-setup", "Bot greeting: a new user's first moves", () => select('.bot-greeting[aria-busy="false"] [data-ask="follow"]')),
    setup: async bridge => {
      await prepareMorningFixture(bridge, "first-day");
      bridge.assetsList = async () => [];
      bridge.briefingSettingsGet = async () => ({ enabled: false, task: null, schedule: { kind: "weekly", days: [1, 2, 3, 4, 5], hour: 8, minute: 0, timezone: "UTC" } });
    } },
  // The news that closes Bot when the user has not written this visit: the unread briefing and the checks coming due.
  { ...base("thread-news", "Bot's news since the last visit: the briefing and the checks coming due", () => {
    const message = select('.morning-message');
    return message?.querySelector(".morning-bullets") && message.querySelector(".morning-ask") ? message : null;
  }), setup: async bridge => { await prepareMorningFixture(bridge, "morning"); bridge.conversation = "answer"; writtenEarlier(bridge, 3); } },
  base("chat-private", "Private chat with persistent disclosure", () => select(".composer-private-status") && select(".composer-disclosure"), async (root) => {
    const trigger = await until(() => root.querySelector<HTMLElement>(".composer-add-trigger"), "The + control did not render");
    trigger.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 }));
    (await until(() => select("[data-composer-private]"), "Private chat item did not render") as HTMLElement).click();
  }),
  { ...base("chat-answer", "Completed sourced answer", () => {
    const answer = select("article.message.assistant.complete");
    return answer && answer.querySelectorAll(".message-content p").length >= 2 && answer.querySelector(".bot-trace") && answer.querySelector("button.loupist-citation") ? answer : null;
  }), setup: (bridge) => prepareFixture(bridge, "answer") },
  { ...base("chat-no-account", "Saved answer after the last AI account is removed", () => {
    const connect = select("button.composer-model-menu:not([aria-label])");
    return connect && select("article.message.assistant.complete");
  }),
    setup: async (bridge) => {
      await prepareFixture(bridge, "answer");
      for (const provider of (await bridge.modelCatalogGet()).providers) await bridge.providerDisconnect(provider.id);
    } },
  { ...base("chat-streaming", "Answer in progress", () => select('.message.assistant.streaming [data-streamdown="strong"]') && select("button.composer-stop-action"), submitPrompt), setup: (bridge) => prepareFixture(bridge, "streaming") },
  // W15.18 and W15.19/W15.22: guidance and a queued message during a run; what waits across Loupists.
  // W15.26–W15.28: actions on hover, the citation card, and the error shapes.
  { ...base("chat-hover-actions", "Earlier answers show their actions on hover; the newest keeps them", () => select(".message.assistant[data-latest] .message-action-bar")), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      const thread = workspace.threads[0];
      const [question, answer] = thread.messages;
      if (question && answer) {
        thread.messages.push(
          { ...question, id: "preview-follow-up", content: previewKo() ? "재고일수는 어떻게 봐야 해?" : "How should I read the inventory days?", created_at: "2026-10-28T00:05:00Z" },
          { ...answer, id: "preview-follow-up-answer", content: previewKo() ? "재고일수 54일은 같은 업종 평균보다 깁니다. 수요가 현금으로 바뀌는 속도가 아직 느리다는 뜻입니다." : "At 54 days, inventory sits above the peer average. Demand has not yet turned into cash as fast as revenue suggests.", evidence_refs: [], memory_update: undefined, created_at: "2026-10-28T00:05:12Z" },
        );
      }
      return workspace;
    };
  } },
  { ...base("chat-citation-hover", "A citation previews its source", () => select(".citation-preview"), async () => {
    await until(messagesRendered, "Message Markdown did not finish rendering");
    // Keyboard focus opens the card the same way a short hover does.
    (await until(() => select("button.loupist-citation"), "Citation did not render") as HTMLButtonElement).focus();
  }), setup: (bridge) => prepareFixture(bridge, "answer") },
  // W15.28 and W17 (providers §2.4): each failed answer names its account problem and one next step.
  ...([
    ["chat-error-quota", "rate_limited", "A busy AI account counts down to a retry", "openai", "button"],
    ["chat-error-signin", "needs_reconnect", "A sign-in that stopped working asks to reconnect", "openai", '[data-error-action="reconnect"]'],
    ["chat-error-plan-limit", "plan_limit", "A ChatGPT plan at its limit never retries on its own", "openai", '[data-error-action="manage_usage"]'],
    ["chat-error-not-eligible", "plan_not_eligible", "A plan other apps can't use offers an API key", "openai", '[data-error-action="use_api_key"]'],
    ["chat-error-no-credit", "out_of_credit", "A key account out of credit", "anthropic", "button"],
  ] as const).map(([id, kind, title, provider, action]): PreviewScenario => ({
    ...base(id, title, () => select(`.message-run-state ${action}`)),
    setup: async (bridge) => {
      await prepareFixture(bridge, "answer");
      if (kind === "out_of_credit") await bridge.providerConfigure({ provider: "anthropic", api_key: "preview-key" });
      const original = bridge.loupistSelect.bind(bridge);
      bridge.loupistSelect = async loupistId => {
        const workspace = await original(loupistId);
        const answer = workspace.threads[0].messages.find(item => item.id === "preview-answer");
        if (answer) Object.assign(answer, {
          status: "error", failure_kind: kind, evidence_refs: [], memory_update: undefined,
          // A busy provider said how long to wait; Rust passed the bounded hint on.
          ...(kind === "rate_limited" ? { retry_after_seconds: 45 } : {}),
          content: previewKo() ? "한울정밀의 영업이익률은 16.2%에서 14.8%로" : "Hanul Precision’s operating margin fell from 16.2% to",
          execution_model: provider === "anthropic"
            ? { profile_id: "anthropic/claude-sonnet-4-5", name: "Claude Sonnet 4.5", provider, model: "claude-sonnet-4-5", thinking_level: "medium", run_options: {} }
            : { profile_id: "model-test", name: "GPT-5.6 Luna", provider, model: "gpt-5.6-luna", thinking_level: "medium", run_options: {} },
        });
        return workspace;
      };
    },
  })),
  // W17.5: Rust retried a retired model once with the default model within the turn; the answer says which model replaced it.
  { ...base("chat-model-gone", "A retired model is replaced once and the answer says so", () => select(".message-model-note")), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      const answer = workspace.threads[0].messages.find(item => item.id === "preview-answer");
      if (answer) Object.assign(answer, {
        replaced_model: "GPT-5.4",
        execution_model: { profile_id: "openai/gpt-5.6-luna", name: "GPT-5.6 Luna", provider: "openai", model: "gpt-5.6-luna", thinking_level: "medium", run_options: {} },
      });
      return workspace;
    };
  } },
  // W17.5: The host selected a new default model because the old one left the list.
  { ...base("chat-default-changed", "Chat names the model it now uses, once", () => select(".model-notice-line")), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    bridge.setDefaultModelChange("loupist-quality", { id: "preview-notice", previous: { id: "old", name: "GPT-5.4", provider: "openai", model: "old" }, current: { id: "model-test", name: "GPT-5.6 Luna", provider: "openai", model: "gpt-5.6-luna" } });
  } },
  { ...base("chat-offline", "Offline: the thread says so and a sent message waits", () => select(".chat-offline-banner") && select(".chat-pending-card"), async (root) => {
    await submitPrompt(root, previewKo() ? "다시 연결되면 마진을 비교해 줘." : "Compare the margins once I'm back online.");
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    Object.defineProperty(window.navigator, "onLine", { configurable: true, get: () => false });
  } },
  { ...base("chat-steer-queued", "Guidance waits in the timeline and a message waits its turn", () => select(".chat-guiding-row") && select(".chat-pending-card") && select(".chat-pending-heading"), async (root) => {
    await submitPrompt(root);
    await until(runningHeader, "Run header did not appear");
    await until(() => select("button.composer-stop-action") && !select("button.composer-queue-action") && !select("button.composer-steer-action"), "An empty composer should show only stop");
    const composer = await until(() => root.querySelector<HTMLTextAreaElement>("textarea[data-chat-composer]"), "Composer did not appear");
    const type = (text: string) => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(composer, text);
      composer.dispatchEvent(new Event("input", { bubbles: true }));
    };
    type("Focus on the new line only");
    await until(() => select("button.composer-queue-action") && select("button.composer-steer-action") && select("button.composer-stop-action"), "Typing should offer queue, guide and stop");
    await click("button.composer-steer-action", root);
    await until(() => select(".chat-guiding-row"), "The guiding row did not appear");
    type("Then compare it with Morrow Components");
    await click("button.composer-queue-action", root);
    await until(() => select(".chat-pending-card") && select(".chat-pending-heading"), "The queued row did not appear");
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "streaming");
    bridge.chatSteer = async (request) => ({ message_id: "preview-steer", prompt: request.prompt, created_at: new Date().toISOString(), mode: "steer" });
  } },
  { ...base("chat-steer-guided", "Guidance a finished reply accepted reads as a Guided row", () => {
    const row = select(".chat-guided");
    return row && !select(".composer-guidance[data-state='pending']") ? row : null;
  }, async () => {
    const row = await until(() => select(".chat-guided"), "The guided row did not appear");
    await until(() => !select(".composer-guidance[data-state='pending']"), "A finished reply still shows waiting guidance");
    // Narrow windows open at the answer; bring the question, the row and the answer's start into view.
    if (hasLayout()) row.scrollIntoView({ block: "center" });
  }), setup: async (bridge) => {
    bridge.guidedAnswer = true;
    await prepareFixture(bridge, "answer");
  } },
  // Back online: the banner goes and the message that waited is sent by itself.
  { ...base("chat-back-online", "Back online, the message that waited is sent without another press",
    () => !select(".chat-offline-banner") && !select(".chat-pending-card") ? select(".message.assistant[data-latest]") : null, async (root) => {
      await submitPrompt(root, previewKo() ? "다시 연결되면 마진을 비교해 줘." : "Compare the margins once I'm back online.");
      await until(() => select(".chat-pending-card"), "The message did not wait");
      Object.defineProperty(window.navigator, "onLine", { configurable: true, get: () => true });
      window.dispatchEvent(new Event("online"));
      // The queue hands the message over as soon as the connection is back; its reply may still be on its way.
      await until(() => !select(".chat-pending-card"), "The waiting message was not sent");
    }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    Object.defineProperty(window.navigator, "onLine", { configurable: true, get: () => false });
  } },
];
