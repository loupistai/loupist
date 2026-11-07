import type { ChatProgress, ChatStreamEvent } from "../contracts/chat";
import type { PreviewBridge } from "./fixtures";

type ProgressItem = Extract<ChatProgress["items"][number], { kind: "tool" }>;
type Beat =
  | { at: number; step: ProgressItem }
  | { at: number; commentary: { id: string; text: string } }
  | { at: number; settle: { id: string; text: string } }
  | { at: number; phase: "tool" | "writing" }
  | { at: number; evidence: true };

const evidenceId = `ev_${"c".repeat(32)}`;

/**
 * A research reply on a real clock: tool steps arrive over a few seconds, then
 * the answer streams in uneven provider-sized bursts, so pacing, scroll and the
 * hand-off from activity to prose can be watched and measured over time.
 */
export const LIVE_ANSWER = [
  "Hanul Precision’s operating margin fell from **16.2% to 14.8%** in the third quarter while revenue grew 11% to ₩184.2 billion. ",
  `Most of the decline traces to qualification costs at the new packaging line, which ran at 61% utilization. [1](#evidence-${evidenceId})`,
  "\n\n### What moved the margin\n\n",
  "| Driver | Q2 2026 | Q3 2026 | Effect |\n| --- | ---: | ---: | --- |\n",
  "| Utilization | 78% | 61% | Fixed costs spread over fewer units |\n",
  "| Qualification spend | ₩3.1bn | ₩7.4bn | One-time, ends with customer sign-off |\n",
  "| Inventory days | 47 | 54 | Cash tied up ahead of shipments |\n",
  "\n### What would change the view\n\n",
  "- **Sign-off timing.** If the second customer qualifies in Q4, utilization should recover above 70%.\n",
  "- **Inventory.** Days above 60 would suggest demand, not qualification, is the constraint.\n",
  "- **Pricing.** The report does not mention price cuts; a later disclosure would weaken the one-time reading.\n",
  "\nThe evidence supports a temporary squeeze rather than lasting damage, but it rests on one company report. ",
  "Compare the next filing’s utilization and cash conversion before treating the recovery as likely.",
].join("");

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
 * Rust streams the answer as a progress commentary item whose whole text is
 * re-sent at most every 32 ms, then settles it with one authoritative delta
 * and a patch that removes the live copy (`chat/progress.rs`, `pi_dispatch.rs`).
 */
function commentaryBeats(id: string, text: string, start: number, final: boolean): { beats: Beat[]; end: number } {
  const beats: Beat[] = [];
  let shown = "";
  let flushAt = start;
  for (const delta of deltas(text, start)) {
    shown += delta.text;
    if (delta.at - flushAt >= 32) {
      flushAt = delta.at;
      beats.push({ at: delta.at, commentary: { id, text: shown } });
    }
  }
  const end = flushAt + 40;
  beats.push({ at: end, commentary: { id, text } });
  if (final) beats.push({ at: end + 20, settle: { id, text } });
  return { beats, end: end + 20 };
}

function tool(id: string, toolName: string, category: ProgressItem["category"], operation: ProgressItem["operation"], action: string, running: string, target: string): ProgressItem {
  return { id, kind: "tool", toolName, category, operation, action, running, target, status: "running" };
}

