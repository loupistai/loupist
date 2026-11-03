import { cadencePhrase, friendlyTimeZone, relativeRun, scheduledTaskTitle } from "./scheduledPresentation";

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
  expect(scheduledTaskTitle("earnings", "Results before and after")).toBe("Results before and after");
  expect(scheduledTaskTitle("earnings", "Results check")).toBe("Results check");
});

it("names a time zone in words and drops the IANA id", () => {
  const named = friendlyTimeZone("Asia/Seoul", "en");
  expect(named).toMatch(/Korea(?:n)? Standard Time/);
  expect(named).not.toContain("Asia/Seoul");
  expect(friendlyTimeZone("Not/AZone", "en")).toBe("Not/AZone");
});
