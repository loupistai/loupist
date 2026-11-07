import type { ComponentProps } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MessageResponse } from "./message-response";
import { OpenProvider } from "../navigation/Open";
import { TurnProvider } from "../chat/TurnProvider";
import { openSpy, renderWithOpen } from "../../test/open";
import { evidenceFixture } from "../../test/evidenceFixtures";
import type { Open } from "../../chat/navigation";
import type { ChatEvidenceRef, ChatMessage } from "../../types";

describe("MessageResponse links", () => {
  it("opens Lens links through whichever opener is current, and leaves them plain outside one", async () => {
    const user = userEvent.setup();
    const text = "[Valuation](lens:rec_0123456789abcdef0123456789abcdef)";
    const view = render(<MessageResponse mode="static">{text}</MessageResponse>);
    expect(screen.queryByRole("link")).toBeNull();
    const first = openSpy();
    view.rerender(<OpenProvider open={first}><MessageResponse mode="static">{text}</MessageResponse></OpenProvider>);
    await user.click(screen.getByRole("link", { name: "Valuation" }));
    expect(first).toHaveBeenCalledExactlyOnceWith({ kind: "lens", recordId: "rec_0123456789abcdef0123456789abcdef" }, undefined);

    const nextLoupist = openSpy();
    view.rerender(<OpenProvider open={nextLoupist}><MessageResponse mode="static">{text}</MessageResponse></OpenProvider>);
    await user.click(screen.getByRole("link", { name: "Valuation" }));
    expect(nextLoupist).toHaveBeenCalledExactlyOnceWith({ kind: "lens", recordId: "rec_0123456789abcdef0123456789abcdef" }, undefined);
  });

  it("does not navigate malformed Lens destinations, images or literal examples", () => {
    renderWithOpen(<MessageResponse mode="static">{[
      "[Path](lens:..%2Fprivate)", "[Fragment](lens:method%23secret)",
      "[Blank](lens:)", "[Control](lens:method%00)", "[Encoded alias](lens:%72ec_0123456789abcdef0123456789abcdef)",
      "![Image](lens:method)", "`[Example](lens:method)`",
    ].join("\n\n")}</MessageResponse>);
    expect(screen.queryByRole("link")).toBeNull();
    expect(document.querySelector('img[src^="lens:"]')).toBeNull();
    expect(screen.getByText("[Example](lens:method)")).toBeVisible();
  });

  it("opens credential-free web links through the host opener and announces a failed open", async () => {
    const user = userEvent.setup();
    const openLink = vi.fn<Open>().mockRejectedValue(new Error("open failed"));
    const windowOpen = vi.spyOn(window, "open").mockImplementation(() => null);
    const url = "https://dart.fss.or.kr/dsab001/search.ax?textCrpNm=005930";
    renderWithOpen(<MessageResponse mode="static">{`[DART company search](${url})`}</MessageResponse>, openLink);
    await user.click(screen.getByRole("link", { name: "DART company search" }));
    expect(openLink).toHaveBeenCalledExactlyOnceWith({ kind: "link", url }, undefined);
    expect(windowOpen).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Could not open this link."));
    windowOpen.mockRestore();
  });

  it.each([false, true])("blocks unsupported and credential-bearing URLs with workspace links %s", (withWorkspace) => {
    const { open } = renderWithOpen(
      <MessageResponse mode="static" workspacePath={withWorkspace ? "/research" : undefined}>
        {[
          "[Local file](file:///tmp/private)",
          "[Script](javascript:alert(1))",
          "[Credential URL](https://user:secret@example.com/report)",
          "[Relative](/report)",
          "[Escaping file](loupist-workspace:..%2Fsecret)",
          "[Absolute file](loupist-workspace:%2Fetc%2Fpasswd)",
          '<script>alert("bad")</script><img src="loupist-workspace:private.png" onerror="alert(1)">',
        ].join("\n\n")}
      </MessageResponse>,
    );

    expect(screen.queryByRole("link")).not.toBeInTheDocument();
    expect(screen.getAllByText("Unsupported link.").length).toBeGreaterThan(0);
    expect(open).not.toHaveBeenCalled();
    expect(document.querySelector("script, [onerror]")).toBeNull();
    expect(document.querySelector('img[src^="loupist-workspace:"]')).toBeNull();
  });

  it("opens a workspace filename with spaces even when the answer omits angle brackets", async () => {
    const user = userEvent.setup();
    const { open } = renderWithOpen(<MessageResponse mode="static" workspacePath="/research" workspaceBasePath="reports">
      {"Saved [월별 잔액.csv](savings/월별 잔액.csv). Also [Report](</research/annual report.md>)."}
    </MessageResponse>);
    const saved = screen.getByRole("link", { name: "월별 잔액.csv" });
    // Prose links carry no icon; the underline is the affordance.
    expect(saved.querySelector("svg")).toBeNull();
    await user.click(saved);
    expect(open).toHaveBeenLastCalledWith({ kind: "file", path: "reports/savings/월별 잔액.csv" }, undefined);
    await user.click(screen.getByRole("link", { name: "Report" }));
    expect(open).toHaveBeenLastCalledWith({ kind: "file", path: "annual report.md" }, undefined);
  });

  it("does not recover unsafe paths, web URLs, images, or literal code as workspace links", () => {
    renderWithOpen(<MessageResponse mode="static" workspacePath="/research">
      {[
        "[Outside](../private report.csv)",
        "[Absolute](/tmp/private report.csv)",
        "[Web](https://example.com/private report.csv)",
        "![Image](private image.png)",
        "`[Inline](private report.csv)`",
        "```text\n[Fenced](private report.csv)\n```",
      ].join("\n\n")}
    </MessageResponse>);
    expect(document.querySelector('a[href^="loupist-workspace:"]')).toBeNull();
    expect(screen.queryByRole("link", { name: "Web" })).toBeNull();
    expect(screen.getByText("[Inline](private report.csv)")).toBeVisible();
    expect(screen.getByText("[Fenced](private report.csv)")).toBeVisible();
  });
});


