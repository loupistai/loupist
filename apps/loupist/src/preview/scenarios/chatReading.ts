/** Preview scenarios, in the capture order `src/preview/scenarios.ts` assembles. */
import { prepareFixture, previewKo } from "../fixtures";
import { base, centerOn, click, hasLayout, inlineChartFixture, select, selectAnswerWords, submitPrompt, until, type PreviewScenario } from "./kit";

/** Reading long threads: dividers, outlines, tables, charts, quotes and chips. */
export const chatReadingScenarios: PreviewScenario[] = [
  // W15.36: reading details. Dividers mark a new day and what finished while the chat was out of sight; long answers get an outline.
  { ...base("chat-dividers", "Dividers mark a new day and what finished while you were away", () => select('.chat-divider[data-kind="new"]'), async () => {
    // Frame the combined day/new-answer divider after the Markdown mounts,
    // rather than leaving it beyond a narrow capture's top scroll edge.
    await centerOn('.chat-divider[data-kind="new"]')();
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    // The user last had this chat in view the evening before the answer finished.
    try { localStorage.setItem("loupist.chatSeen.v1", JSON.stringify({ "preview-chat": Date.parse("2026-10-27T12:00:00Z") })); } catch { /* preview without storage */ }
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      const question = workspace.threads.find(thread => thread.id === "preview-chat")?.messages.find(message => message.role === "user");
      if (question) question.created_at = "2026-10-27T09:00:00Z";
      return workspace;
    };
  } },
  { ...base("chat-branch", "An edited question links to its earlier version", () => select(".message.user .message-version-link"), centerOn(".message.user .message-version-link")), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      const thread = workspace.threads.find(item => item.id === "preview-chat");
      // The question was edited; its first wording stays as an earlier version.
      if (thread) Object.assign(thread, { versions: [{ id: "preview-version", after_message_id: null, first_message_at_ms: Date.parse("2026-10-27T23:50:00Z"), message_count: 2 }] });
      return workspace;
    };
  } },
  { ...base("chat-outline", "A long answer opens with its outline", () => select(".answer-outline[open]"), async () => {
    await click(".answer-outline > summary");
    await centerOn(".answer-outline[open]", "start")();
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const ko = previewKo();
    const sections = ko
      ? ["## 매출", "매출은 11% 늘어 1,842억 원이에요.", "## 영업이익률", "영업이익률은 16.2%에서 14.8%로 낮아졌어요.", "### 인증 비용", "새 생산라인의 인증 비용이 하락의 대부분이에요.", "## 다음에 볼 것", "가동률과 재고일수를 비교해 보세요."]
      : ["## Revenue", "Revenue grew 11% to ₩184.2 billion.", "## Operating margin", "The operating margin fell from 16.2% to 14.8%.", "### Qualification costs", "Most of the decline came from qualification costs at the new line.", "## What to watch next", "Compare utilization against inventory days."];
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      const answer = workspace.threads.find(thread => thread.id === "preview-chat")?.messages.find(message => message.id === "preview-answer");
      if (answer) answer.content = sections.join("\n\n");
      return workspace;
    };
  } },
  { ...base("chat-table", "A wide answer table keeps its figures whole", () => select('.message-content [data-streamdown="table"]'), async () => {
    await centerOn('.message-content [data-streamdown="table"]', "center")();
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const table = [
      "Segment results, ₩ billions.", "",
      "| Segment | Q3 2025 | Q3 2026 | Change | Operating margin | Inventory days | Note |",
      "| :--- | ---: | ---: | ---: | ---: | ---: | :--- |",
      "| Precision parts | 1,204.5 | 1,336.8 | +11.0% | 14.8% | 54 | New production line is still in qualification, so costs run ahead of volume. |",
      "| Tooling | 312.0 | 298.4 | -4.4% | 9.1% | 71 | Two customers delayed orders into next year. |",
      "| Services | 88.2 | 101.7 | +15.3% | 22.6% | 12 | Recurring contracts. |",
    ].join("\n");
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async loupistId => {
      const workspace = await original(loupistId);
      const answer = workspace.threads.find(thread => thread.id === "preview-chat")?.messages.find(message => message.id === "preview-answer");
      if (answer) answer.content = table;
      return workspace;
    };
  } },
  { ...base("chat-chart-inline", "A finished chart sits in the answer", () => {
    const card = select('.chat-artifact-block[data-state="ready"]');
    const chart = card?.querySelector(".artifact-chart-shell");
    // jsdom cannot draw the chart engine; there the chart figure stands in for its canvas.
    return chart && !select(".workspace-panel") && (!hasLayout() || chart.querySelector("canvas")) ? card : null;
  }, async (root) => {
    await submitPrompt(root, "Chart Hanul Precision’s price and volume.");
    await until(() => select('.chat-artifact-block[data-state="ready"]'), "Chart card did not fill");
  }), setup: inlineChartFixture },
  // B73: Rust keeps only a chart's current revision; an earlier answer says its chart changed instead of drawing the newer one.
  { ...base("chat-chart-revised", "An earlier answer's chart was revised by a later answer", () => {
    const card = select('.chat-artifact-block[data-state="revised"]');
    const chart = card?.querySelector(".artifact-chart-shell");
    // Revision metadata arrives before the lazy chart engine finishes loading.
    return chart && (!hasLayout() || chart.querySelector(".chart-financial-canvas canvas")) ? card : null;
  }, async (root) => {
    await submitPrompt(root, "Chart Hanul Precision’s price and volume.");
    await until(() => select('.chat-artifact-block[data-state="revised"]'), "The chart card did not notice the later revision");
    await centerOn('.chat-artifact-block[data-state="revised"]')();
  }), setup: async (bridge) => {
    await inlineChartFixture(bridge);
    // A later answer published revision 2, the only one Rust keeps; this answer recorded revision 1.
    const read = bridge.chatArtifactRead.bind(bridge);
    bridge.chatArtifactRead = async (loupistId, chatId, artifactId, revisionId) => {
      const view = await read(loupistId, chatId, artifactId, revisionId);
      return { ...view, artifact: { ...view.artifact, current_revision: view.artifact.current_revision + 1 } };
    };
  } },
  // W15.20: one chip grammar; a phrase selected in an answer becomes a quote chip.
  { ...base("chat-quote-ask", "Selecting a phrase in an answer offers to ask about it", () => document.getSelection()?.toString().trim() ? select(".quote-ask") : null, async () => {
    await selectAnswerWords(0, 6);
  }), selection: true, setup: (bridge) => prepareFixture(bridge, "answer") },
  { ...base("composer-inline-context", "Inline file reference and a Lens mode in a saved question and draft", () =>
    document.querySelector(".composer-prompt-highlight [data-inline-reference]") && select(".message-user-context .message-tool-label")), setup: async bridge => {
    await prepareFixture(bridge, "answer");
    const read = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async id => {
      const workspace = await read(id);
      const thread = workspace.threads.find(t => t.home);
      if (thread) thread.messages = [{ id: "preview-lens-question", role: "user", status: "complete", created_at: new Date().toISOString(),
        content: previewKo() ? "$lens\n\n지금 어떤 판단 기준을 쓰고 있어?" : "$lens\n\nWhich research principles are active?" }];
      return workspace;
    };
    const token = "@report.md";
    const prompt = previewKo() ? `이 기준으로 ${token} 실적을 살펴봐 줘.` : `Apply these principles to ${token} earnings.`;
    const start = prompt.indexOf(token);
    await bridge.chatDraftWrite({ loupist_id: "loupist-quality", thread_id: "preview-chat" }, {
      prompt, context: { pins: [], asOf: "2026-10-15", chips: [
        { key: "skill:lens", kind: "skill", id: "lens", label: "Lens" },
        { key: "file:report", kind: "file", id: "reports/report.md", label: "report.md" },
      ], inlineReferences: [{ key: "chip:file:report", kind: "file", text: token, start, end: start + token.length }] },
    });
  } },
  { ...base("composer-chips", "Quoted passages stay inline with the question", () => {
    const prompt = select("textarea[data-chat-composer]") as HTMLTextAreaElement | null;
    return document.querySelectorAll('.composer-prompt-highlight [data-inline-reference="quote"]').length === 5 && prompt?.value.trim() ? prompt : null;
  }, async () => {
    // Five different phrases; the Korean answer is short, so they overlap.
    for (const start of [0, 2, 4, 6, 8]) {
      await selectAnswerWords(start, 3, 1);
      await click(".quote-ask");
    }
  }), setup: (bridge) => prepareFixture(bridge, "answer") },
];
