import { render, screen } from "@testing-library/react";
import i18n from "@/i18n";
import type { ChatMessage } from "../../types";
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