export function liveResearchRun(bridge: PreviewBridge) {
  bridge.chatSend = async (request, observer) => {
    const run_id = request.run_id;
    const started = Date.now();
    const progress: ChatProgress = { startedAtMs: started, items: [] };
    let sequence = 0;
    let content = "";
    const steps = [
      tool("live-memory", "world_read", "memory", "read", "Read saved notes", "Reading saved notes", "Hanul Precision"),
      tool("live-search", "web_search", "web", "search", "Searched the web", "Searching the web", "Hanul Precision Q3 utilization"),
      tool("live-page", "web_fetch", "web", "read", "Read a web source", "Reading hanul.example", "Q3 operating review · hanul.example"),
      tool("live-calc", "bash", "compute", "execute", "Ran a calculation", "Running a calculation", "margin bridge Q2 → Q3"),
    ];
    const preamble = commentaryBeats("commentary-0", "I’ll start with the saved notes and the Q3 operating review, then check the margin bridge.", 900, false);
    const beats: Beat[] = [{ at: 880, phase: "writing" }, ...preamble.beats, { at: preamble.end + 60, phase: "tool" }];
    const toolsAt = preamble.end + 100;
    steps.forEach((step, index) => {
      beats.push({ at: toolsAt + index * 900, step });
      beats.push({ at: toolsAt + index * 900 + 760, step: { ...step, status: "succeeded" } });
      if (step.id === "live-page") beats.push({ at: toolsAt + index * 900 + 770, evidence: true });
    });
    const answerAt = toolsAt + steps.length * 900 + 300;
    beats.push({ at: answerAt, phase: "writing" });
    const text = commentaryBeats("commentary-9", LIVE_ANSWER, answerAt, true);
    beats.push(...text.beats);
    const timers: number[] = [];
    const abort = bridge.chatAbort.bind(bridge);
    bridge.chatAbort = async (id) => {
      if (id !== run_id) return abort(id);
      timers.forEach((timer) => window.clearTimeout(timer));
      observer({ type: "aborted", run_id });
    };
    const emit = (event: ChatStreamEvent) => observer(event);
    observer({ type: "started", run_id });
    observer({ type: "progress", run_id, sequence: 0, progress: structuredClone(progress) });
    for (const beat of beats) {
      timers.push(window.setTimeout(() => {
        if ("phase" in beat) emit({ type: "phase", run_id, phase: beat.phase });
        else if ("evidence" in beat) emit({ type: "evidence", run_id, evidence: { id: evidenceId, title: "Hanul Precision Q3 2026 operating review", kind: "original", url: "https://hanul.example/quarterly-review", provider: "Hanul Precision IR", tool_name: "web_fetch", retrieved_at: new Date().toISOString(), published_at: "2026-10-27", excerpt: "Utilization 61%; qualification spend ₩7.4bn; inventory days 54.", excerpt_truncated: false, content_sha256: "d".repeat(64), warnings: [] } });
        else if ("step" in beat) {
          const now = Date.now();
          const index = progress.items.findIndex((item) => item.kind === "tool" && item.id === beat.step.id);
          const item = { ...beat.step, ...(beat.step.status === "running" ? { startedAtMs: now } : { startedAtMs: started + beat.at - 760, finishedAtMs: now }) };
          if (index >= 0) progress.items[index] = item; else progress.items.push(item);
          sequence += 1;
          emit({ type: "progress_patch", run_id, patch: { sequence, upsertItems: [structuredClone(item)] } });
        } else if ("commentary" in beat) {
          const item = { id: beat.commentary.id, kind: "commentary" as const, text: beat.commentary.text };
          const index = progress.items.findIndex((entry) => entry.id === item.id);
          if (index >= 0) progress.items[index] = item; else progress.items.push(item);
          sequence += 1;
          emit({ type: "progress_patch", run_id, patch: { sequence, upsertItems: [item] } });
        } else {
          content += beat.settle.text;
          emit({ type: "delta", run_id, text: beat.settle.text });
          progress.items = progress.items.filter((entry) => entry.id !== beat.settle.id);
          sequence += 1;
          emit({ type: "progress_patch", run_id, patch: { sequence, removeItemIds: [beat.settle.id] } });
        }
      }, beat.at));
    }
    timers.push(window.setTimeout(() => {
      progress.finishedAtMs = Date.now();
      emit({
        type: "completed", run_id, tool_refusals: [], message_id: `live-${run_id}`, final_text: content, created_at: new Date().toISOString(),
        execution_model: { profile_id: "preview-auto", name: "Auto", provider: "openai", model: "preview-model", thinking_level: "medium", run_options: {} },
        agent_harness: { schema: "loupist-harness/1", mode: "chat", skill_ids: [], capability_ids: [], digest: "e".repeat(64) },
      });
    }, text.end + 250));
    return { run_id };
  };
}
