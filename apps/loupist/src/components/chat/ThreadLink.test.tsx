import { act, render, screen } from "@testing-library/react";
import { renderWithOpen } from "../../test/open";
import userEvent from "@testing-library/user-event";
import i18n from "../../i18n";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { commandError } from "../../bridge/mock/commandError";
import type { ChatMessage, LoupistBridge, ResearchHandoffChoices } from "../../types";
import { ThreadLink } from "./ThreadLink";

afterEach(async () => { await i18n.changeLanguage("en"); });

function post(): ChatMessage {
  return {
    id: "link-1",
    role: "assistant",
    content: "New link: Alpha ↔ Gamma through Beta.",
    created_at: "2026-10-30T09:00:00.000Z",
    status: "complete",
    link: {
      connectionId: "con_1",
      seedName: "Alpha",
      farName: "Gamma",
      path: [{ name: "Beta", relation: "SUPPLIES" }, { name: "Gamma", relation: "USES", reversed: true }],
      nodeIds: ["a", "b", "c"],
    },
  } as unknown as ChatMessage;
}

const receipt = { chatId: "chat-9", handoffId: "handoff-9" };

it("renders the path with its relations and opens World on it", async () => {
  const { open: onOpenMap } = renderWithOpen(<ThreadLink message={post()} loupistId="loupist-a" loupistName="Chip desk" />);
  expect(screen.getByText("New link: Alpha ↔ Gamma through Beta.")).toBeVisible();
  // One quiet line: the button is named by its words, and the path is named on hover, not drawn as a card.
  const path = screen.getByRole("button", { name: "Show in World" });
  expect(path).toHaveAttribute("title", "Alpha → supplies → Beta ← uses ← Gamma");
  await userEvent.setup().click(path);
  expect(onOpenMap).toHaveBeenCalledWith({ kind: "page", location: { loupistId: "loupist-a", surface: "world", view: { highlightEntityIds: ["a", "b", "c"] } } });
});

it("names the path's relations in the interface language, and an unlisted one in plain English", async () => {
  await act(async () => { await i18n.changeLanguage("ko"); });
  const message = post();
  message.link!.path = [{ name: "Beta", relation: "LISTS_ON" }, { name: "Gamma", relation: "RAISED_PRICES_FOR", reversed: true }];
  renderWithOpen(<ThreadLink message={message} loupistId="loupist-a" loupistName="Chip desk" />);
  expect(screen.getByTitle("Alpha → 상장 거래소 → Beta ← raised prices for ← Gamma")).toBeVisible();
});

it("sends only the link and the composer's choices at the click, and leaves the opened mark to the host", async () => {
  const researchHandoffStart = vi.fn(async () => receipt);
  const knowledgeConnectionOpen = vi.fn(async () => ({}));
  const bridge = { researchHandoffStart, knowledgeConnectionOpen } as unknown as LoupistBridge;
  let choices: ResearchHandoffChoices = { modelProfileId: "model-a", thinkingLevel: "low", runOptions: {}, useMemory: true, updateMemory: true };
  render(<ThreadLink message={post()} bridge={bridge} loupistId="loupist-a" loupistName="Chip desk" composerChoices={() => choices} />);
  // The composer changed after the card drew: the click reads it then, false kept as false.
  choices = { modelProfileId: "model-b", thinkingLevel: "high", runOptions: { verbosity: "low" }, useMemory: false, updateMemory: false };
  await userEvent.setup().click(screen.getByRole("button", { name: "Look into it" }));
  expect(researchHandoffStart).toHaveBeenCalledExactlyOnceWith({
    loupistId: "loupist-a", connectionId: "con_1",
    modelProfileId: "model-b", thinkingLevel: "high", runOptions: { verbosity: "low" }, useMemory: false, updateMemory: false,
  });
  expect(knowledgeConnectionOpen).not.toHaveBeenCalled();
  const status = await screen.findByRole("status");
  expect(status).toHaveTextContent("Researching it. The result will arrive here.");
  // The pressed button went away: focus moves to what happened, not to the page.
  expect(status).toHaveFocus();
  expect(screen.queryByRole("button", { name: "Look into it" })).toBeNull();
});

it("leaves absent choices absent and starts once however fast it is pressed", async () => {
  let resolve!: (value: typeof receipt) => void;
  const researchHandoffStart = vi.fn(() => new Promise<typeof receipt>((done) => { resolve = done; }));
  const bridge = { researchHandoffStart } as unknown as LoupistBridge;
  render(<ThreadLink message={post()} bridge={bridge} loupistId="loupist-a" loupistName="Chip desk" composerChoices={() => ({ useMemory: true, updateMemory: true })} />);
  const button = screen.getByRole("button", { name: "Look into it" });
  act(() => { button.click(); button.click(); button.click(); });
  expect(button).toHaveAttribute("aria-disabled", "true");
  expect(button).toHaveAttribute("aria-busy", "true");
  await act(async () => resolve(receipt));
  expect(researchHandoffStart).toHaveBeenCalledExactlyOnceWith({ loupistId: "loupist-a", connectionId: "con_1", useMemory: true, updateMemory: true });
  expect(await screen.findByRole("status")).toHaveTextContent("Researching it.");
});

it("starts from the keyboard", async () => {
  const researchHandoffStart = vi.fn(async () => receipt);
  render(<ThreadLink message={post()} bridge={{ researchHandoffStart } as unknown as LoupistBridge} loupistId="loupist-a" loupistName="Chip desk" />);
  const user = userEvent.setup();
  await user.tab();
  expect(screen.getByRole("button", { name: "Look into it" })).toHaveFocus();
  await user.keyboard("{Enter}");
  expect(researchHandoffStart).toHaveBeenCalledOnce();
  expect(await screen.findByRole("status")).toHaveFocus();
});

