import i18n from "@/i18n";
import { act } from "@testing-library/react";
import { allowanceHeader, cadencePhrase, friendlyTimeZone, holdSentence, learnedOutcomeText, learnedThingCount, learningBudgetText, learningCapReached, learningPresetText, learningCostNote, learningDefaultCadence, learningDepthText, learningKeepsText, learningReportText, learningRowOutcome, learningScopeText, metDryRunDays, presetRunsOnCheck, relativeRun, runNowConfirmText, runUndoText, runUsageText, runWording, SCHEDULE_SECTIONS, scheduleSection, scheduledTaskTitle, subjectChipModel, taskKindKey, tellsYouKey, trackDailyState, trackDailyTask, triggerArgumentRows, whenMetSummary } from "./scheduledPresentation";

it("phrases a Monday through Friday cadence as weekdays", () => {
  expect(cadencePhrase({ kind: "weekly", days: [5, 1, 4, 2, 3], hour: 8, minute: 30, timezone: "Asia/Seoul" })).toMatch(/^Weekdays /);
  expect(cadencePhrase({ kind: "weekly", days: [1], hour: 9, minute: 0, timezone: "Asia/Seoul" })).toMatch(/^Weekly on Mon at /);
});

it("describes the next run as today or tomorrow in the task time zone", () => {
  const now = Date.parse("2026-10-14T00:00:00Z");
  expect(relativeRun(Date.parse("2026-10-14T08:30:00Z"), now, "UTC")).toMatch(/^Today at /);
  expect(relativeRun(Date.parse("2026-10-15T08:30:00Z"), now, "UTC")).toMatch(/^Tomorrow at /);
  expect(relativeRun(null, now)).toBe("—");
});

it("keeps a saved task name and localizes only the host defaults", () => {
  expect(scheduledTaskTitle("briefing", "Briefings")).toBe("Briefing");
  expect(scheduledTaskTitle("briefing", "Monday research")).toBe("Monday research");
  expect(scheduledTaskTitle("custom", "Supplier update")).toBe("Supplier update");
});

it("shows a saved when-met line, a preset's locale line, or one generic line", () => {
  expect(whenMetSummary("Tell me the move.")).toBe("Tell me the move.");
  expect(whenMetSummary("schedule.preset.priceMove.whenMet")).toBe("You are told the move, both prices and their dates, and what is reported about why.");
  expect(whenMetSummary(undefined, "price-move")).toBe("You are told the move, both prices and their dates, and what is reported about why.");
  expect(whenMetSummary(undefined, "filings-and-results")).toBe("You are told when the check is met.");
  expect(whenMetSummary(undefined)).toBe("You are told when the check is met.");
});

it("calls a task with a trigger a check that tells you when the check is met", () => {
  expect(tellsYouKey({ trigger: { summary: "x" }, report: "every_run" })).toBe("schedule.whenCheckMet");
  expect(tellsYouKey({ report: "when_met" })).toBe("schedule.whenMet");
  expect(tellsYouKey({ report: "every_run" })).toBe("schedule.everyRun");
  expect(taskKindKey({ kind: "custom", trigger: {} })).toBe("schedule.kind.check");
  expect(taskKindKey({ kind: "custom" })).toBe("schedule.kind.custom");
  expect(taskKindKey({ kind: "briefing" })).toBe("schedule.kind.briefing");
  expect(taskKindKey({ kind: "learning", trigger: {} })).toBe("schedule.kind.learning");
  expect([...SCHEDULE_SECTIONS]).toEqual([
    scheduleSection({ kind: "briefing" }),
    scheduleSection({ kind: "custom", trigger: {} }),
    scheduleSection({ kind: "custom" }),
    scheduleSection({ kind: "learning", trigger: {} }),
  ]);
  expect(runWording("briefing")).toBe("briefing");
  expect(runWording("learning")).toBe("learning");
  expect(runWording("custom")).toBe("custom");
});

it("renders arguments as label and value rows and names assets", () => {
  const names = new Map([["asset-1", "Northwind"]]);
  expect(triggerArgumentRows({ band_percent: 5, asset_names: ["Northwind"] }, names)).toEqual([
    { key: "band_percent", label: "band_percent", value: "5" },
    { key: "asset_names", label: "Assets", value: "Northwind" },
  ]);
  expect(triggerArgumentRows({ asset_ids: ["asset-1", "gone"] }, names)[0]).toEqual({
    key: "asset_ids", label: "Assets", value: "Northwind, Removed asset",
  });
  expect(triggerArgumentRows({ asset_ids: ["asset-1"] }, null)[0]?.value).toBe("asset-1");
  expect(presetRunsOnCheck("price-move")).toBe(true);
  expect(presetRunsOnCheck("fact-changes")).toBe(true);
  expect(presetRunsOnCheck("filings-and-results")).toBe(false);
});

