/**
 * Research hand-offs (Agents A5): a retained link's "Look into it" in Bot while it starts,
 * fails, finds the link gone or opens once however often it is pressed; what Bot posts when a
 * session ends, a pre-update one the host stopped among them (a session at work posts
 * nothing); and a session's first message with Bot's brief above the closed hand-off context, a
 * chosen link action, or no note at all. Discovery posts no new links (world_followups), so
 * these draw stored rows.
 */
import { commandError } from "../../bridge/mock/commandError";
import type { ChatMessage, HomeHandoff } from "../../contracts/chat";
import { FIXTURE_TIME, prepareFixture, PreviewBridge, previewKo } from "../fixtures";
import { asSession, base, centerOn, click, select, until, type PreviewScenario } from "./kit";

const at = (minutesAgo: number) => new Date(FIXTURE_TIME - minutesAgo * 60_000).toISOString();

/** Shows these messages in the fixture's first thread (Bot, until `asSession` makes it a session). */
async function withMessages(bridge: PreviewBridge, messages: () => ChatMessage[]) {
  await prepareFixture(bridge, "answer");
  const original = bridge.loupistSelect.bind(bridge);
  bridge.loupistSelect = async (loupistId) => {
    const workspace = await original(loupistId);
    workspace.threads[0]!.messages = messages();
    return workspace;
  };
}

function linkPost(): ChatMessage {
  const ko = previewKo();
  return {
    id: "preview-link", role: "assistant", status: "complete", created_at: at(20), host_composed: true, content: "",
    link: {
      connectionId: "con_preview", seedName: ko ? "한울정밀" : "Hanul Precision", farName: ko ? "모로 부품" : "Morrow Components",
      path: [{ name: ko ? "가동률" : "Capacity utilization", relation: "AFFECTS" }, { name: ko ? "모로 부품" : "Morrow Components", relation: "SUPPLIES", reversed: true }],
      nodeIds: ["preview-hanul", "preview-utilization", "preview-morrow"],
    },
  };
}

/** The link post framed at the top of the view, above the greeting that follows host posts. */
const showLink = centerOn(".thread-link", "start", '.bot-greeting[aria-busy="false"]');
const lookInto = async () => { await showLink(); await click(".thread-link .bot-quiet-action:nth-of-type(2)"); };
/** Look into it, then the post framed again once its outcome has grown it (a thread opened at its end keeps to it). */
const lookIntoUntil = (outcome: string) => async () => {
  await lookInto();
  await until(() => select(outcome), `No outcome: ${outcome}`);
  await showLink();
};

function card(id: string, handoff: Omit<HomeHandoff, "handoffId" | "chatId">, content: string, minutesAgo: number): ChatMessage {
  return { id, role: "assistant", status: "complete", created_at: at(minutesAgo), host_composed: true, content, handoff: { handoffId: `handoff-${id}`, chatId: `chat-${id}`, ...handoff } };
}

function handoffCards(): ChatMessage[] {
  const ko = previewKo();
  return [
    card("done", { title: ko ? "HBM 공급" : "HBM supply", status: "done" }, ko ? "HBM4 공급은 2027년에 넓어집니다." : "HBM4 supply widens in 2027.", 40),
    card("failed", { title: ko ? "재고 일수" : "Inventory days", status: "failed" }, "", 20),
    card("legacy", { title: ko ? "업데이트 전 요청" : "Request from before the update", status: "stopped", stopReason: "before_update" }, "", 10),
  ];
}

/** Every way a session ends without an answer: the app quit, the user stopped it, it failed, and each reason the host never ran it. */
function stoppedCards(): ChatMessage[] {
  const ko = previewKo();
  return [
    card("interrupted", { title: ko ? "재고 일수" : "Inventory days", status: "stopped", stopReason: "interrupted" }, "", 50),
    card("stopped", { title: ko ? "고객 구성" : "Customer mix", status: "stopped" }, "", 40),
    card("failed", { title: ko ? "가격 결정력" : "Pricing power", status: "failed" }, "", 30),
    card("legacy", { title: ko ? "업데이트 전 요청" : "Request from before the update", status: "stopped", stopReason: "before_update" }, "", 20),
    card("origin", { title: ko ? "삭제된 질문" : "A deleted question", status: "stopped", stopReason: "origin_unavailable" }, "", 10),
    card("authority", { title: ko ? "권한 밖의 요청" : "A request beyond its permissions", status: "stopped", stopReason: "invalid_authority" }, "", 5),
  ];
}