it("keeps the keyboard on Look into it while the host starts, and after a start it refused", async () => {
  let refuse!: (cause: unknown) => void;
  const researchHandoffStart = vi.fn(() => new Promise<typeof receipt>((_, reject) => { refuse = reject; }));
  render(<ThreadLink message={post()} bridge={{ researchHandoffStart } as unknown as LoupistBridge} loupistId="loupist-a" loupistName="Chip desk" />);
  const user = userEvent.setup();
  await user.tab();
  const button = screen.getByRole("button", { name: "Look into it" });
  await user.keyboard("{Enter}");
  expect(button).toHaveFocus();
  await user.keyboard("{Enter}");
  expect(researchHandoffStart).toHaveBeenCalledOnce();
  await act(async () => refuse(commandError("provider_unavailable", "The provider is unavailable.")));
  expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't start the research.");
  expect(button).toHaveFocus();
  expect(button).not.toHaveAttribute("aria-disabled");
});

it("says a link the host no longer holds is gone, without marking it opened", async () => {
  // The mock holds no such link in this Loupist's Bot: the host's `not_found`.
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  renderWithOpen(<ThreadLink message={post()} bridge={bridge} loupistId="loupist-quality" loupistName="Chip desk" />);
  await userEvent.setup().click(screen.getByRole("button", { name: "Look into it" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("This link is no longer available. Ask about it in a message instead.");
  expect(screen.queryByText(/Researching it/)).toBeNull();
  expect(screen.queryByRole("button", { name: "Look into it" })).toBeNull();
  expect(screen.getByRole("button", { name: "Show in World" })).toBeVisible();
});

it("keeps the link to try again when the host could not start, and says a repeat with other settings already runs", async () => {
  const researchHandoffStart = vi.fn()
    .mockRejectedValueOnce(commandError("provider_unavailable", "The provider is unavailable."))
    .mockRejectedValueOnce(commandError("conflict", "Already opened with other settings."));
  render(<ThreadLink message={post()} bridge={{ researchHandoffStart } as unknown as LoupistBridge} loupistId="loupist-a" loupistName="Chip desk" />);
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "Look into it" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't start the research. Check the model in the composer, then try again.");
  expect(screen.queryByRole("status")).toBeNull();
  await user.click(screen.getByRole("button", { name: "Look into it" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("This link is already being researched with other settings.");
  expect(screen.getByRole("button", { name: "Look into it" })).toBeEnabled();
  expect(researchHandoffStart).toHaveBeenCalledTimes(2);
});

it.each([
  ["done", "Research complete."],
  ["failed", "The research couldn't finish."],
  ["stopped", "The research stopped."],
] as const)("shows %s and lets the same card start another run", async (status, text) => {
  const researchHandoffStart = vi.fn()
    .mockResolvedValueOnce(receipt)
    .mockResolvedValueOnce({ chatId: "chat-10", handoffId: "handoff-10" });
  const props = {
    message: post(), bridge: { researchHandoffStart } as unknown as LoupistBridge,
    loupistId: "loupist-a", loupistName: "Chip desk",
  };
  const view = render(<ThreadLink {...props} />);
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "Look into it" }));
  expect(screen.getByRole("status")).toHaveTextContent("Researching it.");
  view.rerender(<ThreadLink {...props} research={{ ...receipt, title: "Link: Alpha and Gamma", status, connectionId: "con_1" }} />);
  expect(screen.getByRole("status")).toHaveTextContent(text);
  await user.click(screen.getByRole("button", { name: "Look into it again" }));
  expect(researchHandoffStart).toHaveBeenCalledTimes(2);
  expect(screen.getByRole("status")).toHaveTextContent("Researching it.");
});

it("dismisses with Not useful and keeps the card when that fails", async () => {
  const knowledgeConnectionDismiss = vi.fn(async () => { throw new Error("offline"); });
  const bridge = { knowledgeConnectionDismiss } as unknown as LoupistBridge;
  render(<ThreadLink message={post()} bridge={bridge} loupistId="loupist-a" loupistName="Chip desk" />);
  await userEvent.setup().click(screen.getByRole("button", { name: "Not useful" }));
  expect(knowledgeConnectionDismiss).toHaveBeenCalledWith({ loupistId: "loupist-a", connectionId: "con_1" });
  expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't do that.");
  expect(screen.getByRole("button", { name: "Not useful" })).toBeEnabled();
});

it("reads in Korean, its failures too", async () => {
  await act(async () => { await i18n.changeLanguage("ko"); });
  const researchHandoffStart = vi.fn()
    .mockRejectedValueOnce(commandError("invalid_request", "Unknown model."))
    .mockRejectedValueOnce(commandError("not_found", "Gone."));
  render(<ThreadLink message={post()} bridge={{ researchHandoffStart } as unknown as LoupistBridge} loupistId="loupist-a" loupistName="칩 데스크" />);
  expect(screen.getByText("새 연결: Alpha ↔ Gamma, Beta를 거칩니다.")).toBeVisible();
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "살펴보기" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("조사를 시작하지 못했습니다. 입력창의 모델을 확인한 뒤 다시 시도하십시오.");
  await user.click(screen.getByRole("button", { name: "살펴보기" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("이 연결은 더 이상 열 수 없습니다. 메시지로 직접 물어보십시오.");
});
