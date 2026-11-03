import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StrictMode } from "react";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { LoupistWorkspace } from "../../types";
import { loupistIsRunning, openApp, openChat } from "../renderApp";

const run = (run_id: string, loupist_id: string, target: string, started_at_ms: number) => ({ run_id, loupist_id, kind: "chat" as const, target, started_at_ms });

describe("Loupist · Chat run lifecycle", () => {
  it("keeps the App-lifetime Chat registry active under StrictMode", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const chatSend = vi.spyOn(bridge, "chatSend");
    render(<StrictMode><App bridge={bridge} /></StrictMode>);
    await openChat("How should we read the margin decline?");
    await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "Strict lifecycle check");
    await user.click(await screen.findByRole("button", { name: "Send" }));
    await waitFor(() => expect(chatSend).toHaveBeenCalledTimes(1));
    expect(await screen.findByText(/I reviewed “Strict lifecycle check”/)).toBeVisible();
  });

  it("recovers native runs after reload and Stops only the visible exact run", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "runActivityList").mockResolvedValue([
      run("chat-reload-margin", "loupist-quality", "thread-margin", 1),
      run("chat-reload-capital", "loupist-quality", "thread-capital", 2),
      run("chat-reload-downside", "loupist-value", "thread-downside", 3),
    ]);
    const chatAbort = vi.spyOn(bridge, "chatAbort").mockRejectedValueOnce(new Error("Active Chat run not found.")).mockResolvedValue();
    render(<App bridge={bridge} />);
    await openChat("How should we read the margin decline?");
    expect(loupistIsRunning("Quality Compounder")).toBe(true);
    expect(loupistIsRunning("Contrarian Value")).toBe(true);
    expect(screen.getByRole("status", { name: "Capital allocation checklist is running" })).toBeVisible();
    expect(screen.getByRole("textbox", { name: "Message Loupist" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Stop response" }));
    await waitFor(() => expect(chatAbort).toHaveBeenCalledTimes(2));
    expect(chatAbort.mock.calls).toEqual([["chat-reload-margin"], ["chat-reload-margin"]]);
  });

  it("hydrates active Chat before initial selection and rereads the canonical completion", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const originalSelect = bridge.loupistSelect.bind(bridge);
    const stale = await originalSelect("loupist-quality");
    const canonical = structuredClone(stale);
    canonical.threads[0]?.messages.push({ id: "assistant-after-reload", role: "assistant", content: "Canonical response committed after reload.", created_at: "2026-08-10T00:00:00.000Z", status: "complete" });
    const activity = vi.spyOn(bridge, "runActivityList").mockResolvedValueOnce([run("chat-bootstrap", "loupist-quality", "thread-margin", 1)]).mockResolvedValue([]);
    let selections = 0;
    let resolveRecovery!: (workspace: LoupistWorkspace) => void;
    const select = vi.spyOn(bridge, "loupistSelect").mockImplementation((loupistId) => {
      selections += 1;
      if (loupistId !== "loupist-quality") return originalSelect(loupistId);
      return selections === 1 ? Promise.resolve(stale) : new Promise<LoupistWorkspace>((resolve) => { resolveRecovery = resolve; });
    });
    render(<App bridge={bridge} />);
    await openChat();
    await screen.findByText("Updating the answer");
    expect(activity.mock.invocationCallOrder[0]).toBeLessThan(select.mock.invocationCallOrder[0]!);
    expect(screen.getByRole("button", { name: "Stop response" })).toBeVisible();
    await act(async () => resolveRecovery(canonical));
    expect(await screen.findByText("Canonical response committed after reload.")).toBeVisible();
    await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeVisible());
  });

  it("stops the sealed run before the native Started event arrives", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    let sealedRunId = "";
    vi.spyOn(bridge, "chatSend").mockImplementation((request) => new Promise(() => { sealedRunId = request.run_id; }));
    const chatAbort = vi.spyOn(bridge, "chatAbort").mockRejectedValueOnce(new Error("Active Chat run not found.")).mockResolvedValue();
    await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "Stop during native preflight");
    await user.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(sealedRunId).toMatch(/^chat-ui-/));
    await user.click(await screen.findByRole("button", { name: "Stop response" }));
    await waitFor(() => expect(chatAbort).toHaveBeenCalledTimes(2));
    expect(chatAbort.mock.calls).toEqual([[sealedRunId], [sealedRunId]]);
  });
});
