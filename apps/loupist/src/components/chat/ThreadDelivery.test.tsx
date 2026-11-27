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

it("renders a met check's explanation as Markdown, with citations as their plain words and no link", async () => {
  const message: ChatMessage = {
    id: "met", role: "assistant", status: "complete", created_at: new Date().toISOString(),
    content: "The supply notice says the agreement [has ended](#evidence-ev_86c0aa12). **Shipments stop in March** [2](#evidence-ev_86c0bb34).\n\n- Notice dated 2026-11-20\n- Signed by both parties",
    delivery: {
      run_id: "run", task_id: "task", task_title: "Supplier contract check", task_kind: "custom", status: "succeeded",
      schedule: null, run_thread_id: "thread", finished_at_ms: Date.now(),
    },
  };
  render(<ThreadDelivery message={message} loupistName="Research" />);
  const explanation = document.querySelector<HTMLElement>("[data-delivery-explanation]")!;
  await waitFor(() => expect(explanation).toHaveTextContent("The supply notice says the agreement has ended."));
  await waitFor(() => expect(explanation.querySelector("ul")).not.toBeNull(), { timeout: 5000 });
  expect(within(explanation).getByText("Shipments stop in March")).toHaveAttribute("data-streamdown", "strong");
  expect(within(explanation).getAllByRole("listitem").map(item => item.textContent)).toEqual(["Notice dated 2026-11-20", "Signed by both parties"]);
  expect(explanation).not.toHaveTextContent(/#evidence|\*\*|\]\(/u);
  expect(explanation.querySelector("a, button")).toBeNull();
});
