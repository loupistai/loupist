import { filingPassage } from "../../test/filingPassage";
import i18n from "../../i18n";
import { fireEvent, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { evidenceFixture } from "../../test/evidenceFixtures";
import { LivePageButton, WorkspaceEvidence } from "./WorkspaceEvidence";
import type { ChatEvidenceRef } from "../../types";

/** The reader with the live-page button from the panel status row beside it, as the panel draws them. */
const Reader = ({ evidence }: { evidence: ChatEvidenceRef }) => <><LivePageButton evidence={evidence} /><WorkspaceEvidence evidence={evidence} /></>;
import { renderWithOpen, destinations } from "../../test/open";

it("shows a saved source excerpt with its original URL and retrieval context", async () => {
  const evidence = evidenceFixture();
  const { open } = renderWithOpen(<Reader evidence={evidence} />);
  expect(screen.getByText(evidence.excerpt)).toBeVisible();
  expect(screen.getByText("FY2025 · USD millions")).toBeVisible();
  const live = screen.getByRole("button", { name: "Open live page" });
  // It opens in the panel's browser, so it carries the panel icon, not the system browser's.
  expect(live.querySelector("[data-open-icon]")).toHaveAttribute("data-open-icon", "panel");
  await userEvent.click(live);
  expect(destinations(open)).toEqual([{ kind: "link", url: evidence.url }]);
  fireEvent.click(live, { ctrlKey: true });
  expect(open).toHaveBeenLastCalledWith({ kind: "link", url: evidence.url }, expect.objectContaining({ external: true }));
});

it("says a retained source is gone and still offers the live page", () => {
  renderWithOpen(<Reader evidence={evidenceFixture({ excerpt: "" })} />);
  expect(screen.getByRole("alert")).toHaveTextContent("This source is no longer retained.");
  expect(screen.getByRole("button", { name: "Open live page" })).toBeVisible();
  expect(screen.queryByText("Saved excerpt")).toBeNull();
});

it("distinguishes discovery from reading and never executes source markup or unsafe URLs", () => {
  renderWithOpen(<WorkspaceEvidence evidence={evidenceFixture({ kind: "discovery", url: "https://user:secret@example.com", excerpt: '<script>alert(1)</script><button>Fake control</button>', excerpt_truncated: true, warnings: ["Snippet only"] })} />);
  expect(screen.getByText("Search result")).toBeVisible();
  expect(screen.getByRole("note")).toHaveTextContent("Check the original page");
  expect(screen.queryByRole("button")).toBeNull();
  expect(document.querySelector("script")).toBeNull();
});

it("separates the retained result from the smaller sample delivered to Loupist", async () => {
  renderWithOpen(<WorkspaceEvidence evidence={evidenceFixture({ kind: "saved_context", tool_name: "dataset_query", excerpt: "Saved result: 251 rows", model_delivery: { coverage: "bounded_sample", excerpt: "Delivered sample: 10 rows", excerpt_truncated: false, content_sha256: "b".repeat(64) } })} />);
  expect(screen.getByText("Selected data")).toBeVisible();
  expect(screen.getByText("Saved result: 251 rows")).toBeVisible();
  expect(screen.getByText(/Loupist received a smaller sample/)).toBeVisible();
  expect(screen.getByText("Delivered sample: 10 rows")).not.toBeVisible();
  await userEvent.click(screen.getByText("Sample received by Loupist"));
  expect(screen.getByText("Delivered sample: 10 rows")).toBeVisible();
});

it("opens at the host-selected passage and offers the jump back only after scrolling away", async () => {
  const scroll = vi.spyOn(HTMLElement.prototype, "scrollIntoView");
  renderWithOpen(<WorkspaceEvidence evidence={evidenceFixture({ excerpt: "Before\n111,482\nAfter", excerpt_match: { start: 7, end: 14 }, coverage: "UTF-8 bytes 132340–140532" })} />);
  const mark = document.querySelector("mark")!;
  expect(mark).toHaveTextContent("111,482");
  expect(scroll).toHaveBeenCalledWith({ block: "center" });
  expect(screen.queryByRole("button", { name: "Jump to cited passage" })).toBeNull();
  const reader = screen.getByRole("article");
  vi.spyOn(reader, "getBoundingClientRect").mockReturnValue({ top: 0, bottom: 400 } as DOMRect);
  vi.spyOn(mark, "getBoundingClientRect").mockReturnValue({ top: 900, bottom: 920 } as DOMRect);
  fireEvent.scroll(reader);
  scroll.mockClear();
  await userEvent.click(screen.getByRole("button", { name: "Jump to cited passage" }));
  expect(scroll).toHaveBeenCalledWith({ block: "center" });
  expect(screen.queryByText("UTF-8 bytes 132340–140532")).not.toBeInTheDocument();
});

it("reads a partial Memory search without routing metadata and clears it when switching sources", async () => {
  const { rerender } = renderWithOpen(<WorkspaceEvidence evidence={evidenceFixture({ kind: "saved_context", tool_name: "world_search", excerpt: '{"hits":[{"title":"Samsung","snippet":"Makes memory chips.","recordId":"private-record', excerpt_truncated: true })} />);
  expect(await screen.findByText(/Samsung\s+Makes memory chips\./)).toBeVisible();
  expect(document.body.textContent).not.toContain("private-record");
  rerender(<WorkspaceEvidence evidence={evidenceFixture({ id: "next-source", excerpt: "A different saved passage." })} />);
  expect(screen.getByText("A different saved passage.")).toBeVisible();
  expect(screen.queryByText(/Makes memory chips/)).toBeNull();
});

it("names the host only in the panel's status row, never again on the button or in the details", () => {
  const evidence = evidenceFixture();
  renderWithOpen(<Reader evidence={evidence} />);
  const host = new URL(evidence.url!).hostname;
  expect(screen.getByRole("button", { name: "Open live page" })).toHaveAttribute("title", evidence.url);
  expect(screen.getByRole("article").textContent).not.toContain(host);
});

it.each([
  ["en", "complete", "Full result"], ["ko", "complete", "전체 결과"],
  ["en", "untrusted_web", "Unverified web content"], ["ko", "untrusted_web", "검증되지 않은 웹 콘텐츠"],
  ["en", "future_coverage", "Unspecified"], ["ko", "future_coverage", "미지정"],
])("labels %s evidence coverage %s and keeps the saved excerpt verbatim", async (language, coverage, label) => {
  await i18n.changeLanguage(language);
  const excerpt = JSON.stringify({ coverage });
  const view = renderWithOpen(<WorkspaceEvidence evidence={evidenceFixture({ coverage, excerpt })} />);
  try {
    expect(screen.getByText(label, { selector: "dd" })).toBeVisible();
    expect(screen.getByText(excerpt, { selector: "pre" })).toBeVisible();
  } finally {
    view.unmount();
    await i18n.changeLanguage("en");
  }
});

it.each([
  ["en", "menus and navigation", "Check the original page"],
  ["ko", "페이지 메뉴와 탐색 링크", "원문을 확인하십시오"],
])("explains extraction warnings and unknown values in %s", async (locale, navigation, unknown) => {
  await i18n.changeLanguage(locale);
  const view = renderWithOpen(<Reader evidence={evidenceFixture({ warnings: ["navigation_heavy", "new_warning"] })} />);
  try {
    expect(screen.getByRole("note")).toHaveTextContent(navigation);
    expect(screen.getByRole("note")).toHaveTextContent(unknown);
    expect(screen.getByRole("note")).not.toHaveTextContent(/navigation_heavy|new_warning/);
  } finally {
    view.unmount();
    await i18n.changeLanguage("en");
  }
});

it("opens successive warnings on one source at their byte span, retained context or exact field", () => {
  const excerpt = "한울 reported 123 and 345.";
  const evidence = evidenceFixture({ title: "Filing", excerpt, excerpt_match: { start: 0, end: 6 } });
  const start = new TextEncoder().encode("한울 reported ").length;
  const check = { link_index: 1, text: "124", evidence_id: evidence.id, kind: "passage" as const, status: "not_found" as const, comparisonReason: "digits_differ" as const };
  const view = renderWithOpen(<WorkspaceEvidence evidence={evidence} step={{ label: "", comparison: { ...check, span: { start, end: start + 3 } } }} />);
  expect(view.container.querySelector("mark")).toHaveTextContent("123");
  view.rerender(<WorkspaceEvidence evidence={evidence} step={{ label: "", comparison: { ...check, context: { text: "Outside excerpt: 243", start: 17, end: 20 } } }} />);
  expect(view.container.querySelector("mark")).toHaveTextContent("243");
  const structured = { ...evidence, excerpt: JSON.stringify({ rows: [{ v: "first 123" }, { v: "second 123" }] }) };
  view.rerender(<WorkspaceEvidence evidence={structured} step={{ label: "", comparison: { ...check, pointer: "/rows/1/v", comparedValue: "123" } }} />);
  expect(view.container.querySelector("mark")?.parentElement).toHaveTextContent("second 123");
});

it("keeps the recorded field and printed precision when the saved JSON excerpt ends before the cell", () => {
  const evidence = evidenceFixture({ excerpt: '{"rows": [', excerpt_truncated: true });
  renderWithOpen(<WorkspaceEvidence evidence={evidence} step={{ label: "", comparison: { evidence_id: evidence.id,
    link_index: 0, kind: "data", status: "not_found", text: "30.1", comparisonReason: "digits_differ",
    pointer: "/rows/1/total", comparedValue: "30.0200" } }} />);
  const selected = screen.getByText("In the source").closest("section")!;
  expect(selected).toHaveTextContent("/rows/1/total");
  expect(selected.querySelector("mark")).toHaveTextContent("30.0200");
});

it("renders the retained filing table with its cited cell still marked", () => {
  const start = filingPassage.indexOf("$167,045");
  renderWithOpen(<WorkspaceEvidence evidence={evidenceFixture({ excerpt: filingPassage, excerpt_match: { start, end: start + 8 } })} />);
  expect(screen.getByRole("columnheader", { name: "2024" })).toBeVisible();
  const amount = screen.getByRole("cell", { name: "$167,045" });
  expect(amount.querySelector("mark")).toHaveTextContent("$167,045");
});

it("shows the original publisher update and a separate saved date in Evidence", () => {
  const evidence = evidenceFixture({ title: "Japan's population figures", kind: "saved_context", tool_name: "world_read", provider: "stale_runtime",
    retrieved_at: "2026-11-21T12:00:00Z", published_at: "2026-11-21",
    memory_origin: { route: "source", publisher: "world-bank.indicators", publication: { kind: "updated", time: { precision: "date", value: "2026-11-11" } }, saved_at: "2026-11-21T12:00:00Z" },
  });
  renderWithOpen(<WorkspaceEvidence evidence={evidence} />);
  expect(screen.getByText("World Bank Indicators")).toBeVisible();
  expect(screen.getByText("Updated").nextElementSibling).toHaveTextContent("Nov 11, 2026");
  expect(screen.getByText("from your Sources · saved Nov 21, 2026")).toBeVisible();
  expect(screen.queryByText("Published")).toBeNull();
  expect(screen.queryByText("Retrieved")).toBeNull();
});
