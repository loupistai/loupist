import { act, render, screen, waitFor, within } from "@testing-library/react";
import i18n from "../../i18n";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatMessage } from "../../types";
import { MessageMemoryRow } from "./MessageMemoryRow";
import { renderWithOpen } from "../../test/open";
import { libraryState } from "../map/libraryState";
import { FixMemoryProvider } from "../memory/fixMemory";
import { ChatSetAsideProvider, type ChatSetAside } from "../../chat/chatSetAside";
import type { GrowthEvent, KnowledgeMap } from "../../contracts/knowledge";
import type { MemorySaveSkipReason } from "../../types";
import { useChatMap } from "../workspace/ChatMapContext";

vi.mock("../workspace/ChatMapContext", () => ({ useChatMap: vi.fn(() => null) }));

const askAgain = ["Ask again in this conversation to read and keep them again.", "이 대화에서 다시 요청하면 자료를 다시 읽고 보관합니다."] as const;
// A failure with a recorded cause also says what to do; one from before causes were recorded keeps its old text.
const skipReasons: Array<[MemorySaveSkipReason, string, string, (readonly [string, string])?]> = [
  ["user", "you skipped saving.", "저장을 건너뛰었습니다."],
  ["deadline", "another conversation was still running after 10 minutes.", "10분이 지나도 다른 대화가 끝나지 않았습니다."],
  ["interrupted", "Loupist quit before saving finished.", "저장이 끝나기 전에 Loupist가 종료되었습니다."],
  ["failed", "saving failed.", "저장에 실패했습니다."],
  ["policy_changed", "Memory was reset while this answer was being written.", "답변을 작성하는 동안 메모리가 초기화되었습니다."],
  ["invalid", "Loupist refused something this answer tried to keep, so nothing from it was saved.", "이 답변이 보관하려던 항목을 Loupist가 거부하여 아무것도 저장되지 않았습니다.",
    ["Ask again in this conversation to read and keep them again. If it fails again, keep fewer items in one answer.", "이 대화에서 다시 요청하면 자료를 다시 읽고 보관합니다. 다시 실패하면 한 답변에서 보관할 항목을 줄이십시오."]],
  ["store_busy", "Loupist was busy saving other work and could not finish this save in time.", "다른 저장 작업으로 Loupist가 바빠 제시간에 저장을 마치지 못했습니다.", askAgain],
  ["storage", "Loupist could not write to this Loupist’s data.", "Loupist가 이 Loupist의 데이터에 쓰지 못했습니다.",
    ["Check that the disk has free space and the Loupist folder is available, then ask again in this conversation.", "디스크 여유 공간과 Loupist 폴더를 확인한 뒤 이 대화에서 다시 요청하십시오."]],
  ["conflict", "this conversation or Memory changed while the answer was being saved.", "저장하는 동안 이 대화나 메모리가 바뀌었습니다.", askAgain],
];

