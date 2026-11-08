import { render as renderUi, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactElement, ReactNode } from "react";
import { ChatRail, type ChatRailActions } from "./ChatRail";
import { ChatLightboxProvider } from "./ChatLightbox";
import type { RailData } from "../../chat/chatRail";
import type { ChatMessage } from "../../types";

// The rail's thumbnails open the chat's lightbox, which reads each image once.
const sent: ChatMessage[] = [{ id: "m1", role: "user", content: "Look at this deck", created_at: "2026-10-28T00:00:00Z", status: "complete",
  attachments: [{ id: "a1", filename: "deck.png", media_type: "image/png", size_bytes: 2048 }] }];
const Lightbox = ({ children }: { children: ReactNode }) =>
  <ChatLightboxProvider messages={sent} onRead={async () => "data:image/png;base64,AAAA"}>{children}</ChatLightboxProvider>;
const render = (ui: ReactElement) => renderUi(ui, { wrapper: Lightbox });

const data: RailData = {
  files: [
    { key: "img", kind: "image", filename: "deck.png", sizeBytes: 2048, messageId: "m1", attachmentId: "a1", fromLoupist: false },
    { key: "memo", kind: "file", filename: "memo.md", path: "memo.md", fromLoupist: true },
  ],
  using: [{ key: "principle:p1", kind: "principle", label: "Check operating cash flow", recordId: "p1" }],
  learningPending: false,
  learned: { names: 2, relationships: 1, methods: 0, chips: [{ id: "hanul", label: "Hanul Precision", swatch: "company" }], highlightIds: new Set(["hanul", "morrow"]) },
  needs: [],
  waiting: [],
  questions: [],
  hasCards: true,
};

function actions(extra: Partial<ChatRailActions> = {}): ChatRailActions {
  return { loupistId: "loupist", onOpenFile: vi.fn(), onOpenUsing: vi.fn(), onShowLearned: vi.fn(), ...extra };
}

beforeEach(() => localStorage.clear());

it("stacks the cards with files open and the rest folded, and remembers a fold per Loupist", async () => {
  const user = userEvent.setup();
  const handlers = actions();
  const { unmount } = render(<ChatRail mode="rail" data={data} actions={handlers} />);
  const rail = screen.getByRole("complementary", { name: "In this chat" });
  const files = within(rail).getByRole("button", { name: /Files & images/ });
  expect(files).toHaveAttribute("aria-expanded", "true");
  expect(within(rail).getByRole("button", { name: /^Using/ })).toHaveAttribute("aria-expanded", "false");
  expect(await within(rail).findByRole("button", { name: "deck.png" })).toBeEnabled();
  // The rail's thumbnail opens the chat's lightbox, not a viewer of its own.
  await user.click(within(rail).getByRole("button", { name: "deck.png" }));
  expect(await screen.findByRole("dialog", { name: "Image viewer" })).toHaveTextContent("deck.png");
  await user.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Image viewer" })).toBeNull());
  await user.click(within(rail).getByRole("button", { name: /memo\.md/ }));
  expect(handlers.onOpenFile).toHaveBeenCalledWith("memo.md");

  await user.click(within(rail).getByRole("button", { name: /^Learned/ }));
  await user.click(within(rail).getByRole("button", { name: "Hanul Precision" }));
  expect(handlers.onShowLearned).toHaveBeenCalledWith(new Set(["hanul"]));
  await user.click(within(rail).getByRole("button", { name: "Show on the Map" }));
  expect(handlers.onShowLearned).toHaveBeenLastCalledWith(data.learned!.highlightIds);
  await user.click(files);
  unmount();
  render(<ChatRail mode="rail" data={data} actions={actions()} />);
  expect(screen.getByRole("button", { name: /Files & images/ })).toHaveAttribute("aria-expanded", "false");
  expect(screen.getByRole("button", { name: /^Learned/ })).toHaveAttribute("aria-expanded", "true");
});

it("leaves out empty cards and renders nothing when there is nothing to show", () => {
  const { container, rerender } = render(<ChatRail mode="rail" data={{ ...data, files: [], learned: null }} actions={actions()} />);
  expect(screen.queryByRole("button", { name: /Files & images/ })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: /^Using/ })).toBeInTheDocument();
  rerender(<ChatRail mode="none" data={data} actions={actions()} />);
  expect(container).toBeEmptyDOMElement();
});

it("keeps the Learned status visible while folded and lets the reader inspect additions as they arrive", async () => {
  const handlers = actions();
  const { rerender } = render(<ChatRail mode="rail" data={{ ...data, learned: null, learningPending: true }} actions={handlers} />);
  expect(screen.getByRole("status")).toHaveTextContent("Reviewing sources…");
  expect(screen.queryByRole("button", { name: /^Learned/ })).toBeNull();
  rerender(<ChatRail mode="rail" data={{ ...data, learningPending: true }} actions={handlers} />);
  const header = screen.getByRole("button", { name: /^Learned.*Reviewing sources/ });
  expect(header).toHaveAttribute("aria-expanded", "false");
  await userEvent.click(header);
  expect(screen.getByRole("button", { name: "Hanul Precision" })).toBeVisible();
  rerender(<ChatRail mode="rail" data={data} actions={handlers} />);
  expect(screen.queryByText("Reviewing sources…")).toBeNull();
  expect(header).toHaveAccessibleName(/Learned.*\+2 on the map/);
  expect(header).toHaveAttribute("aria-expanded", "true");
});

