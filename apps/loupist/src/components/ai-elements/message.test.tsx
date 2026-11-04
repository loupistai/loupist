import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MessageResponse } from "./message-response";

describe("MessageResponse links", () => {
  it("opens Lens links only through an explicitly enabled Lens callback", async () => {
    const user = userEvent.setup();
    const onOpenLink = vi.fn();
    const onLensLink = vi.fn();
    const text = "[Valuation](lens:rec_0123456789abcdef0123456789abcdef)";
    const view = render(<MessageResponse mode="static" onOpenLink={onOpenLink}>{text}</MessageResponse>);
    expect(screen.queryByRole("link")).toBeNull();
    view.rerender(<MessageResponse mode="static" onOpenLink={onOpenLink} onLensLink={onLensLink}>{text}</MessageResponse>);
    await user.click(screen.getByRole("link", { name: "Valuation" }));
    expect(onLensLink).toHaveBeenCalledExactlyOnceWith("rec_0123456789abcdef0123456789abcdef");
    expect(onOpenLink).not.toHaveBeenCalled();

    const nextLoupist = vi.fn();
    view.rerender(<MessageResponse mode="static" onOpenLink={onOpenLink} onLensLink={nextLoupist}>{text}</MessageResponse>);
    await user.click(screen.getByRole("link", { name: "Valuation" }));
    expect(nextLoupist).toHaveBeenCalledExactlyOnceWith("rec_0123456789abcdef0123456789abcdef");
  });

  it("does not navigate malformed Lens destinations, images or literal examples", () => {
    render(<MessageResponse mode="static" onOpenLink={vi.fn()} onLensLink={vi.fn()}>{[
      "[Path](lens:..%2Fprivate)", "[Fragment](lens:method%23secret)",
      "[Blank](lens:)", "[Control](lens:method%00)", "[Encoded alias](lens:%72ec_0123456789abcdef0123456789abcdef)",
      "![Image](lens:method)", "`[Example](lens:method)`",
    ].join("\n\n")}</MessageResponse>);
    expect(screen.queryByRole("link")).toBeNull();
    expect(document.querySelector('img[src^="lens:"]')).toBeNull();
    expect(screen.getByText("[Example](lens:method)")).toBeVisible();
  });

  it("opens credential-free web links through the host callback and announces a failed open", async () => {
    const user = userEvent.setup();
    const openLink = vi.fn().mockRejectedValue(new Error("open failed"));
    const windowOpen = vi.spyOn(window, "open").mockImplementation(() => null);
    const url = "https://dart.fss.or.kr/dsab001/search.ax?textCrpNm=005930";
    render(<MessageResponse mode="static" onOpenLink={openLink}>{`[DART company search](${url})`}</MessageResponse>);
    await user.click(screen.getByRole("link", { name: "DART company search" }));
    expect(openLink).toHaveBeenCalledExactlyOnceWith(url);
    expect(windowOpen).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Could not open this link."));
    windowOpen.mockRestore();
  });

  it.each([false, true])("blocks unsupported and credential-bearing URLs with workspace links %s", (withWorkspace) => {
    const openLink = vi.fn();
    const openFile = vi.fn();
    render(
      <MessageResponse mode="static" onOpenLink={openLink} onOpenFile={withWorkspace ? openFile : undefined} workspacePath="/research">
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
    expect(openLink).not.toHaveBeenCalled();
    expect(openFile).not.toHaveBeenCalled();
    expect(document.querySelector("script, [onerror]")).toBeNull();
    expect(document.querySelector('img[src^="loupist-workspace:"]')).toBeNull();
  });

  it("opens a workspace filename with spaces even when the answer omits angle brackets", async () => {
    const user = userEvent.setup();
    const openFile = vi.fn();
    render(<MessageResponse mode="static" onOpenLink={vi.fn()} onOpenFile={openFile} workspacePath="/research" workspaceBasePath="reports">
      {"Saved [월별 잔액.csv](savings/월별 잔액.csv). Also [Report](</research/annual report.md>)."}
    </MessageResponse>);
    await user.click(screen.getByRole("link", { name: "월별 잔액.csv" }));
    expect(openFile).toHaveBeenLastCalledWith("reports/savings/월별 잔액.csv");
    await user.click(screen.getByRole("link", { name: "Report" }));
    expect(openFile).toHaveBeenLastCalledWith("annual report.md");
  });

  it("does not recover unsafe paths, web URLs, images, or literal code as workspace links", () => {
    render(<MessageResponse mode="static" onOpenLink={vi.fn()} onOpenFile={vi.fn()} workspacePath="/research">
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

it("opens only host-attached citations and refreshes the binding when evidence arrives during streaming", async () => {
  const { evidenceFixture } = await import("../../test/evidenceFixtures");
  const evidence = evidenceFixture();
  const user = userEvent.setup();
  const open = vi.fn();
  const openLink = vi.fn();
  const text = `Revenue rose [1](#evidence-${evidence.id}). Unknown [2](#evidence-ev_unissued).`;
  const view = render(<MessageResponse isAnimating onOpenLink={openLink} onOpenEvidence={open} evidenceRefs={[]}>{text}</MessageResponse>);
  expect(screen.queryByRole("button")).toBeNull();
  expect(screen.queryByRole("link")).toBeNull();
  view.rerender(<MessageResponse isAnimating onOpenLink={openLink} onOpenEvidence={open} evidenceRefs={[evidence]}>{text}</MessageResponse>);
  const citation = await screen.findByRole("button", { name: "Source 1: Apple FY2025 annual report" });
  await user.click(citation);
  expect(open).toHaveBeenCalledExactlyOnceWith(evidence, citation);
  expect(open.mock.calls[0]?.[1]).toBe(citation);
  expect(openLink).not.toHaveBeenCalled();
  expect(screen.getAllByRole("button")).toHaveLength(1);
});


it("keeps a citation's visible link text in its accessible name", async () => {
  const { evidenceFixture } = await import("../../test/evidenceFixtures");
  const evidence = evidenceFixture({ title: "Retained market data" });
  const open = vi.fn();
  render(<MessageResponse mode="static" onOpenLink={vi.fn()} onOpenEvidence={open} evidenceRefs={[evidence]}>{`Read the [truncated quote](#evidence-${evidence.id}).`}</MessageResponse>);
  const citation = await screen.findByRole("button", { name: "truncated quote · Source 1: Retained market data" });
  await userEvent.setup().click(citation);
  expect(open).toHaveBeenCalledWith(evidence, citation);
});
