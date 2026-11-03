import { errorMessage } from "./errors";

it("preserves Error, string and serialized Tauri command messages, else uses the fallback", () => {
  expect(errorMessage(new Error("native error"), "fallback")).toBe("native error");
  expect(errorMessage("string error", "fallback")).toBe("string error");
  expect(errorMessage({ code: "internal", message: "Tauri error" }, "fallback")).toBe("Tauri error");
  expect(errorMessage({ message: "" }, "fallback")).toBe("fallback");
  expect(errorMessage(null, "fallback")).toBe("fallback");
});
