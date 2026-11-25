import { formatDateTime, formatDay } from "@/i18n/format";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi } from "vitest";
import type { ChatMessage, LoupistBridge } from "../../types";
import { ThreadCards } from "./ThreadCards";

function offer(): ChatMessage {
  return {
    id: "card-1",
    role: "assistant",
    status: "complete",
    host_composed: true,
    created_at: "2026-11-24T09:00:00.000Z",
    content: "Nothing checks this line.",
    offers: [{
      id: "offer-1",
      preset: "thesis-lines",
      assetId: "ast_0123456789abcdef",
      assetName: "NVIDIA",
      reason: "Nothing checks this line.",
      createdAtMs: 1,
      expiresAtMs: Date.now() + 86_400_000,
    }],
  } as unknown as ChatMessage;
}

it("turns an offer card on through the host", async () => {
  const user = userEvent.setup();
  const botOfferAccept = vi.fn().mockResolvedValue({ id: "task-1" });
  const bridge = { botOfferAccept, botOfferDismiss: vi.fn() } as unknown as LoupistBridge;
  render(<ThreadCards host message={offer()} bridge={bridge} loupistId="loupist-a" loupistName="Desk" />);
  expect(screen.getByRole("heading", { name: /NVIDIA/ })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Turn on" }));
  expect(botOfferAccept).toHaveBeenCalledExactlyOnceWith({ loupistId: "loupist-a", offerId: "offer-1" });
});

it("posts an ask choice once", async () => {
  const user = userEvent.setup();
  const botAskAnswer = vi.fn().mockResolvedValue(undefined);
  const bridge = { botAskAnswer } as unknown as LoupistBridge;
  const message = {
    id: "ask-1",
    role: "assistant",
    status: "complete",
    host_composed: true,
    created_at: "2026-11-24T09:00:00.000Z",
    content: "Which listing?",
    ask: {
      id: "ask-9",
      question: "Which listing?",
      options: [{ label: "The common stock" }, { label: "The ADR" }],
      recommended: 0,
      createdAtMs: 1,
      expiresAtMs: Date.now() + 86_400_000,
    },
  } as unknown as ChatMessage;
  render(<ThreadCards host message={message} bridge={bridge} loupistId="loupist-a" loupistName="Desk" />);
  await user.click(screen.getByRole("button", { name: "The ADR" }));
  expect(botAskAnswer).toHaveBeenCalledExactlyOnceWith({ loupistId: "loupist-a", askId: "ask-9", choice: 1 });
});


it("shows the results date and the host's run time before turning on a follow-up", () => {
  const message = offer();
  const runAtMs = Date.parse("2099-02-01T09:00:00+09:00");
  const resultsDate = "2099-01-31";
  Object.assign(message.offers![0]!, { preset: "results-follow-up", resultsDate, runAtMs });
  render(<ThreadCards host message={message} loupistId="loupist-a" loupistName="Desk" />);
  expect(screen.getByText(formatDay(resultsDate))).toBeVisible();
  expect(screen.getByText(`Next run ${formatDateTime(runAtMs)}`)).toBeVisible();
  expect(screen.getByRole("button", { name: "Turn on" })).toBeVisible();
});
