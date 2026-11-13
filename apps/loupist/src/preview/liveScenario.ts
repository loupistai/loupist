import type { ChatEvidenceRef, ChatProgress, ChatQuestion, ChatStreamEvent } from "../contracts/chat";
import type { AgentHarnessSnapshot } from "../contracts/model";
import { previewKo, type PreviewBridge } from "./fixtures";
import { liveGridArtifact } from "./gridScenarios";

type ProgressItem = Extract<ChatProgress["items"][number], { kind: "tool" | "system" }> & { kind: "tool" };
type Beat =
  | { at: number; step: ProgressItem }
  | { at: number; commentary: { id: string; text: string } }
  | { at: number; settle: { id: string; text: string } }
  | { at: number; phase: "thinking" | "tool" | "writing" }
  | { at: number; evidence: ChatEvidenceRef }
  | { at: number; artifact: true }
  | { at: number; question: ChatQuestion }
  | { at: number; done: true };

/** How the live reply ends: it completes, the reader stops it, the provider fails, or it asks a question and then completes. */
export type LiveEnding = "complete" | "stopped" | "failed" | "question" | "quick";

const statementsId = (company: string) => `ev_${company}_statements`;
const newsEvidence = (): ChatEvidenceRef => ({
  id: "ev_live_news", title: previewKo() ? "한바다일보 FY2025 실적 기사" : "Hanbada Daily on FY2025 results", kind: "original", url: "https://fictional.example/markets/fy2025-margins",
  provider: "Hanbada Daily", tool_name: "web_fetch", retrieved_at: "2026-03-20T06:00:00Z", published_at: "2026-03-19",
  excerpt: previewKo() ? "한빛화학의 이익률은 원가 하락 덕분에 개선됐다." : "Hanbit Chem's margins improved on lower feedstock costs.", excerpt_truncated: false, content_sha256: "e".repeat(64), warnings: [],
});

/**
 * A research reply on a real clock: a short note, a ledger of steps (three
 * statements read in parallel, a failed page retried, a calculation), a table
 * card prepared then filled, and an answer streamed in uneven provider-sized
 * bursts with a heading, a list and a Markdown table. Pacing, the live line,
 * scroll and the commit can be watched and measured over time.
 */
export const LIVE_ANSWER = [
  "Hanbit Chem has the widest margins of the three: operating margin **11.0%** against 8.4% for Solsup Foods and 5.9% for Iseul Logistics. ",
  `Revenue tells a different story; Solsup is the largest at ₩452.3bn. [1](#evidence-${statementsId("solsup")})`,
  "\n\n### What sets them apart\n\n",
  "- **Feedstock costs.** Hanbit's input prices fell through the second half, which the news coverage also notes. ",
  "[2](#evidence-ev_live_news)\n",
  "- **Scale.** Solsup spreads fixed costs over the largest revenue base, yet its net margin stays below Hanbit's.\n",
  "- **Reporting basis.** Iseul files separate statements only, so its figures leave out subsidiaries.\n",
  "\n### Margins side by side\n\n",
  "| Company | Operating margin | Net margin |\n| --- | ---: | ---: |\n",
  "| Hanbit Chem | 11.0% | 6.9% |\n",
  "| Solsup Foods | 8.4% | 4.7% |\n",
  "| Iseul Logistics | 5.9% | 3.8% |\n",
  "\nThe comparison is fair for operating margin, but read Iseul's net margin with care: separate statements can flatter it. ",
  "Check the next filing's consolidated figures before ranking the three on net margin.",
].join("");