it.each(skipReasons.flatMap(([reason, en, ko, next]) => [false, true].map(staged => ({ reason, en, ko, next, staged }))))(
  "shows the terminal receipt for $reason with staged writes = $staged in both languages", async ({ reason, en, ko, next, staged }) => {
  const items = [
    { kind: "note", title: "Overlap check" }, { kind: "source", title: "Annual report" },
    { kind: "lens_page", title: "Fiscal calendar" }, { kind: "journal_entry", title: "Margin thesis" },
    { kind: "principle", title: "Read comparable periods" },
  ] as const;
  const message: ChatMessage = { id: "answer", role: "assistant", status: "complete", content: "The answer.", created_at: new Date().toISOString(),
    memory_update: { status: "no_change", commitId: null, changes: [], skipReason: reason, discardedMemoryItems: staged ? [...items] : undefined },
    number_checks: [{ text: "42", evidence_id: "ev_filing", link_index: 0, status: "not_found", kind: "passage" }],
  };
  const { unmount } = renderWithOpen(<MessageMemoryRow loupistId="loupist-a" message={message} />);
  try {
    const receipt = screen.getByTestId("memory-row");
    const assertReceipt = (status: string, correction: string, heading: string, kinds: string[], nextStep?: string) => {
      const line = within(receipt).getByText(status);
      const shownNext = receipt.querySelector("[data-save-next]");
      if (nextStep) expect(shownNext).toHaveTextContent(nextStep);
      else expect(shownNext).toBeNull();
      expect(line).toBeVisible();
      expect(within(receipt).getByRole("button")).toBeVisible();
      expect(within(receipt).queryByText(i18n.t("chat:chatReceipt.nothingNew"))).toBeNull();
      expect(receipt.querySelector('[data-receipt^="saved:"]')).toBeNull();
      if (staged) {
        expect(line.nextElementSibling).toHaveTextContent(correction);
        const list = within(receipt).getByRole("list", { name: heading });
        expect(within(list).getAllByRole("listitem").map(item => item.textContent)).toEqual(items.map((item, index) => `${kinds[index]} · ${item.title}`));
      } else {
        expect(within(receipt).queryByText(correction)).toBeNull();
        expect(within(receipt).queryByRole("list", { name: heading })).toBeNull();
      }
    };
    assertReceipt(`Not saved to Memory: ${en}`, "Anything this answer says it saved or filed was not kept.", "Not kept", ["Note", "Source", "Lens page", "Journal entry", "Principle"], next?.[0]);
    await act(async () => { await i18n.changeLanguage("ko"); });
    assertReceipt(`메모리에 저장하지 않음: ${ko}`, "이 답변에서 저장하거나 기록했다고 한 내용은 보관되지 않았습니다.", "보관되지 않음", ["메모", "출처", "렌즈 페이지", "저널 항목", "원칙"], next?.[1]);
  } finally {
    unmount();
    await i18n.changeLanguage("en");
  }
});

describe("the relation this answer added", () => {
  const node = (id: string, label: string, extra: Record<string, unknown> = {}) => ({ id, label, kind: "entity", entityKind: "Organization", degree: 1, watched: false, ...extra });
  const claim = (id: string, source: string, target: string, relation = "SUPPLIES") => ({ id, kind: "claim", source, target, relation });
  const added = (id: string): GrowthEvent => ({ eventId: id, loupistId: "loupist-a", atMs: 1, kind: "claim_added", subjectId: id, originChatId: "chat", originMessageId: "answer", recordId: "world", actor: "loupist", summary: "", beforeSummary: null, afterSummary: null });
  const message: ChatMessage = { id: "answer", role: "assistant", content: "Answer.", status: "complete", created_at: new Date().toISOString() };
  const show = (edges: ReturnType<typeof claim>[]) => {
    const map = { nodes: [node("nvda", "NVDA"), node("nvda-2", "nvda "), node("nvidia", "NVIDIA Corporation", { localNames: ["엔비디아"] }), node("nvidia-ko", "엔비디아"), node("tsmc", "TSMC")], edges, worldRevisionId: "rev" } as unknown as KnowledgeMap;
    vi.mocked(useChatMap).mockReturnValue({ loupistId: "loupist-a", chatId: "chat", map, events: edges.map(edge => added(edge.id)), select: vi.fn() } as unknown as ReturnType<typeof useChatMap>);
    return renderWithOpen(<MessageMemoryRow loupistId="loupist-a" chatId="chat" message={message} />);
  };
  afterEach(() => { vi.mocked(useChatMap).mockReturnValue(null); });

  it("skips an edge whose two ends are the same asset and shows the next one", () => {
    show([claim("self", "nvda", "nvda"), claim("same-label", "nvda", "nvda-2"), claim("same-name", "nvidia", "nvidia-ko"), claim("real", "tsmc", "nvda")]);
    const pills = document.querySelectorAll(".answer-map-preview");
    expect(pills).toHaveLength(1);
    expect(pills[0]).toHaveTextContent("TSMCsuppliesNVDA");
    // Named as one phrase, by what it opens.
    expect(screen.getByRole("button", { name: "Show in World: TSMC supplies NVDA" })).toBe(pills[0]);
  });

  it("names an open relation in the interface language", async () => {
    await act(async () => { await i18n.changeLanguage("ko"); });
    try {
      show([claim("real", "tsmc", "nvda", "PROVIDES_FUNDING_TO")]);
      expect(document.querySelector(".answer-map-preview")).toHaveTextContent("TSMC자금 제공NVDA");
    } finally {
      await act(async () => { await i18n.changeLanguage("en"); });
    }
  });

  it("shows no relation when every added edge joins an asset to itself", () => {
    show([claim("self", "nvda", "nvda"), claim("same-label", "nvda-2", "nvda"), claim("same-name", "nvidia-ko", "nvidia")]);
    expect(document.querySelector(".answer-map-preview")).toBeNull();
  });
});

