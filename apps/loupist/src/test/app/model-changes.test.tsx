import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { PreviewBridge, prepareFixture } from "../../preview/fixtures";
import { openApp } from "../renderApp";

describe("Loupist · Model changes (W17.5)", () => {
  it("shows the default-changed line once and acknowledges it when the picker opens", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    bridge.setDefaultModelChange("loupist-quality", { id: "notice-1", previous: { id: "old", name: "GPT-5.4", provider: "openai", model: "old" }, current: { id: "model-test", name: "GPT-5.6 Luna", provider: "openai", model: "gpt-5.6-luna" } });
    const ack = vi.spyOn(bridge, "modelNoticeAck");
    await openApp(bridge);
    const line = await waitFor(() => { const found = document.querySelector(".model-notice-line"); expect(found).not.toBeNull(); return found; });
    expect(line).toHaveTextContent("Now answering with GPT-5.6 Luna (was GPT-5.4) Change");
    await user.click(screen.getByRole("button", { name: "Change" }));
    expect(await screen.findByRole("dialog", { name: "Available models" })).toBeInTheDocument();
    await user.keyboard("{Escape}");

    expect(ack).toHaveBeenCalledExactlyOnceWith({ id: "notice-1" });
    await waitFor(() => expect(document.querySelector(".model-notice-line")).toBeNull());
    expect((await bridge.configRead({ scope: { kind: "project", loupistId: "loupist-quality" } })).defaultModelChange).toBeUndefined();
  });

  it("acknowledges the line when the next message goes out, and a default send names no model", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    bridge.setDefaultModelChange("loupist-quality", { id: "notice-2", previous: { id: "old", name: "GPT-5.4", provider: "openai", model: "old" }, current: { id: "model-test", name: "GPT-5.6 Luna", provider: "openai", model: "gpt-5.6-luna" } });
    const ack = vi.spyOn(bridge, "modelNoticeAck");
    const send = vi.spyOn(bridge, "chatSend");
    await openApp(bridge);
    expect(document.querySelector(".model-notice-line")).toHaveTextContent("Now answering with GPT-5.6 Luna (was GPT-5.4)");
    await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "Compare the margins");
    await user.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(send).toHaveBeenCalled());
    expect(ack).toHaveBeenCalledWith({ id: "notice-2" });
    expect(send.mock.calls[0][0]).toMatchObject({ model_profile_id: "", run_options: {} });
  });

  // W17.5: Rust retries a retired model once with the default model inside the turn. The renderer never resends.
  const storedAnswer = (bridge: PreviewBridge, change: Record<string, unknown>) => {
    const original = bridge.loupistSelect.bind(bridge);
    bridge.loupistSelect = async (loupistId) => {
      const workspace = await original(loupistId);
      const answer = workspace.threads[0].messages.find((item) => item.id === "preview-answer");
      if (answer) Object.assign(answer, change);
      return workspace;
    };
  };

  it("never resends when a thread's last answer is a stored model_gone error", async () => {
    const bridge = new PreviewBridge({ delay_ms: 0 });
    await prepareFixture(bridge, "answer");
    storedAnswer(bridge, {
      status: "error", failure_kind: "model_gone", evidence_refs: [], memory_update: undefined, content: "",
      execution_model: { profile_id: "openai/gpt-5.4", name: "GPT-5.4", provider: "openai", model: "gpt-5.4", thinking_level: "medium", run_options: {} },
    });
    const retry = vi.spyOn(bridge, "chatRetryLast");
    const send = vi.spyOn(bridge, "chatSend");
    render(<App bridge={bridge} />);
    expect(await screen.findByText("This model isn't offered anymore.", {}, { timeout: 5000 })).toBeVisible();
    // Configuration and the model catalog have loaded: Send is ready, yet nothing went out.
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Message Loupist" })).toBeEnabled());
    await new Promise((resolve) => setTimeout(resolve, 500));
    expect(retry).not.toHaveBeenCalled();
    expect(send).not.toHaveBeenCalled();
  });

  it("names the retired model under the answer the default model provided", async () => {
    const bridge = new PreviewBridge({ delay_ms: 0 });
    await prepareFixture(bridge, "answer");
    storedAnswer(bridge, {
      replaced_model: "GPT-5.4",
      execution_model: { profile_id: "openai/gpt-5.6-luna", name: "GPT-5.6 Luna", provider: "openai", model: "gpt-5.6-luna", thinking_level: "medium", run_options: {} },
    });
    const send = vi.spyOn(bridge, "chatSend");
    render(<App bridge={bridge} />);
    expect(await screen.findByText("GPT-5.4 isn't offered anymore, so this answer used GPT-5.6 Luna.", {}, { timeout: 5000 })).toBeVisible();
    expect(send).not.toHaveBeenCalled();
  });
});
