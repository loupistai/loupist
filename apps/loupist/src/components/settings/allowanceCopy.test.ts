import type { Bucket, Hold } from "../../contracts/generated";
import type { OpenRouterQuota } from "../../contracts/generated/providers/openrouter_quota";
import { backgroundLine, creditLine, holdLine, sourcesLine } from "./allowanceCopy";

const bucket = (state: Bucket["state"], used = 320, limit = 400): Bucket => ({ limit, used, reserved: 0, state });

it("uses the scheduler sentences for off, the 80% warning and the allowance hold", () => {
  expect(backgroundLine(bucket("off", 0, 0))).toBe("Off");
  expect(backgroundLine(bucket("ok", 2, 400))).toBe("Background work: 2 of 400 requests today.");
  expect(backgroundLine(bucket("warning", 320, 400))).toBe("Background work has used 320 of 400 requests today.");
  expect(backgroundLine(bucket("reached", 400, 400))).toBe("Waiting for tomorrow's allowance.");
  expect(sourcesLine(bucket("warning", 240, 300))).toBe("Sources from Chat have used 240 of 300 requests today.");
  expect(sourcesLine(bucket("off", 0, 0))).toBe("Off");
});

it("states a plan limit and a credit hold without promising when they clear", () => {
  const plan: Hold = { reason: "plan_limit", local_day: "2026-11-25", provider: "openai" };
  const credit: Hold = { reason: "out_of_credit", local_day: "2026-11-25", provider: "openrouter" };
  const unnamed: Hold = { reason: "out_of_credit", local_day: "2026-11-25", provider: null };
  expect(holdLine(plan)).toBe("ChatGPT reached its plan limit.");
  expect(holdLine(plan)).not.toMatch(/tomorrow|resumes|resets/i);
  expect(holdLine(credit)).toBe("OpenRouter is out of credit.");
  expect(holdLine(credit)).not.toMatch(/tomorrow|resumes|resets/i);
  expect(holdLine(unnamed)).toBe("This account is out of credit.");
  expect(holdLine({ reason: "allowance", local_day: "2026-11-25", provider: "openai" })).toBe("Waiting for tomorrow's allowance.");
});

it("formats the OpenRouter balance and its reset period", () => {
  const quota: OpenRouterQuota = { limitUsd: 50, remainingUsd: 12.5, usageDailyUsd: 1.25, usageMonthlyUsd: 37.5, reset: "monthly" };
  expect(creditLine(quota)).toBe("$12.50 of $50.00 left · resets monthly");
  expect(creditLine({ ...quota, reset: null })).toBe("$12.50 of $50.00 left");
});
