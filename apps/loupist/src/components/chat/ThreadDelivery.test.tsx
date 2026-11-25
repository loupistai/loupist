import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "@/i18n";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatMessage } from "../../types";
import { ChatExecutionProvider } from "./ChatExecutionContext";
import { ThreadDelivery } from "./ThreadDelivery";

it.each([
  ["en", "Not used: FRED, KRX (macOS permission needed for the saved key)"],
  ["ko", "사용하지 않음: FRED, KRX (저장된 키에 macOS 권한 필요)"],
])("names saved keys omitted from a delivered scheduled result in %s", async (language, label) => {
  await i18n.changeLanguage(language);
  try {
    const message: ChatMessage = {
      id: "delivery", role: "assistant", status: "complete", created_at: new Date().toISOString(),
      content: "Reported on the available data.",
      delivery: {
        run_id: "run", task_id: "task", task_title: "Briefings", task_kind: "briefing", status: "succeeded",
        schedule: null, run_thread_id: "thread", finished_at_ms: Date.now(), skipped_saved_keys: ["FRED", "KRX"],
      },
    };
    render(<ThreadDelivery message={message} loupistName="Research" />);
    expect(screen.getByText(label)).toBeVisible();
  } finally {
    await i18n.changeLanguage("en");
  }
});

it("keeps evidence visible when undoing a saved script from the first result", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = "loupist-quality";
  const { summary } = await bridge.scriptSave({
    loupistId, draft: { name: "check-filings", summary: "Read recent filings", code: "print('fixture')" },
  });
  const script = { name: summary.name, version: summary.version };
  const remove = vi.spyOn(bridge, "scriptDelete");
  const message: ChatMessage = {
    id: "result", role: "assistant", status: "complete", created_at: new Date().toISOString(),
    content: "Reported on the available data.",
    delivery: {
      run_id: "run", task_id: "task", task_title: "Filings and results", task_kind: "custom", status: "succeeded",
      schedule: null, run_thread_id: "thread", finished_at_ms: Date.now(),
      firstRun: true, evidence: { figuresChecked: 3, figuresUnmatched: 1, sources: 2 },
      learned: { messageId: "result", pages: [], scripts: [script] },
    },
  };
  render(<ChatExecutionProvider value={{ bridge, loupistId, threadId: "bot", surface: "bot", theme: "light", active: true }}>
    <ThreadDelivery message={message} loupistName="Research" />
  </ChatExecutionProvider>);
  expect(screen.getByText("First check")).toBeVisible();
  expect(screen.getByText("3 figures checked · 2 sources")).toBeVisible();
  expect(screen.getByText(/1 figure didn’t match/)).toBeVisible();
  const scripts = screen.getByRole("region", { name: "Saved scripts" });
  expect(within(scripts).getByText(`${script.name}@${script.version}`)).toBeVisible();
  await userEvent.setup().click(within(scripts).getByRole("button", { name: "Undo" }));
  await waitFor(() => expect(scripts).not.toBeInTheDocument());
  expect(remove).toHaveBeenCalledWith({ loupistId, ...script });
  expect(screen.getByText("3 figures checked · 2 sources")).toBeVisible();
});