const LIVE_ANSWER_KO = [
  "세 회사 가운데 이익률은 한빛화학이 가장 높아요. 영업이익률이 **11.0%**로, 솔숲식품 8.4%, 이슬물류 5.9%보다 높아요. ",
  `매출 규모는 솔숲식품이 4,523억 원으로 가장 커요. [1](#evidence-${statementsId("solsup")})`,
  "\n\n### 차이를 만든 요인\n\n",
  "- **원재료 가격.** 하반기 들어 한빛화학의 원재료 가격이 내려갔고, 기사도 같은 점을 짚어요. ",
  "[2](#evidence-ev_live_news)\n",
  "- **규모.** 솔숲식품은 가장 큰 매출에 고정비를 나누지만, 순이익률은 한빛화학보다 낮아요.\n",
  "- **공시 기준.** 이슬물류는 별도 재무제표만 공시해서 자회사 실적이 빠져 있어요.\n",
  "\n### 이익률 비교\n\n",
  "| 회사 | 영업이익률 | 순이익률 |\n| --- | ---: | ---: |\n",
  "| 한빛화학 | 11.0% | 6.9% |\n",
  "| 솔숲식품 | 8.4% | 4.7% |\n",
  "| 이슬물류 | 5.9% | 3.8% |\n",
  "\n영업이익률 비교는 공정하지만, 이슬물류의 순이익률은 조심해서 보세요. 별도 기준이라 실제보다 좋아 보일 수 있어요. ",
  "다음 공시의 연결 기준 수치를 확인한 뒤 순이익률 순위를 매기세요.",
].join("");

const QUICK_ANSWER = [
  "Operating margin is operating profit divided by revenue. EBITDA margin adds depreciation and amortization back before dividing, so it is always the higher of the two.\n\n",
  "| Measure | Starts from | Leaves out |\n| --- | --- | --- |\n",
  "| Operating margin | Operating profit | Nothing |\n",
  "| EBITDA margin | Operating profit + D&A | Depreciation and amortization |\n",
  "\nFor a company that has just built a new line the gap widens, because depreciation starts on day one while the line is still filling.",
].join("");

const QUICK_ANSWER_KO = [
  "영업이익률은 영업이익을 매출로 나눈 값이에요. EBITDA 마진은 나누기 전에 감가상각비와 무형자산상각비를 다시 더하기 때문에, 둘 중 항상 더 높게 나와요.\n\n",
  "| 지표 | 출발점 | 빼지 않는 비용 |\n| --- | --- | --- |\n",
  "| 영업이익률 | 영업이익 | 없음 |\n",
  "| EBITDA 마진 | 영업이익 + 상각비 | 감가상각비, 무형자산상각비 |\n",
  "\n새 라인을 막 지은 회사에서는 두 값의 차이가 벌어져요. 라인이 다 차기 전에도 감가상각은 첫날부터 시작되기 때문이에요.",
].join("");

/** The question each live reply answers. */
export const livePrompt = (ending: LiveEnding) => ending === "quick"
  ? (previewKo() ? "영업이익률과 EBITDA 마진은 뭐가 달라?" : "What’s the difference between operating margin and EBITDA margin?")
  : (previewKo() ? "솔숲식품, 이슬물류, 한빛화학의 FY2025 이익률을 비교해 줘." : "Compare Solsup Foods, Iseul Logistics and Hanbit Chem on FY2025 margins.");

/** Provider deltas as Pi sends them: mostly small pieces, now and then a stall followed by a burst. */
function deltas(text: string, start: number) {
  const out: Array<{ at: number; text: string }> = [];
  const parts = text.match(/[\s\S]{1,6}/gu) ?? [text];
  let at = start;
  parts.forEach((part, index) => {
    at += index % 31 === 30 ? 520 : index % 7 === 6 ? 120 : 18;
    out.push({ at, text: part });
  });
  return out;
}

/**
 * Rust streams the answer as a progress commentary item: the whole item once,
 * then the text appended since, at most every 32 ms. A final answer settles
 * with one authoritative delta that names the commentary it replaces, followed
 * by the patch that removes the live copy (`chat/progress.rs`, `pi_dispatch.rs`).
 */
function commentaryBeats(id: string, text: string, start: number, final: boolean): { beats: Beat[]; end: number } {
  const beats: Beat[] = [];
  let pending = "";
  let flushAt = start;
  for (const delta of deltas(text, start)) {
    pending += delta.text;
    if (delta.at - flushAt >= 32) {
      flushAt = delta.at;
      beats.push({ at: delta.at, commentary: { id, text: pending } });
      pending = "";
    }
  }
  const end = flushAt + 40;
  if (pending) beats.push({ at: end, commentary: { id, text: pending } });
  if (final) beats.push({ at: end + 20, settle: { id, text } });
  return { beats, end: end + 20 };
}