/**
 * Finished sessions: one with no prose to preview (the host's English caption is not shown), and
 * one with a long title and a count of what it learned, whose status line wraps at 700 px.
 */
function finishedCards(): ChatMessage[] {
  const ko = previewKo();
  const learned = { messageId: "answer-preview", pages: [
    { recordId: "rec_preview_a", title: "HBM", kind: "subject" as const, created: true, outcomes: [
      { opId: "op-a", outcome: "added", key: "supply" }, { opId: "op-b", outcome: "added", key: "pricing" }, { opId: "op-c", outcome: "replaced", key: "customers" },
    ] },
  ] };
  const long = ko
    ? "2027년에 HBM4를 대량으로 공급할 수 있는 다른 회사와 그 회사의 고객별 공급 조건"
    : "Who else can supply HBM4 at volume in 2027, and on what terms with which customers";
  return [
    card("empty", { title: ko ? "가동률 비교" : "Utilization comparison", status: "done" }, `Session ended: ${ko ? "가동률 비교" : "Utilization comparison"}.`, 20),
    card("long", { title: long, status: "done", learned }, ko
      ? "SK하이닉스가 2027년까지 HBM4 물량의 절반 이상을 공급하고, 삼성전자는 하반기에 인증을 마칠 것으로 보입니다."
      : "SK hynix supplies more than half of HBM4 volume through 2027, and Samsung Electronics looks set to qualify in the second half.", 10),
  ];
}

/** The hand-off cards framed at the top of the view, above the greeting that follows host posts. */
const showCards = centerOn(".thread-handoff", "start", '.bot-greeting[aria-busy="false"]');

// A request past 2,000 characters whose last sentence limits the answer.
function longRequest() {
  const ko = previewKo();
  const body = ko
    ? "한울정밀과 모로 부품의 최근 세 분기 가동률, 가격, 고객 구성을 비교하고 공급 계약의 변화가 마진에 어떤 영향을 주는지 정리해 주십시오. "
    : "Compare Hanul Precision and Morrow Components on utilization, pricing and customer mix over the last three quarters, and set out how changes in supply contracts reach margins. ";
  return `${body.repeat(ko ? 30 : 14)}${ko ? "표 하나로 답하고 전망은 넣지 마십시오." : "Answer in one table and leave out any forecasts."}`;
}

function sessionFirst(kind: "note" | "link" | "legacy"): ChatMessage[] {
  const ko = previewKo();
  const { connectionId, seedName, farName, path } = linkPost().link!;
  const note = {
    title: ko ? "한울정밀 공급 점검" : "Hanul supply check",
    question: ko ? "한울정밀 외에 같은 부품을 공급하는 회사는 어디입니까?" : "Who else supplies the same part as Hanul Precision?",
    brief: ko ? "최근 공시와 저장된 출처를 먼저 확인하십시오. 수치에는 출처를 붙이십시오." : "Check the latest filings and saved sources first. Cite every figure.",
  };
  const first: ChatMessage = kind === "note"
    ? { id: "preview-first", role: "bot_handoff", status: "complete", created_at: at(5), content: note.brief, handoff_note: { ...note, userMessageInBot: longRequest() } }
    : kind === "link"
      ? { id: "preview-first", role: "user", status: "complete", created_at: at(5), content: "The user chose “Look into it” on this World link.",
        handoff_action: { kind: "link", connectionId },
        handoff_note: {
          title: ko ? `연결: ${seedName} ↔ ${farName}` : `Link: ${seedName} and ${farName}`,
          linkPath: { seed: seedName, far: farName, path: [
            { from: seedName, relation: "AFFECTS", to: path[0]!.name },
            { from: farName, relation: "SUPPLIES", to: path[0]!.name },
          ] },
        } }
      : { id: "preview-first", role: "user", status: "complete", created_at: at(5), content: `${note.question}\n\n${note.brief}` };
  return [first, { id: "preview-reply", role: "assistant", status: "complete", created_at: at(4), content: ko ? "저장된 출처를 확인했습니다." : "The saved sources were checked." }];
}

