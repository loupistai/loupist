import { act, fireEvent, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "@/i18n";
import type { ChatMessage, NumberCheck } from "../../types";
import { evidenceFixture } from "../../test/evidenceFixtures";
import { renderWithOpen } from "../../test/open";
import { TooltipProvider } from "../ui/tooltip";
import { ChatMessageCard } from "./ChatMessageCard";
import { NumberChip } from "./NumberChip";

it.each([
  ["en", "This value is not in the saved passage of this Source.", "The evidence for this figure is no longer stored.", "Open the source"],
  ["ko", "이 값은 이 출처의 저장된 구간에 없습니다.", "이 숫자의 근거는 더 이상 저장되어 있지 않습니다.", "자료 열기"],
])("distinguishes a saved passage from unavailable evidence in %s", async (language, retainedReason, missingReason, openSource) => {
  await act(() => i18n.changeLanguage(language));
  try {
    const evidence = evidenceFixture({ id: "ev_retained", title: "Issuer filing", tool_name: "web_fetch", excerpt: "Table of Contents" });
    const missing = evidenceFixture({ id: "ev_missing", title: "Old guidance", excerpt: "" });
    const title = "Issuer filing (Item 8 excerpt)";
    const retained: NumberCheck = { evidence_id: evidence.id, link_index: 0, kind: "passage", status: "not_found", text: "$42",
      comparisonReason: "no_candidate", comparisonReplay: { reason: "no_candidate", searched: { kind: "retained_passage", title } } };
    const unavailable: NumberCheck = { evidence_id: missing.id, link_index: 1, kind: "passage", status: "differs", text: "$37",
      comparisonReplay: { reason: "evidence_unavailable" } };
    const message: ChatMessage = { id: "answer", role: "assistant", status: "complete", created_at: "2026-11-21T09:00:00Z",
      content: "[$42](#evidence-ev_retained) and [$37](#evidence-ev_missing).", evidence_refs: [evidence], number_checks: [retained, unavailable] };
    const props = { message, loupistId: "loupist-a", chatId: "chat-a", loupistName: "Quality", onReadAttachment: vi.fn() };
    const user = userEvent.setup();
    const view = renderWithOpen(<TooltipProvider><ChatMessageCard {...props} /></TooltipProvider>);
    const mark = await screen.findByTestId("number-chip");
    expect(mark).toHaveAccessibleDescription(retainedReason);
    fireEvent.click(mark);
    const popover = await screen.findByRole("dialog", { name: title });
    expect(within(popover).getByText(retainedReason)).toBeVisible();
    expect(within(popover).getByRole("button", { name: openSource })).toBeEnabled();
    await user.keyboard("{Escape}");

    expect(screen.getByText("$37")).toHaveAttribute("title", missingReason);
    await user.click(screen.getByRole("button", { name: i18n.t("chat:chatReceipt.numbersToCheck", { count: 2, formattedCount: "2" }) }));
    const rows = within(await screen.findByRole("dialog")).getAllByRole("listitem");
    expect(rows[0]).toHaveTextContent(title);
    expect(rows[0]).toHaveTextContent(retainedReason);
    expect(rows[1]).toHaveTextContent(missingReason);
    expect(within(rows[1]!).queryByRole("button")).toBeNull();

    view.rerender(<NumberChip check={unavailable} evidence={missing} evidenceRefs={[missing]}>{unavailable.text}</NumberChip>);
    const unavailableMark = await screen.findByTestId("number-chip");
    expect(unavailableMark).toHaveAccessibleDescription(missingReason);
    fireEvent.click(unavailableMark);
    const unavailablePopover = await screen.findByRole("dialog");
    expect(within(unavailablePopover).getByText(missingReason)).toBeVisible();
    expect(within(unavailablePopover).queryByRole("button")).toBeNull();
  } finally {
    await act(() => i18n.changeLanguage("en"));
  }
});