it("renders completed math without TeX errors and leaves currency prose alone", async () => {
  const { container } = render(<MessageResponse mode="static">{String.raw`가격은 $25,000입니다. $$\frac{1}{2} + \begin{pmatrix}1 & 2 \\ 3 & 4\end{pmatrix}$$`}</MessageResponse>);
  await waitFor(() => expect(container.querySelector(".katex math mfrac")).not.toBeNull());
  expect(container.querySelector(".katex-error")).toBeNull();
  expect(container.textContent).toContain("가격은 $25,000입니다.");
});

const answer = (evidence: ChatEvidenceRef[]): ChatMessage => ({ id: "answer", role: "assistant", status: "streaming", content: "", created_at: "2026-10-28T00:00:00Z", evidence_refs: evidence });
const inTurn = (message: ChatMessage, text: string, extra: Partial<ComponentProps<typeof MessageResponse>> = {}) =>
  <TurnProvider message={message} loupistId="loupist-a"><MessageResponse {...extra}>{text}</MessageResponse></TurnProvider>;

it("opens only host-attached citations and refreshes the binding when evidence arrives during streaming", async () => {
  const evidence = evidenceFixture();
  const user = userEvent.setup();
  const text = `Revenue rose [1](#evidence-${evidence.id}). Unknown [2](#evidence-ev_unissued).`;
  const { open, rerender } = renderWithOpen(inTurn(answer([]), text, { isAnimating: true }));
  expect(screen.queryByRole("button")).toBeNull();
  expect(screen.queryByRole("link")).toBeNull();
  rerender(inTurn(answer([evidence]), text, { isAnimating: true }));
  const citation = await screen.findByRole("button", { name: "Source 1: Apple FY2025 annual report" });
  await user.click(citation);
  expect(open).toHaveBeenCalledExactlyOnceWith({ kind: "evidence", evidence, anchor: citation }, undefined);
  expect(screen.getAllByRole("button")).toHaveLength(1);
});

it("keeps a citation's visible link text in its accessible name", async () => {
  const evidence = evidenceFixture({ title: "Retained market data" });
  const { open } = renderWithOpen(inTurn(answer([evidence]), `Read the [truncated quote](#evidence-${evidence.id}).`, { mode: "static" }));
  const citation = await screen.findByRole("button", { name: "truncated quote · Source 1: Retained market data" });
  await userEvent.setup().click(citation);
  expect(open).toHaveBeenCalledWith(expect.objectContaining({ kind: "evidence", evidence, anchor: citation }), undefined);
});

it("never leaves a numbered saved-context citation dead: an asset page read opens its page, other saved research its reader", async () => {
  const asset = evidenceFixture({ id: "ev_asset", kind: "saved_context", tool_name: "asset_page_read", url: undefined, title: "Asset page: Apple", excerpt: '{\n  "assetId": "NASDAQ:AAPL"\n}' });
  const journal = evidenceFixture({ id: "ev_journal", kind: "saved_context", tool_name: "journal_read", url: undefined, title: "Journal: hold the quality call" });
  const user = userEvent.setup();
  const { open } = renderWithOpen(inTurn(answer([asset, journal]), `Held [1](#evidence-${asset.id}) since [2](#evidence-${journal.id}).`, { mode: "static" }));
  await user.click(await screen.findByRole("button", { name: /^Source 1:/u }));
  expect(open).toHaveBeenLastCalledWith({ kind: "asset", target: { loupistId: "loupist-a", assetId: "NASDAQ:AAPL" } }, undefined);
  await user.click(screen.getByRole("button", { name: /^Source 2:/u }));
  expect(open).toHaveBeenLastCalledWith(expect.objectContaining({ kind: "evidence", evidence: journal }), undefined);
});