const session = (kind: "note" | "link" | "legacy"): PreviewScenario["setup"] => async (bridge) => {
  await withMessages(bridge, () => sessionFirst(kind));
  asSession(bridge);
};

export const handoffScenarios: PreviewScenario[] = [
  { ...base("bot-link", "Bot: a retained link with Show in World, Look into it and Not useful", () => select(".thread-link .bot-quiet-action:nth-of-type(2)"), showLink),
    setup: (bridge) => withMessages(bridge, () => [linkPost()]) },
  { ...base("bot-link-busy", "Bot: Look into it while the host starts the session; both actions wait", () => select('.thread-link .bot-quiet-action[aria-busy="true"]'), lookInto),
    setup: async (bridge) => {
      await withMessages(bridge, () => [linkPost()]);
      bridge.researchHandoffStart = () => new Promise(() => {});
    } },
  { ...base("bot-link-failed", "Bot: the host could not start (model or settings); the link stays to try again", () => select(".thread-link .inline-error"), lookIntoUntil(".thread-link .inline-error")),
    setup: async (bridge) => {
      await withMessages(bridge, () => [linkPost()]);
      bridge.researchHandoffStart = async () => { throw commandError("provider_unavailable", "The provider is unavailable."); };
    } },
  { ...base("bot-link-gone", "Bot: the host no longer holds the link; it is not marked opened", () => select('.thread-link .thread-delivery-source[role="alert"]'), lookIntoUntil('.thread-link .thread-delivery-source[role="alert"]')),
    // The mock's Bot holds no such link: the host's `not_found`.
    setup: (bridge) => withMessages(bridge, () => [linkPost()]) },
  { ...base("bot-link-opened", "Bot: Look into it pressed three times opens one session", () => select('.thread-link .thread-delivery-source[role="status"]'), async () => {
    await showLink();
    const button = await until(() => select(".thread-link .bot-quiet-action:nth-of-type(2)") as HTMLButtonElement | null, "Look into it did not appear");
    button.click(); button.click(); button.click();
    await until(() => select('.thread-link .thread-delivery-source[role="status"]'), "Look into it showed no outcome");
    await showLink();
  }), setup: async (bridge) => {
    await withMessages(bridge, () => [linkPost()]);
    let started = 0;
    bridge.researchHandoffStart = async () => { started += 1; if (started > 1) throw new Error("A second session started."); return { chatId: "chat-preview", handoffId: "handoff-preview" }; };
  } },
  { ...base("bot-handoff-cards", "Bot: what a session posts when it ends: finished, failed and a pre-update hand-off the host stopped, each with Open", () => select('.thread-delivery [data-stop-reason="before_update"]'), () => showCards()),
    setup: (bridge) => withMessages(bridge, handoffCards) },
  { ...base("session-handoff-note", "Research session: Bot’s brief with its hand-off details closed", () => select(".handoff-opening .message-handoff-note > summary")),
    setup: session("note") },
  { ...base("session-handoff-note-open", "Research session: the Bot's note opened", () => select(".handoff-opening .message-handoff-note[open] .message-handoff-note-title"),
    () => click(".handoff-opening .message-handoff-note > summary")), setup: session("note") },
  { ...base("session-handoff-link", "Research session: the user chose Look into it on a link; the link's details closed", () => select(".message.user [data-handoff-action=\"link\"]")),
    setup: session("link") },
  { ...base("session-handoff-legacy", "Research session from before notes: the request alone, as before", () => select(".message.user .message-user-text")),
    setup: session("legacy") },
  { ...base("bot-handoff-stopped", "Bot: hand-offs that ended without an answer: the app quit, stopped, couldn't finish, and each reason the host never ran one",
    () => select('.thread-handoff [data-stop-reason="invalid_authority"]'), showCards),
    setup: (bridge) => withMessages(bridge, stoppedCards) },
  { ...base("bot-handoff-finished", "Bot: a finished hand-off with no preview, and one with a long title and what it learned, its status line wrapping between items",
    () => select('.thread-handoff[data-status="done"] .bot-meta-item'), showCards),
    setup: (bridge) => withMessages(bridge, finishedCards) },
];
