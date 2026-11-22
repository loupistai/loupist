import { act, fireEvent, render, screen } from "@testing-library/react";
import i18n from "../../i18n";
import { ChatConversation } from "./ChatConversation";
import { WorldProcessingStatus } from "./WorldProcessingStatus";
import { evidencePresentation } from "../../chat/evidence";
import type { ChatEvidenceRef, LoupistBridge } from "../../types";

afterEach(async () => {
  await act(async () => { await i18n.changeLanguage("en"); });
});

it("switches the Bot greeting language without changing the Loupist name", async () => {
  const suggest = vi.fn();
  const bridge = {
    knowledgeMap: async () => ({ nodes: [], edges: [] }),
    knowledgeGrowthEvents: async () => ({ events: [], nextCursor: null }),
    inboxList: async () => ({ items: [], nextCursor: null }),
    journalList: async () => [],
    watchlistList: async () => [],
    watchlistQuotes: async () => [],
    briefingSettingsGet: async () => ({ enabled: true, task: null, schedule: { kind: "weekly", days: [1], hour: 8, minute: 0, timezone: "UTC" } }),
  } as unknown as LoupistBridge;
  // The empty home Thread greets; the morning message is for a Thread the user has written in.
  render(<ChatConversation home bridge={bridge} messages={[]} messageKeys={[]} loupistId="loupist" loupistName="My research"
    onSuggestion={suggest} onReadAttachment={vi.fn()} />);
  expect(await screen.findByRole("button", { name: /What matters in markets this week/ })).toBeVisible();
  await act(async () => { await i18n.changeLanguage("en-XA"); });
  const greeting = document.querySelector(".bot-greeting")!;
  // The name stays as the user wrote it; the visible line under the greeting is the date only.
  expect(greeting.getAttribute("aria-label")).toContain("My research");
  expect(greeting.querySelector(".bot-greeting-top small")?.textContent).not.toContain("My research");
  const asks = [...greeting.querySelectorAll<HTMLButtonElement>(".bot-ask")];
  expect(asks).toHaveLength(1);
  for (const ask of asks) expect(ask.querySelector("strong")?.textContent).toMatch(/^⟦.*⟧$/);
  for (const bubble of greeting.querySelectorAll(".bot-bubble")) expect(bubble.textContent).toMatch(/^⟦.*⟧$/);
  fireEvent.click(asks.at(-1)!);
  expect(suggest).toHaveBeenCalledWith(expect.stringMatching(/^⟦.*⟧$/));
});

it("resolves status and evidence labels from the current language", async () => {
  render(<WorldProcessingStatus initialStatus="queued" />);
  expect(screen.getByRole("status")).toHaveTextContent("World: queued");
  const evidence = { kind: "original", tool_name: "web_fetch" } as ChatEvidenceRef;
  expect(evidencePresentation(evidence).label).toBe("Retrieved source");
  await act(async () => { await i18n.changeLanguage("en-XA"); });
  expect(screen.getByRole("status").textContent).toMatch(/^⟦.*⟧$/);
  expect(evidencePresentation(evidence).label).toMatch(/^⟦.*⟧$/);
});

it("distinguishes a Source allowance stop from a user pause in Korean", async () => {
  await act(async () => { await i18n.changeLanguage("ko"); });
  const view = render(<WorldProcessingStatus initialStatus="paused_budget" />);
  expect(screen.getByRole("status")).toHaveTextContent("월드: 이 자료의 월드 구성 멈춤");
  view.rerender(<WorldProcessingStatus initialStatus="paused" />);
  expect(screen.getByRole("status")).toHaveTextContent("월드: 구성 일시정지");
});