it("localizes the World name inside the saved-change action", async () => {
  const change = { title: "Saved relationships", recordId: "world-1", operation: "create" as const, lesson: "", basis: "", futureUse: "" };
  const message: ChatMessage = { id: "world-answer", role: "assistant", content: "Saved.", status: "complete", created_at: new Date().toISOString(),
    memory_update: { status: "applied", commitId: "commit", changes: [
      { ...change, kind: "World", knowledge: { kind: "world", recordId: "world-1", revisionId: "rev", objectId: "world-1" } },
      { ...change, kind: "Journal", recordId: "journal-1", knowledge: { kind: "journal", recordId: "journal-1", revisionId: "rev", objectId: "journal-1" } },
    ] } };
  renderWithOpen(<MessageMemoryRow loupistId="loupist-a" message={message} />);
  try {
    await act(async () => { await i18n.changeLanguage("en-XA"); });
    expect(screen.getByRole("button", { name: i18n.t("chat:messageMemoryRow.openIn", { value: i18n.t("chat:workspaceEvidence.kind.world") }) })).toBeVisible();
  } finally {
    await act(async () => { await i18n.changeLanguage("en"); });
  }
});
it("marks a saved Source the user pasted as user-provided in the receipt", async () => {
  const source = (recordId: string, title: string, userProvided?: boolean) => ({ kind: "Source" as const, recordId, title, operation: "create" as const, lesson: "", basis: "", futureUse: "",
    knowledge: { kind: "source" as const, recordId: `rec-${recordId}`, revisionId: "rev", objectId: recordId }, ...(userProvided ? { userProvided } : {}) });
  const message: ChatMessage = { id: "source-answer", role: "assistant", content: "Saved.", status: "complete", created_at: new Date().toISOString(),
    memory_update: { status: "applied", commitId: "commit", changes: [source("src-pasted", "Solsup backlog", true), source("src-web", "Issuer note")] } };
  renderWithOpen(<MessageMemoryRow loupistId="loupist-a" message={message} />);
  await userEvent.setup().click(screen.getByRole("button", { name: "Saved 2 sources" }));
  const list = screen.getByRole("list", { name: "Saved in this answer" });
  const items = within(list).getAllByRole("listitem");
  expect(within(items[0]).getByTestId("memory-change-provided")).toHaveTextContent("User-provided");
  expect(within(items[1]).queryByTestId("memory-change-provided")).toBeNull();
});
it("shows the principle statement and supports View and Undo", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0].id;
  const principle = await bridge.principleCreate({ loupistId, statement: "Check the reporting period before comparing figures.", origin: "settings" });
  const message: ChatMessage = { id: "answer", role: "assistant", content: "Saved.", status: "complete", created_at: new Date().toISOString(), memory_update: { status: "applied", commitId: "commit", changes: [{ kind: "principle", statement: principle.statement, title: "Short title", recordId: principle.recordId, operation: "create", lesson: "", basis: "", futureUse: "", knowledge: { kind: "lens", recordId: principle.recordId, revisionId: principle.revisionId, objectId: principle.recordId } }] } };
  const archive = vi.spyOn(bridge, "principleArchive");
  const { open } = renderWithOpen(<MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={message} />);
  expect(screen.queryByText("Saved to your principles")).toBeNull();
  expect(screen.getByRole("button", { name: "Saved 1 new principle" })).toBeVisible();
  expect(screen.getByText(principle.statement)).toBeVisible();
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "View" }));
  expect(open).toHaveBeenCalledWith({ kind: "page", location: { loupistId, surface: "lens", recordId: principle.recordId } });
  archive.mockRejectedValueOnce(new Error("Offline"));
  await user.click(screen.getByRole("button", { name: "Undo" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Could not archive this principle.");
  await user.click(screen.getByRole("button", { name: "Undo" }));
  expect(await screen.findByText("Principle archived")).toBeVisible();
  expect(archive).toHaveBeenLastCalledWith({ loupistId, recordId: principle.recordId, archived: true });
});

it("opens a saved journal entry on the Journal tab", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0].id;
  const message: ChatMessage = { id: "answer", role: "assistant", content: "Saved.", status: "complete", created_at: new Date().toISOString(), memory_update: { status: "applied", commitId: "commit", changes: [{ kind: "Journal", title: "Hold the quality call", recordId: "journal-1", operation: "create", lesson: "", basis: "", futureUse: "", knowledge: { kind: "journal", recordId: "journal-1", revisionId: "rev", objectId: "journal-1" } }] } };
  const { open } = renderWithOpen(<MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={message} />);
  await userEvent.click(screen.getByRole("button", { name: "Saved 1 journal entry" }));
  expect(libraryState(bridge, loupistId)).toMatchObject({ open: true, tab: "journal" });
  expect(open).toHaveBeenCalledWith({ kind: "memory", location: expect.objectContaining({ loupistId, surface: "note" }), title: "Hold the quality call" });
});