function tool(id: string, toolName: string, category: ProgressItem["category"], operation: ProgressItem["operation"], action: string, running: string, target: string): ProgressItem {
  return { id, kind: "tool", toolName, category, operation, action, running, target, href: null, status: "running", startedAtMs: 0, finishedAtMs: null };
}

/** One step from start to end: `ends` after `starts`, succeeded unless `failed`, linking `results`. */
function stepBeats(step: ProgressItem, starts: number, ends: number, extra: { failed?: boolean; results?: ChatEvidenceRef[] } = {}): Beat[] {
  const results = extra.results ?? [];
  return [
    { at: starts, step },
    ...results.map((evidence) => ({ at: ends - 5, evidence })),
    { at: ends, step: { ...step, status: extra.failed ? "failed" : "succeeded", ...(results.length ? { resultIds: results.map((item) => item.id) } : {}) } as ProgressItem },
  ];
}

function researchBeats(ending: LiveEnding): { beats: Beat[]; answerAt: number } {
  const ko = previewKo();
  const grid = liveGridArtifact();
  const statement = (company: string) => grid.evidence.find((item) => item.id === statementsId(company))!;
  const names = { solsup: ko ? "솔숲식품" : "Solsup Foods", iseul: ko ? "이슬물류" : "Iseul Logistics", hanbit: ko ? "한빛화학" : "Hanbit Chem" };
  const note = commentaryBeats("commentary-0", ko
    ? "저장한 노트와 세 회사의 FY2025 재무제표를 먼저 읽고, 이익률을 표로 나란히 놓아 볼게요."
    : "I’ll start with your saved notes and the three FY2025 filings, then line the margins up in a table.", 1_800, false);
  const beats: Beat[] = [{ at: 900, phase: "thinking" }, { at: 1_780, phase: "writing" }, ...note.beats];
  let at = note.end + 300;
  beats.push({ at: at - 40, phase: "tool" });
  beats.push(...stepBeats(tool("live-memory", "world_read", "memory", "read", "Read saved notes", "Reading saved notes", ko ? "솔숲식품 · 이슬물류 · 한빛화학" : "Solsup, Iseul and Hanbit"), at, at + 800));
  at += 850;
  beats.push(...stepBeats(tool("live-filings", "dart_filing_search", "finance", "search", "Searched filings", "Searching filings", ko ? "FY2025 사업보고서" : "FY2025 annual reports"), at, at + 850));
  at += 950;
  // Three statements read in parallel: the live line names the newest and counts the others.
  beats.push(...stepBeats(tool("live-solsup", "dart_financials", "finance", "read", "Read financial statements", "Reading financial statements", names.solsup), at, at + 2_400, { results: [statement("solsup")] }));
  beats.push(...stepBeats(tool("live-iseul", "dart_financials", "finance", "read", "Read financial statements", "Reading financial statements", names.iseul), at + 80, at + 1_900, { results: [statement("iseul")] }));
  beats.push(...stepBeats(tool("live-hanbit", "dart_financials", "finance", "read", "Read financial statements", "Reading financial statements", names.hanbit), at + 160, at + 2_900, { results: [statement("hanbit")] }));
  at += 3_000;
  if (ending === "question") {
    beats.push({ at, question: {
      requestId: "live-question", itemId: "live-ask",
      question: ko ? "순이익률은 어느 기준으로 비교할까요?" : "Which basis should net margin use?",
      options: ko
        ? [{ label: "연결 기준", description: "자회사까지 포함해요" }, { label: "공시된 기준 그대로", description: "이슬물류는 별도 기준으로 남아요" }]
        : [{ label: "Consolidated", description: "Includes subsidiaries" }, { label: "As filed", description: "Iseul stays on separate statements" }],
      recommended: 0, state: "waiting", requestedAtMs: 0,
    } });
    at += 200;
  }
  // A page that fails once and is read again: a fact in the ledger, never an alarm.
  const news = ko ? "FY2025 실적 기사 · fictional.example" : "FY2025 results · fictional.example";
  beats.push(...stepBeats(tool("live-news", "web_fetch", "web", "read", "Read a web source", "Reading fictional.example", news), at, at + 900, { failed: true }));
  at += 1_000;
  beats.push(...stepBeats(tool("live-news-retry", "web_fetch", "web", "read", "Read a web source", "Reading fictional.example", news), at, at + 1_200, { results: [newsEvidence()] }));
  at += 1_300;
  beats.push(...stepBeats(tool("live-calc", "run_python", "compute", "execute", "Ran Python", "Running Python", ""), at, at + 2_600));
  at += 2_800;
  // The table card is reserved when its step starts and fills when the artifact arrives.
  beats.push(...stepBeats(tool("live-grid", "grid_publish", "artifact", "publish", "Built a table", "Building a table", grid.title), at, at + 2_300));
  beats.push({ at: at + 2_290, artifact: true });
  at += 2_600;
  return { beats, answerAt: at };
}

