import { buildFeedbackEvalCase, portableFeedbackText } from "./answerFeedbackCore";
import { assertScenarioContract, isKoreanUserTurn } from "../../evals/agent/suite-contract.mjs";
import { feedbackTeachingPrompt } from "./answerFeedback";
import { fromLoupistUIMessage, toLoupistUIMessage } from "./ai-sdk";

it("builds a regression scenario the shared suite contract accepts", () => {
  const request = "영업이익을 비교해 주세요";
  const correction = "연결 기준으로 비교하세요.";
  const scenario = buildFeedbackEvalCase("assistant-1234", request, correction);
  assertScenarioContract(scenario, { suite: "feedback", label: "feedback-v1.json" });
  expect(scenario.tags).toEqual(expect.arrayContaining(["user-feedback", "ko"]));
  expect(isKoreanUserTurn(request + correction)).toBe(true);
  expect(scenario.sessions[0].turns[0].user).toBe(request);
  expect(scenario.sessions[0].turns[0].expect.rubric[0].criterion).toContain("연결 기준");
});

it("tags a short Hangul name inside English as English and keeps Japanese and Chinese", () => {
  const request = "Please compare Value Hanbit operating profit in the filing";
  const correction = "한빛은 연결 기준";
  const named = buildFeedbackEvalCase("named", request, correction);
  assertScenarioContract(named, { suite: "feedback", label: "feedback-v1.json" });
  expect(isKoreanUserTurn(request + correction)).toBe(false);
  expect(named.tags).toContain("en");
  expect(named.tags).not.toContain("ko");
  expect(buildFeedbackEvalCase("ja", "売上高を比べてください。前期比も見たいです。", "連結で見てください。").tags).toContain("ja");
  expect(buildFeedbackEvalCase("zh", "请比较两家公司的营业收入和利润。", "使用合并口径。").tags).toContain("zh");
});

it("exports only portable request/correction fields and removes links, credentials, and local paths", () => {
  const json = JSON.stringify(buildFeedbackEvalCase("answer", "Review /Users/alice/private/report.md at https://real.example/report", "Use api_key=secret-value and C:\\Users\\alice\\report.csv, not sk-test-secret or Bearer token-value"));
  for (const secret of ["alice", "secret-value", "real.example", "sk-test-secret", "token-value"]) expect(json).not.toContain(secret);
  expect(JSON.parse(json).sessions[0].memory).toBe("off");
});

it.each([
  ["~/Library/Application Support/loupist/auth.json", ["Application Support", "auth.json"]],
  ["Users/alice/private/report.md", ["Users/alice", "report.md"]],
  ["../.loupist/secrets.txt", ["../", "secrets.txt"]],
  ["./secrets/local.txt", ["./secrets", "local.txt"]],
  ["C:\\Users\\carol\\report.csv", ["carol", "report.csv"]],
  ["aws_secret_access_key=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY", ["aws_secret_access_key", "wJalr"]],
  ["AKIAIOSFODNN7EXAMPLE", ["AKIAIOSFODNN7EXAMPLE"]],
  // Keep the fake token's provider shape without publishing a token-like literal.
  [["xoxb", "123456789012", "abcdefghijklmnopqrstuvwx"].join("-"), ["xoxb-"]],
  ["AIzaSyA1234567890abcdefghijklmnopqrstuv", ["AIza"]],
  ["sk-test-secret", ["sk-test-secret"]],
  ["ghp_1234567890abcdefghijklmnopqrstuv", ["ghp_"]],
  ["api_key = secret-value", ["api_key", "secret-value"]],
  ["my_password = hunter2", ["hunter2"]],
  ["www.secret.example/path", ["www.secret.example", "secret.example"]],
  ["0123456789abcdef0123456789abcdef", ["0123456789abcdef0123456789abcdef"]],
  ["YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXowMTIzNDU2Nzg5QUJD", ["YWJjZGVmZ2hpamts"]],
])("removes %s from an exported case", (sample, leaks) => {
  const cleaned = portableFeedbackText(`Keep this note. ${sample} End of note.`);
  for (const leak of leaks) expect(cleaned).not.toContain(leak);
  expect(cleaned).toContain("Keep this note.");
  expect(cleaned).toContain("End of note.");
});

it("keeps short corrections valid and makes an editable teaching quote with a bounded answer excerpt", () => {
  expect(buildFeedbackEvalCase("a", "Question", "No").sessions[0].turns[0].expect.rubric[0].criterion.length).toBeGreaterThanOrEqual(40);
  expect(feedbackTeachingPrompt("Original answer", "Corrected answer")).toContain("> Original answer");
  expect(feedbackTeachingPrompt("Original answer", "Corrected answer")).toContain("> Corrected answer");
  expect(feedbackTeachingPrompt("x".repeat(8000), "Fix").length).toBeLessThan(1500);
});

it("retains feedback through the Chat registry serialization", () => {
  const message = { id: "answer", role: "assistant" as const, content: "Answer", created_at: "2026-10-18T00:00:00Z", status: "complete" as const, feedback: { rating: "up" as const, correction: "Fix", updatedAt: 5 } };
  expect(fromLoupistUIMessage(toLoupistUIMessage(message)).feedback).toEqual(message.feedback);
});