it("says when nothing new was saved, and shows nothing for a private chat or a running answer", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0].id;
  const unchanged: ChatMessage = { id: "answer", role: "assistant", content: "Done.", status: "complete", created_at: new Date().toISOString(), memory_update: { status: "no_change", commitId: null, changes: [] } };
  const { rerender } = render(<MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={unchanged} />);
  expect(screen.getByRole("navigation", { name: "What this answer used and saved" })).toHaveTextContent("Nothing new saved");
  rerender(<MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={{ ...unchanged, memory_update: undefined }} />);
  expect(screen.queryByRole("navigation")).toBeNull();
  rerender(<MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={{ ...unchanged, status: "streaming" }} />);
  expect(screen.queryByRole("navigation")).toBeNull();
});

describe("what the answer learned", () => {
  const filed: ChatMessage = { id: "mock-filed-answer", role: "assistant", content: "Noted.", status: "complete", created_at: new Date().toISOString(), memory_update: { status: "applied", commitId: "commit", changes: [] } };

  it("says Learned 3 things · 1 idea for two filed pages and lists each section with Fix and Forget", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const loupistId = (await bridge.loupistList())[0].id;
    const fix = vi.fn();
    const user = userEvent.setup();
    const { open: openPage } = renderWithOpen(<FixMemoryProvider value={fix}><MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={filed} /></FixMemoryProvider>);
    const receipt = await screen.findByRole("navigation", { name: "What this answer used and saved" });
    await waitFor(() => expect(receipt).toHaveTextContent("Learned 3 things"));
    expect(receipt).toHaveTextContent("Learned 3 things·1 idea");
    await user.click(within(receipt).getByRole("button", { name: "Learned 3 things" }));
    const card = await screen.findByRole("group", { name: "Filed in this answer" });
    const pages = card.querySelectorAll("[data-learned-page]");
    expect([...pages].map(page => page.getAttribute("data-learned-page"))).toEqual(["lens-subject-tcp", "lens-idea-quic"]);
    const tcp = pages[0] as HTMLElement;
    await waitFor(() => expect(within(tcp).getByText("Overview")).toBeVisible());
    expect(within(tcp).getByText("Risks")).toBeVisible();
    await user.click(within(tcp).getByRole("button", { name: "Fix Overview" }));
    expect(fix).toHaveBeenCalledWith(expect.objectContaining({ kind: "lens", recordId: "lens-subject-tcp", label: "TCP · Overview" }));
    await user.click(within(tcp).getByRole("button", { name: "Forget Risks" }));
    expect(await within(tcp).findByText("Forgotten.")).toBeVisible();
    await user.click(within(tcp).getByRole("button", { name: "Undo" }));
    expect(await within(tcp).findByRole("button", { name: "Forget Risks" })).toBeVisible();
    await user.click(within(tcp).getByRole("button", { name: "TCP" }));
    // A learned page's title opens the Lens record the one way chat opens Lens: the Evidence reader.
    expect(openPage).toHaveBeenCalledWith({ kind: "lens", recordId: "lens-subject-tcp" });
  });

  it("counts a perspective the host delivered as a used page", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const loupistId = (await bridge.loupistList())[0].id;
    const message: ChatMessage = { ...filed, id: "answer-2", lens_receipts: [{ recordId: "lens-perspective", revisionId: "rev", kind: "perspective", title: "Evidence-led perspective", delivery: "context", revisionDigest: "digest" }] };
    render(<MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={message} />);
    const receipt = await screen.findByRole("navigation", { name: "What this answer used and saved" });
    expect(receipt).toHaveTextContent("Used 1 page");
    await userEvent.click(within(receipt).getByRole("button", { name: "Used 1 page" }));
    expect(await screen.findByText("Evidence-led perspective")).toBeVisible();
  });

  it("shows no Learned line in a private chat", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const loupistId = (await bridge.loupistList())[0].id;
    const read = vi.spyOn(bridge, "memoryLearned");
    const aside: ChatSetAside = { ids: new Set<string>(), saveOff: true, toggle: vi.fn() };
    render(<ChatSetAsideProvider value={aside}><MessageMemoryRow bridge={bridge} loupistId={loupistId} chatId="chat" message={filed} /></ChatSetAsideProvider>);
    await new Promise(resolve => setTimeout(resolve, 20));
    expect(screen.queryByText(/Learned/)).toBeNull();
    expect(read).not.toHaveBeenCalled();
  });
});

