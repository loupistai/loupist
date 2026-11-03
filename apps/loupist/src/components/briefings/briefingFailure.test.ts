import { briefingFailure } from "./briefingFailure";

it("maps provider failures to an actionable recovery", () => {
  expect(briefingFailure("ECONNREFUSED")).toBe("unreachable");
  expect(briefingFailure("401 invalid_api_key")).toBe("auth");
  expect(briefingFailure("429 insufficient_quota")).toBe("budget");
  expect(briefingFailure("An unexpected execution error")).toBe("generic");
  expect(briefingFailure(null, "provider_timeout")).toBe("unreachable");
});