/**
 * The live reply. `ending` picks how it ends; "stopped" and "failed" end it
 * part-way through the answer. Pressing Stop in the preview also stops it.
 */
export function liveResearchRun(bridge: PreviewBridge, ending: LiveEnding = "complete") {
  bridge.chatSend = async (request, observer) => {
    const run_id = request.run_id;
    const started = Date.now();
    const ko = previewKo();
    const progress: ChatProgress = { startedAtMs: started, items: [], finishedAtMs: null };
    let sequence = 0;
    let content = "";
    const grid = liveGridArtifact();
    const research = ending === "quick" ? { beats: [{ at: 1_100, phase: "thinking" } as Beat], answerAt: 2_300 } : researchBeats(ending);
    const answer = ending === "quick" ? (ko ? QUICK_ANSWER_KO : QUICK_ANSWER) : ko ? LIVE_ANSWER_KO : LIVE_ANSWER;
    const beats: Beat[] = [...research.beats, { at: research.answerAt, phase: "writing" }];
    const text = commentaryBeats("commentary-9", answer, research.answerAt + 20, true);
    beats.push(...text.beats);
    const timers: number[] = [];
    let offset = 0;
    let paused: Beat[] | null = null;
    const abort = bridge.chatAbort.bind(bridge);
    const finish = () => timers.forEach((timer) => window.clearTimeout(timer));
    bridge.chatAbort = async (id) => {
      if (id !== run_id) return abort(id);
      finish();
      // The host confirms in the same turn, as the preview's other streams do.
      observer({ type: "aborted", run_id });
    };
    const emit = (event: ChatStreamEvent) => observer(event);
    const patch = (next: Omit<Extract<ChatStreamEvent, { type: "progress_patch" }>["patch"], "sequence">) => {
      sequence += 1;
      emit({ type: "progress_patch", run_id, patch: { sequence, ...next } });
    };
    const answerQuestion = bridge.chatQuestionAnswer.bind(bridge);
    bridge.chatQuestionAnswer = async (answered) => {
      if (answered.request_id !== "live-question") return answerQuestion(answered);
      const question = progress.questions?.find((item) => item.requestId === answered.request_id);
      if (question) Object.assign(question, { state: "answered", answer: answered.choice !== undefined ? { choice: answered.choice } : { text: answered.text ?? "" } });
      const ask = progress.items.find((item) => item.id === "live-ask");
      if (ask?.kind === "tool") Object.assign(ask, { status: "succeeded", finishedAtMs: Date.now() });
      sequence += 1;
      emit({ type: "progress", run_id, sequence, progress: structuredClone(progress) });
      const rest = paused ?? [];
      paused = null;
      offset = 300 - (rest[0]?.at ?? 0);
      schedule(rest);
    };
    const model = { profile_id: "preview-auto", name: "Auto", provider: "openai", model: "preview-model", thinking_level: "medium", run_options: {} };
    const harness: AgentHarnessSnapshot = { schema: "loupist-harness/1", mode: "chat", skill_ids: [], capability_ids: [], digest: "e".repeat(64) };
    const run = (beat: Beat) => {
      if ("done" in beat) {
        progress.finishedAtMs = Date.now();
        emit({
          type: "completed", run_id, lens_receipts: [], tool_refusals: [], host_composed: false, message_id: `live-${run_id}`, final_text: content, created_at: new Date().toISOString(),
          performance: { setupMs: 40, generationMs: Math.max(0, text.end), totalMs: text.end + 250, sessionCache: "warm", inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0 },
          execution_model: model, agent_harness: harness,
        });
      } else if ("phase" in beat) emit({ type: "phase", run_id, phase: beat.phase });
      else if ("evidence" in beat) emit({ type: "evidence", run_id, evidence: beat.evidence });
      else if ("artifact" in beat) emit({ type: "artifact", run_id, artifact: { artifact_id: grid.id, revision: 1, kind: "grid", title: grid.title, digest: "b".repeat(64) } });
      else if ("question" in beat) {
        const now = Date.now();
        const ask: ProgressItem = { ...tool("live-ask", "ask_user", "capability", "generic", "Asked you a question", "Asking you a question", ""), startedAtMs: now };
        progress.items.push(ask);
        progress.questions = [{ ...beat.question, requestedAtMs: now }];
        sequence += 1;
        emit({ type: "progress", run_id, sequence, progress: structuredClone(progress) });
      } else if ("step" in beat) {
        const now = Date.now();
        const index = progress.items.findIndex((item) => item.kind === "tool" && item.id === beat.step.id);
        const previous = index >= 0 ? progress.items[index] : undefined;
        const item = { ...beat.step, ...(beat.step.status === "running" ? { startedAtMs: now } : { startedAtMs: previous?.kind === "tool" ? previous.startedAtMs : now, finishedAtMs: now }) };
        if (index >= 0) progress.items[index] = item; else progress.items.push(item);
        patch({ upsertItems: [structuredClone(item)] });
      } else if ("commentary" in beat) {
        const { id, text } = beat.commentary;
        const existing = progress.items.find((entry) => entry.id === id);
        if (existing?.kind === "commentary") {
          existing.text += text;
          patch({ appendText: [{ id, text }] });
        } else {
          const item = { id, kind: "commentary" as const, text };
          progress.items.push(item);
          patch({ upsertItems: [structuredClone(item)] });
        }
      } else {
        content += beat.settle.text;
        emit({ type: "delta", run_id, text: beat.settle.text, settles: beat.settle.id });
        progress.items = progress.items.filter((entry) => entry.id !== beat.settle.id);
        patch({ removeItemIds: [beat.settle.id] });
      }
    };
    const schedule = (list: Beat[]) => {
      for (const [index, beat] of list.entries()) {
        timers.push(window.setTimeout(() => {
          if (paused) return;
          run(beat);
          // A question holds the reply until it is answered; the clock keeps counting.
          if ("question" in beat) { paused = list.slice(index + 1); finish(); }
        }, beat.at + offset));
      }
    };
    emit({ type: "started", run_id });
    emit({ type: "progress", run_id, sequence: 0, progress: structuredClone(progress) });
    const read = bridge.chatArtifactRead.bind(bridge);
    bridge.chatArtifactRead = async (loupistId, threadId, artifactId, revisionId) => artifactId === grid.id ? structuredClone(grid.view) : read(loupistId, threadId, artifactId, revisionId);
    const middle = text.beats[Math.floor(text.beats.length * 0.45)]?.at ?? text.end;
    if (ending === "stopped") {
      schedule(beats.filter((beat) => beat.at < middle));
      // The preview's store keeps no runs, so the stop arrives as the host's confirmation, as a stop from another window would.
      timers.push(window.setTimeout(() => observer({ type: "aborted", run_id }), middle));
      return { run_id };
    }
    if (ending === "failed") {
      schedule(beats.filter((beat) => beat.at < middle));
      timers.push(window.setTimeout(() => {
        const draft = progress.items.find((item) => item.id === "commentary-9");
        progress.finishedAtMs = Date.now();
        emit({ type: "error", run_id, failure_kind: "provider_unavailable", message: "Provider unavailable", message_id: `live-${run_id}`,
          final_text: draft?.kind === "commentary" ? draft.text : "", created_at: new Date().toISOString(), execution_model: model, agent_harness: harness, progress: structuredClone(progress) });
      }, middle + 40));
      return { run_id };
    }
    schedule([...beats, { at: text.end + 250, done: true }]);
    return { run_id };
  };
}
