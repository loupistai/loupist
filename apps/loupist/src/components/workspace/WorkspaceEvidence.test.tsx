import { fireEvent, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { evidenceFixture } from "../../test/evidenceFixtures";
import { WorkspaceEvidence } from "./WorkspaceEvidence";
import { renderWithOpen } from "../../test/open";

it("shows a saved source excerpt with its original URL and retrieval context", async () => {
  const evidence = evidenceFixture();
  const { open } = renderWithOpen(<WorkspaceEvidence evidence={evidence} />);
  expect(screen.getByText(evidence.excerpt)).toBeVisible();
  expect(screen.getByText("FY2025 · USD millions")).toBeVisible();
  const live = screen.getByRole("button", { name: "Open live page" });
  // It opens in the panel's browser, so it carries the panel icon, not the system browser's.
  expect(live.querySelector("[data-open-icon]")).toHaveAttribute("data-open-icon", "panel");
  await userEvent.click(live);
  expect(open).toHaveBeenCalledExactlyOnceWith({ kind: "link", url: evidence.url }, undefined);
  fireEvent.click(live, { ctrlKey: true });
  expect(open).toHaveBeenLastCalledWith({ kind: "link", url: evidence.url }, { external: true });
});

it("says a retained source is gone and still offers the live page", () => {
  renderWithOpen(<WorkspaceEvidence evidence={evidenceFixture({ excerpt: "" })} />);
  expect(screen.getByRole("alert")).toHaveTextContent("This source is no longer retained.");
  expect(screen.getByRole("button", { name: "Open live page" })).toBeVisible();
  expect(screen.queryByText("Saved excerpt")).toBeNull();
});

it("distinguishes discovery from reading and never executes source markup or unsafe URLs", () => {
  renderWithOpen(<WorkspaceEvidence evidence={evidenceFixture({ kind: "discovery", url: "https://user:secret@example.com", excerpt: '<script>alert(1)</script><button>Fake control</button>', excerpt_truncated: true, warnings: ["Snippet only"] })} />);
  expect(screen.getByText("Search result")).toBeVisible();
  expect(screen.getByText("Snippet only")).toBeVisible();
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
  renderWithOpen(<WorkspaceEvidence evidence={evidence} />);
  const host = new URL(evidence.url!).hostname;
  expect(screen.getByRole("button", { name: "Open live page" })).toHaveAttribute("title", evidence.url);
  expect(screen.getByRole("article").textContent).not.toContain(host);
});