it("shows a partly kept save as kept, and names what it left out, why and what to do", async () => {
  const kept = { kind: "Source" as const, operation: "create" as const, title: "Annual report", recordId: "src-1", lesson: "", basis: "", futureUse: "",
    knowledge: { kind: "source" as const, recordId: "rec-src-1", revisionId: "rev", objectId: "src-1" } };
  const message: ChatMessage = { id: "answer", role: "assistant", content: "I saved the report and the principle.", status: "complete", created_at: new Date().toISOString(),
    memory_update: { status: "applied", commitId: "commit", changes: [kept],
      discardedMemoryItems: [{ kind: "principle", title: "Read comparable periods", reason: "invalid" }] } };
  const { unmount } = renderWithOpen(<MessageMemoryRow loupistId="loupist-a" message={message} />);
  try {
    const receipt = screen.getByTestId("memory-row");
    expect(within(receipt).getByRole("button", { name: "Saved 1 source" })).toBeVisible();
    const assertPartly = (status: string, claim: string, heading: string, item: string, next: string) => {
      expect(within(receipt).getByText(status)).toBeVisible();
      expect(within(receipt).getByText(claim)).toBeVisible();
      const list = within(receipt).getByRole("list", { name: heading });
      expect(within(list).getAllByRole("listitem").map(entry => entry.textContent)).toEqual([item]);
      expect(receipt.querySelector("[data-save-next]")).toHaveTextContent(next);
    };
    assertPartly("Partly saved to Memory: Loupist refused something this answer tried to keep and saved the rest.",
      "Anything listed below that this answer says it saved or filed was not kept.", "Not kept", "Principle · Read comparable periods",
      "Ask again in this conversation to read and keep them again. If it fails again, keep fewer items in one answer.");
    await act(async () => { await i18n.changeLanguage("ko"); });
    assertPartly("메모리에 일부만 저장함: 이 답변이 보관하려던 항목 일부를 Loupist가 거부하고 나머지를 저장했습니다.",
      "아래 목록에서 이 답변이 저장하거나 기록했다고 한 내용은 보관되지 않았습니다.", "보관되지 않음", "원칙 · Read comparable periods",
      "이 대화에서 다시 요청하면 자료를 다시 읽고 보관합니다. 다시 실패하면 한 답변에서 보관할 항목을 줄이십시오.");
  } finally {
    unmount();
    await i18n.changeLanguage("en");
  }
});
