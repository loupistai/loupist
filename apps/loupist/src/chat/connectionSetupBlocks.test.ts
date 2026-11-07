import { describe, expect, it } from "vitest";
import type { ChatMessage, ChatProgressItem } from "../types";
import { connectionSetupBlocks } from "./connectionSetupBlocks";

const step = (status: "running" | "succeeded" | "failed", overrides: Partial<Extract<ChatProgressItem, { kind: "tool" | "system" }>> = {}): ChatProgressItem => ({
  id: `step-${status}-${overrides.target ?? ""}`, kind: "tool", toolName: "connection_setup", category: "capability", operation: "generic",
  action: "Prepared a connection", running: "Preparing a connection", target: "fred.macro", status, ...overrides,
});
const message = (items: ChatProgressItem[], overrides: Partial<ChatMessage> = {}): ChatMessage => ({
  id: "answer", role: "assistant", content: "", created_at: "2026-10-28T00:00:00Z", status: "complete", progress: { startedAtMs: 0, items }, ...overrides,
});

describe("connectionSetupBlocks", () => {
  it("draws one card per connection the reply set up, from completed steps only", () => {
    expect(connectionSetupBlocks(message([step("succeeded")]))).toEqual([{ key: "connection-setup:fred.macro", entryId: "fred.macro" }]);
    expect(connectionSetupBlocks(message([step("running"), step("failed", { target: "Bloomberg" })]))).toEqual([]);
    expect(connectionSetupBlocks(message([
      step("succeeded"), step("succeeded", { id: "again" }), step("succeeded", { target: "sec.edgar" }),
      step("succeeded", { toolName: "capability_load", target: "opendart" }),
    ])).map((block) => block.entryId)).toEqual(["fred.macro", "sec.edgar"]);
  });

  it("ignores user messages and steps without a resolved target", () => {
    expect(connectionSetupBlocks(message([step("succeeded")], { role: "user" }))).toEqual([]);
    expect(connectionSetupBlocks(message([step("succeeded", { target: undefined })]))).toEqual([]);
  });
});