it("keeps the same cards in flow on narrow chats and flags what needs the user", () => {
  const needs = { ...data, learningPending: true, needs: [{ connection_id: "c1", connection_name: "Morrow", tool_name: "read_filings", reason: "needs_approval" as const }] };
  render(<ChatRail mode="inline" data={needs} actions={actions()} />);
  const rail = screen.getByRole("complementary", { name: "In this chat" });
  expect(rail).toHaveAttribute("data-placement", "inline");
  expect(within(rail).getByRole("button", { name: /Files & images/ })).toBeInTheDocument();
  expect(within(rail).getByText("Reviewing sources…")).toBeVisible();
  expect(within(rail).getByRole("button", { name: /^Needs your OK/ })).toBeVisible();
  expect(rail.firstElementChild).toHaveAttribute("data-card", "needs");
  expect(rail.children[1]).toHaveAttribute("data-card", "learned");
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("flags a waiting approval in compact context, the same as a refusal that needs Settings", () => {
  const waiting = { ...data, waiting: [{ requestId: "r1", connectionId: "c1", connectionName: "Morrow", toolName: "read_filings", state: "waiting" as const, requestedAtMs: 0 }] };
  render(<ChatRail mode="inline" data={waiting} actions={actions()} />);
  expect(screen.getByRole("button", { name: /^Needs your OK/ })).toHaveTextContent("Read filings");
});

it("counts a waiting question in the same row and leads to its card", async () => {
  const user = userEvent.setup();
  const question = { requestId: "q1", question: "Compare on which period?", options: [{ label: "Last 4 quarters" }, { label: "Last 3 years" }], state: "waiting" as const, requestedAtMs: 0 };
  const card = document.createElement("section");
  card.className = "question-card";
  card.dataset.requestId = "q1";
  card.tabIndex = -1;
  Object.defineProperty(card, "scrollIntoView", { value: vi.fn(), configurable: true });
  document.body.append(card);
  try {
    const { rerender } = render(<ChatRail mode="rail" data={{ ...data, questions: [question] }} actions={actions()} />);
    const row = screen.getByRole("button", { name: /^Needs your OK/ });
    expect(row).toHaveTextContent("Compare on which period?");
    await user.click(row);
    expect(card).toHaveFocus();
    const waiting = { requestId: "r1", connectionId: "c1", connectionName: "Morrow", toolName: "read_filings", state: "waiting" as const, requestedAtMs: 0 };
    rerender(<ChatRail mode="rail" data={{ ...data, waiting: [waiting], questions: [question] }} actions={actions()} />);
    expect(screen.getByRole("button", { name: /^Needs your OK/ })).toHaveTextContent("2 requests");
  } finally {
    card.remove();
  }
});

it("names a waiting approval in one row with no buttons of its own, and leads to the card in the answer", async () => {
  const user = userEvent.setup();
  const waiting = { ...data, waiting: [{ requestId: "r1", connectionId: "c1", connectionName: "Morrow", toolName: "read_company_filings", state: "waiting" as const, requestedAtMs: 0 }] };
  // The card in the answer, where the buttons are.
  const answer = document.createElement("section");
  answer.className = "approval-card";
  answer.dataset.requestId = "r1";
  answer.tabIndex = -1;
  const scroll = vi.fn();
  Object.defineProperty(answer, "scrollIntoView", { value: scroll, configurable: true });
  document.body.append(answer);
  try {
    render(<ChatRail mode="rail" data={waiting} actions={actions()} />);
    const rail = screen.getByRole("complementary", { name: "In this chat" });
    expect(within(rail).queryByRole("button", { name: "Allow once" })).toBeNull();
    const row = within(rail).getByRole("button", { name: /^Needs your OK/ });
    expect(row).toHaveTextContent("Needs your OKRead company filingsShow");
    await user.click(row);
    expect(scroll).toHaveBeenCalledWith(expect.objectContaining({ block: "center" }));
    expect(answer).toHaveFocus();
  } finally {
    answer.remove();
  }
});

it("leads from compact context to the newest waiting card without opening an overlay", async () => {
  const user = userEvent.setup();
  const waiting = { ...data, waiting: [
    { requestId: "r1", connectionId: "c1", connectionName: "Morrow", toolName: "read_company_filings", state: "waiting" as const, requestedAtMs: 0 },
    { requestId: "r2", kind: "web_site" as const, connectionId: "web", connectionName: "Web", toolName: "web_fetch", target: "news.example/q3", state: "waiting" as const, requestedAtMs: 0 },
  ] };
  const cards = ["r1", "r2"].map((id) => {
    const card = document.createElement("section");
    card.className = "approval-card";
    card.dataset.requestId = id;
    card.tabIndex = -1;
    Object.defineProperty(card, "scrollIntoView", { value: vi.fn(), configurable: true });
    document.body.append(card);
    return card;
  });
  try {
    render(<ChatRail mode="inline" data={waiting} actions={actions()} />);
    const rail = screen.getByRole("complementary", { name: "In this chat" });
    const row = within(rail).getByRole("button", { name: /^Needs your OK/ });
    expect(row).toHaveTextContent("2 requests");
    await user.click(row);
    expect(screen.queryByRole("dialog", { name: "In this chat" })).toBeNull();
    // The newest request is the one in view.
    expect(cards[1]).toHaveFocus();
  } finally {
    for (const card of cards) card.remove();
  }
});
