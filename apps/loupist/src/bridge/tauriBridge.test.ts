import { invoke } from "@tauri-apps/api/core";
import nativeEntry from "../../src-tauri/src/lib.rs?raw";
import { createLoupistBridge } from "./tauriBridge";
import type { ChatStreamEvent } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), Channel: class { onmessage?: (event: unknown) => void } }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

beforeEach(() => vi.mocked(invoke).mockReset());

it("passes typed intent to native commands in the request envelope", async () => {
  const bridge = createLoupistBridge();
  vi.mocked(invoke).mockResolvedValue(undefined);
  await bridge.knowledgeSourceSelectionResolve({ source: { loupistId: "a", recordId: "r", revisionId: "v", spanId: "s", purpose: "renderer_inspection" }, selection: { startUtf16: 2, endUtf16: 8, text: "phrase" } });
  expect(invoke).toHaveBeenLastCalledWith("knowledge_source_selection_resolve", { request: { source: { loupistId: "a", recordId: "r", revisionId: "v", spanId: "s", purpose: "renderer_inspection" }, selection: { startUtf16: 2, endUtf16: 8, text: "phrase" } } });
  await bridge.knowledgeMap({ loupistId: "loupist-quality", maxNodes: 120 });
  expect(invoke).toHaveBeenLastCalledWith("knowledge_map", { request: { loupistId: "loupist-quality", maxNodes: 120 } });
  await bridge.workspaceUiWrite({ version: 1, panels: {} } as never);
  expect(invoke).toHaveBeenLastCalledWith("workspace_ui_write", { request: { state: { version: 1, panels: {} } } });
  await bridge.loupistSelect("loupist-quality");
  expect(invoke).toHaveBeenLastCalledWith("loupist_select", { loupist_id: "loupist-quality" });
});

it("never lets the renderer choose the acknowledged notice version", async () => {
  vi.mocked(invoke).mockResolvedValue({ notice_acknowledged_version: 1 });
  const bridge = createLoupistBridge();
  await bridge.noticeAcknowledge();
  expect(invoke).toHaveBeenLastCalledWith("notice_acknowledge", undefined);
});

it("passes artifact revision selection and restore concurrency intent to Rust", async () => {
  const bridge = createLoupistBridge();
  vi.mocked(invoke).mockResolvedValue(undefined);
  await bridge.chatArtifactRead("loupist", "chat", "chart", 2);
  expect(invoke).toHaveBeenLastCalledWith("chat_artifact_read", { request: { loupist_id: "loupist", thread_id: "chat", artifact_id: "chart", revision_id: 2 } });
  await bridge.chatArtifactVersions("loupist", "chat", "chart");
  expect(invoke).toHaveBeenLastCalledWith("artifact_versions", { request: { loupist_id: "loupist", thread_id: "chat", artifact_id: "chart" } });
  const request = { loupist_id: "loupist", thread_id: "chat", artifact_id: "chart", revision_id: 2, expected_current_revision: 4 };
  await bridge.chatArtifactRestore(request);
  expect(invoke).toHaveBeenLastCalledWith("artifact_restore", { request });
});

it("preserves native cancellation and missing results", async () => {
  const bridge = createLoupistBridge();
  vi.mocked(invoke).mockResolvedValue(null);
  expect(await bridge.pickFile({ title: "Holdings", filters: [{ name: "CSV", extensions: ["csv"] }] })).toBeNull();
  expect(await bridge.exportSave({ suggestedName: "map.png", bytes: [1], kind: "png", title: "Save image" })).toBeNull();
  expect(await bridge.siteFavicon("https://example.com")).toBeNull();
  expect(invoke).toHaveBeenLastCalledWith("site_favicon", { url: "https://example.com" });
});

it("streams observed commands through the on_event channel", async () => {
  const bridge = createLoupistBridge();
  const observer = vi.fn();
  vi.mocked(invoke).mockResolvedValue({ run_id: "run-1" });
  const request = { loupist_id: "loupist-quality", thread_id: "thread-1", prompt: "Hi" };
  await bridge.chatSend(request as never, observer);
  const [command, args] = vi.mocked(invoke).mock.calls.at(-1)!;
  expect(command).toBe("chat_send");
  expect(args).toMatchObject({ request });
  const event: ChatStreamEvent = { type: "started", run_id: "run-1" };
  (args as { on_event: { onmessage: (event: ChatStreamEvent) => void } }).on_event.onmessage(event);
  expect(observer).toHaveBeenCalledWith(event);
});

/** Command names in `generate_handler![...]`, the only native IPC registry. */
const registeredCommands = (source: string) => {
  const block = source.match(/generate_handler!\[([\s\S]*?)\n\s*\]\)/)?.[1];
  if (!block) throw new Error("generate_handler! registry not found");
  return new Set(
    block
      .replace(/\/\/.*$/gm, "")
      .replace(/#\[[^\]]*\]/g, "")
      .split(",")
      .map((entry) => entry.trim())
      .filter(Boolean)
      .map((path) => path.split("::").at(-1)!),
  );
};

it("sends only registered native commands, and every registered command has a bridge caller", async () => {
  vi.mocked(invoke).mockResolvedValue(undefined);
  const bridge = createLoupistBridge() as unknown as Record<string, (...args: unknown[]) => unknown>;
  for (const method of Object.values(bridge)) await method(undefined, () => {}, undefined, undefined);
  const sent = new Set(vi.mocked(invoke).mock.calls.map(([command]) => command));
  const registered = registeredCommands(nativeEntry);
  expect(sent.size).toBeGreaterThan(100);
  expect([...sent].filter((command) => !registered.has(command))).toEqual([]);
  expect([...registered].filter((command) => !sent.has(command))).toEqual([]);
});

it("passes the explicit Source retry intent to the host", async () => {
  const bridge = createLoupistBridge();
  vi.mocked(invoke).mockResolvedValue(undefined);
  await bridge.knowledgeGraphRetrySource({ loupistId: "a", recordId: "source-a" });
  expect(invoke).toHaveBeenLastCalledWith("knowledge_graph_retry_source", { request: { loupistId: "a", recordId: "source-a" } });
});
