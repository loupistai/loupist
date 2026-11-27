import { followingBadges, learningProgramsFor, priceChange } from "./assetListing";
import type { ScheduledTaskListItem } from "../../types";

const task = (patch: Partial<ScheduledTaskListItem>): ScheduledTaskListItem => ({
  kind: "custom", id: "task", loupist_id: "loupist", revision: 1, title: "Watch", prompt: "", schedule: { kind: "daily", hour: 8, minute: 0, timezone: "UTC" },
  target: { kind: "new_chat" }, session_thread_id: null, model_profile_id: "", thinking_level: "", run_options: {}, use_memory: true, update_memory: false,
  status: "active", next_run_at_ms: null, last_run_at_ms: null, created_at_ms: 0, updated_at_ms: 0, last_error: null, awaiting_confirmation: false, latest_run: null, ...patch,
});

it.each([
  ["check", task({ trigger: { script: { name: "band", version: 1, sha256: "ab" }, summary: "", tools: [], args: { asset_ids: ["nvda"] }, state: "on", script_state: {}, evaluations: [], counters: { today: { day: "", evaluations: 0, reads: 0, met: 0 }, recent: [], agent_runs_today: 0, error_streak: 0, no_data_streak: 0, backoff_ms: 0, reads_blocked: false }, approved_versions: [], stale_after_days: 5, reads_per_check: 1 } }), "check"],
  ["failing check", task({ subjects: [{ asset: "nvda" }], trigger: { script: { name: "band", version: 1, sha256: "ab" }, summary: "", tools: [], args: {}, state: "failing", script_state: {}, evaluations: [], counters: { today: { day: "", evaluations: 0, reads: 0, met: 0 }, recent: [], agent_runs_today: 0, error_streak: 0, no_data_streak: 0, backoff_ms: 0, reads_blocked: false }, approved_versions: [], stale_after_days: 5, reads_per_check: 1 } }), "check"],
  ["watch preset", task({ preset: "price-move", subjects: [{ asset: "nvda" }] }), "watch"],
  ["custom watch", task({ subjects: [{ asset: "nvda" }] }), "watch"],
  ["daily", task({ kind: "learning" as ScheduledTaskListItem["kind"], subjects: [{ asset: "nvda" }] }), "daily"],
] as const)("badges an active %s", (_label, item, kind) => {
  const badges = followingBadges("nvda", [item]);
  expect(badges.map(badge => badge.kind)).toContain(kind);
  if (_label === "failing check") expect(badges.find(badge => badge.kind === "check")?.failing).toBe(true);
});

it("returns the active learning program whose subject is the asset", () => {
  const daily = task({ kind: "learning", subjects: [{ asset: "nvda" }] });
  const paused = task({ id: "paused", kind: "learning", status: "paused", subjects: [{ asset: "nvda" }] });
  const watch = task({ id: "watch", subjects: [{ asset: "nvda" }] });
  expect(learningProgramsFor("nvda", [daily, paused, watch]).map(item => item.id)).toEqual([daily.id]);
  expect(learningProgramsFor("other", [daily])).toEqual([]);
});

it("omits a change when no earlier stored close exists and never treats that as zero", () => {
  expect(priceChange({ value: 10, date: "2026-11-24" }, [])).toBeNull();
  expect(priceChange({ value: 10, date: "2026-11-24" }, [{ value: 8, date: "2026-11-20" }, { value: 9, date: "2026-11-24" }])).toEqual({ delta: 2, percent: 0.25, date: "2026-11-20" });
});