it("lists only the dry-run days the check would have been met", () => {
  expect(metDryRunDays([
    { date: "2026-11-12", outcome: "met", reading: "Northwind +6.2%" },
    { date: "2026-11-13", outcome: "not_met", reading: "inside the band" },
    { date: "2026-11-14", outcome: "met", reading: "Northwind −7.4%" },
  ])).toEqual([
    { date: "2026-11-12", outcome: "met", reading: "Northwind +6.2%" },
    { date: "2026-11-14", outcome: "met", reading: "Northwind −7.4%" },
  ]);
});

it("names allowance, holds, learning outcomes and track-daily states", async () => {
  const bucket = (used: number, state: string) => ({ limit: 400, used, state });
  expect(allowanceHeader(bucket(319, "ok"))).toBeNull();
  expect(allowanceHeader({ limit: 0, used: 0, state: "off" })).toBeNull();
  expect(allowanceHeader(bucket(320, "warning"))).toEqual({ tone: "warning", text: "80% of today's allowance used" });
  expect(allowanceHeader(bucket(400, "reached"))).toEqual({ tone: "plain", text: "Waiting for tomorrow's allowance" });
  expect(holdSentence("allowance")).toBe("Waiting for tomorrow's allowance");
  expect(holdSentence("daily_allowance")).toBe("Waiting for tomorrow's allowance");
  expect(holdSentence("plan_limit")).toBe("Your AI plan's limit was reached. Runs resume tomorrow.");
  expect(holdSentence("out_of_credit")).toBe("Your AI plan's limit was reached. Runs resume tomorrow.");
  expect(holdSentence("other")).toBeNull();
  expect(runNowConfirmText("allowance")).toBe("This runs beyond today's allowance.");
  expect(runNowConfirmText("plan_limit")).toBe("Your AI plan's limit was reached. Runs resume tomorrow.");
  expect(runNowConfirmText(null)).toBeNull();

  const usage = (outcome: string, status = "succeeded", requests = 12) => runUsageText({ status, quiet: false, budget: { requests, outcome }, usage: { requests } });
  expect(usage("none")).toBe("12 requests · finished");
  expect(usage("soft")).toBe("12 requests · finished");
  expect(usage("refused", "interrupted")).toBe("12 requests · stopped at its request limit");
  expect(usage("cancelled", "interrupted")).toBe("12 requests · stopped at its request limit");
  expect(runUsageText({ status: "succeeded", quiet: true, budget: { requests: 1, outcome: "none" } })).toBe("1 request · nothing to report");
  expect(runUsageText({ status: "failed", budget: { requests: 4, outcome: "none" } })).toBe("4 requests · couldn't finish");
  expect(runUsageText({ status: "running", budget: { requests: 2, outcome: "none" } })).toBe("2 requests · running");
  expect(runUsageText({ status: "succeeded" })).toBeNull();

  const row = (patch: Parameters<typeof learningRowOutcome>[0]) => learningRowOutcome(patch, false);
  expect(row({ status: "active", latest_run: { status: "running", summary: "Later" } })).toBe("Running…");
  expect(row({ status: "active", hold: { reason: "allowance" }, latest_run: { status: "succeeded", summary: "Saved" } })).toBe("Waiting for tomorrow's allowance");
  expect(row({ status: "active", hold: { reason: "plan_limit" } })).toBe("Your AI plan's limit was reached. Runs resume tomorrow.");
  expect(row({ status: "active", hold: { reason: "out_of_credit" } })).toBe("Your AI plan's limit was reached. Runs resume tomorrow.");
  expect(learningRowOutcome({ status: "active", latest_run: { status: "succeeded", summary: "Saved" } }, true)).toBe("Waiting for another learning run");
  expect(row({ status: "active", latest_run: { status: "interrupted", budget: { outcome: "cancelled" }, summary: "Partial" } })).toBe("Stopped at its request limit.");
  expect(row({ status: "active", latest_run: { status: "interrupted", budget: { outcome: "refused" } } })).toBe("Stopped at its request limit.");
  expect(row({ status: "active", latest_run: { status: "failed", summary: "No" } })).toBe("Couldn't finish this learning");
  expect(row({ status: "active", latest_run: { status: "succeeded", quiet: true, summary: "Quiet" } })).toBe("Nothing to report");
  expect(row({ status: "active", latest_run: { status: "succeeded", summary: "Chip demand held up." } })).toBe("Chip demand held up.");
  expect(row({ status: "paused" })).toBeNull();

  const names = new Map([["asset-1", "Northwind"], ["asset-2", "Harbor"], ["asset-3", "Cedar"], ["asset-4", "Delta"]]);
  const chips = subjectChipModel([
    { asset: "asset-1" }, { asset: "missing" }, { name: "A theme" }, { world: "world-1" }, { lens: "lens-1" },
  ], names);
  expect(chips.shown.map(chip => chip.label)).toEqual(["Northwind", "Removed asset", "A theme"]);
  expect(chips.shown.map(chip => chip.assetId)).toEqual(["asset-1", undefined, undefined]);
  expect(chips.extra).toBe(2);
  expect(chips.shown.some(chip => JSON.stringify(chip).includes("world-1") || JSON.stringify(chip).includes("lens-1"))).toBe(false);

  expect(learningDepthText("deep")).toBe("Deep");
  expect(learningScopeText({ connections: "all_enabled", web: true })).toBe("Every connection turned on for this Loupist · Web research on");
  expect(learningScopeText({ connections: ["conn-a", "conn-b"], web: false })).toBe("2 connections · Web research off");
  expect(learningScopeText({ connections: ["conn-a", "conn-b"], web: false })).not.toContain("conn-a");
  expect(learningKeepsText({ sources: true, lens: false, asset_reads: true })).toBe("Sources, Asset reads");
  expect(learningKeepsText({ sources: false, lens: false, asset_reads: false })).toBe("Nothing kept");
  expect(learningReportText("when_new")).toBe("When there is something new");
  expect(learningReportText(undefined)).toBe("When there is something new");
  expect(learningBudgetText({ soft: 8, hard: 12 })).toBe("Up to 12 requests a run, wrapping up near 8.");
  expect(learningCostNote({ hard: 12 })).toBe("Each run is a model run on your AI account, about 12 requests.");
  expect(learningCostNote(undefined)).toBe("Each run is a model run on your AI account.");
  expect(learningCapReached("up to 20 learning programs can be active in this Loupist")).toBe(20);
  expect(learningCapReached("up to 7 learning programs can be active in this Loupist")).toBe(7);
  expect(learningCapReached("scheduled task compare-and-swap")).toBeNull();
  expect(runUndoText(2)).toBe("Undid this run. 2 sections changed since and were kept.");
  expect(learnedThingCount([{ title: "Harbor", outcomes: [{}, {}] }, { title: "Cedar", outcomes: [] }])).toBe(3);
  expect(learnedOutcomeText([{ outcome: "added" }, { outcome: "section_went_stale" }, { outcome: "added" }])).toBe("Added");

  const tasks = [
    { id: "daily", kind: "learning", preset: "asset-daily", status: "active", subjects: [{ asset: "asset-1" }], latest_run: { status: "succeeded" } },
    { id: "other", kind: "learning", status: "paused", subjects: [{ asset: "asset-1" }, { asset: "asset-2" }] },
    { id: "watch", kind: "custom", preset: "price-move", status: "active", subjects: [{ asset: "asset-1" }] },
  ];
  expect(trackDailyTask(tasks, "asset-1")?.id).toBe("daily");
  expect(trackDailyState(tasks[0])).toBe("on");
  expect(trackDailyState({ ...tasks[0], latest_run: { status: "running" } })).toBe("running");
  expect(trackDailyState({ ...tasks[0], hold: { reason: "allowance" } })).toBe("waiting");
  expect(trackDailyState({ ...tasks[0], status: "paused" })).toBe("off");
  expect(trackDailyState(undefined)).toBe("off");
  expect(learningDefaultCadence("UTC")).toMatch(/Weekdays/);
  expect(learningDefaultCadence("UTC")).toMatch(/6:30/);

  await i18n.changeLanguage("ko");
  try {
    expect(allowanceHeader(bucket(320, "warning"))?.text).toBe("오늘 허용량의 80%를 사용했습니다");
    expect(allowanceHeader(bucket(400, "reached"))?.text).toBe("내일 허용량을 기다리는 중입니다");
    expect(holdSentence("out_of_credit")).toBe("AI 요금제 한도에 도달했습니다. 내일 다시 실행합니다.");
    expect(runNowConfirmText("allowance")).toBe("오늘 허용량을 넘겨 실행합니다.");
    expect(usage("refused", "interrupted")).toBe("요청 12회 · 요청 한도에서 멈췄습니다");
    expect(row({ status: "active", latest_run: { status: "interrupted", budget: { outcome: "cancelled" } } })).toBe("요청 한도에서 멈췄습니다.");
    expect(learningRowOutcome({ status: "active" }, true)).toBe("다른 학습 실행을 기다리는 중입니다");
  } finally {
    await act(async () => { await i18n.changeLanguage("en"); });
  }
});

it("names a time zone in words and drops the IANA id", () => {
  const named = friendlyTimeZone("Asia/Seoul", "en");
  expect(named).toMatch(/Korea(?:n)? Standard Time/);
  expect(named).not.toContain("Asia/Seoul");
  expect(friendlyTimeZone("Not/AZone", "en")).toBe("Not/AZone");
});

it("names every bundled learning preset and no other id", () => {
  expect(["asset-daily", "theme", "company-filings", "macro-series", "question", "market"].map(learningPresetText))
    .toEqual(["Track daily", "Theme", "Company filings", "Economic series", "Question", "Market"]);
  expect(learningPresetText("weather-patterns")).toBeNull();
  expect(learningPresetText(undefined)).toBeNull();
});
