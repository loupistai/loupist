import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "@/i18n";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { LoupistBridge, WorkbookRead, WorkspaceDocumentOpen, WorkspaceFile } from "../../types";
import { WorkspaceFiles } from "./WorkspaceFiles";

const file: WorkspaceFile = { path: "analysis.py", content: "print(17 * 23)", content_sha256: "abc123", media_type: "text/plain", size_bytes: 14, truncated: false, binary: false };

it("opens a generated chart in the reader and offers recovery for corrupt images", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const image = { ...file, path: "returns.png", content: null, binary: true, media_type: "image/png", image_data_url: "data:image/png;base64,iVBORw0KGgo=" };
  vi.spyOn(bridge, "workspaceRead").mockResolvedValue(image);
  const reveal = vi.spyOn(bridge, "workspaceReveal").mockResolvedValue(undefined);
  render(<WorkspaceFiles bridge={bridge} loupistId="loupist" path="" filePath="returns.png" onNavigate={vi.fn()} onAsk={vi.fn()} />);
  const chart = await screen.findByRole("img", { name: "returns.png" });
  expect(chart).toHaveAttribute("src", image.image_data_url);
  // The size button names what it will do; it is not also a pressed toggle, which would read "Fit to panel, pressed".
  const size = screen.getByRole("button", { name: "Actual size" });
  expect(size).not.toHaveAttribute("aria-pressed");
  fireEvent.click(size);
  expect(screen.getByRole("button", { name: "Fit to panel" })).not.toHaveAttribute("aria-pressed");
  fireEvent.error(chart);
  expect(screen.getByRole("status")).toHaveTextContent("Could not display this image");
  await userEvent.setup().click(screen.getByRole("button", { name: "Open in file manager" }));
  expect(reveal).toHaveBeenCalledWith({ loupist_id: "loupist", path: "returns.png" });
});

it.each(["https://example.com/chart.png", "data:image/svg+xml;base64,PHN2Zz4=", "data:text/html;base64,PHNjcmlwdD4="])("never loads a remote or active-content image payload %s", async (image_data_url) => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "workspaceRead").mockResolvedValue({ ...file, path: "chart.png", content: null, binary: true, image_data_url });
  render(<WorkspaceFiles bridge={bridge} loupistId="loupist" path="" filePath="chart.png" onNavigate={vi.fn()} onAsk={vi.fn()} />);
  await screen.findByText("No preview for this kind of file.");
  expect(screen.queryByRole("img")).not.toBeInTheDocument();
});
it("previews an SVG output only as an image resource, with its source one toggle away", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const content = `<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"><script>alert(2)</script><image href="https://example.com/x.png"/><rect id="bar" width="4" height="9"/></svg>`;
  vi.spyOn(bridge, "workspaceRead").mockResolvedValue({ ...file, path: "outputs/chart.svg", content, media_type: "image/svg+xml", size_bytes: content.length });
  const view = render(<WorkspaceFiles bridge={bridge} loupistId="loupist" path="" filePath="outputs/chart.svg" onNavigate={vi.fn()} onAsk={vi.fn()} />);
  const chart = await screen.findByRole("img", { name: "chart.svg" });
  expect(chart.tagName).toBe("IMG");
  expect(chart).toHaveAttribute("src", `data:image/svg+xml;charset=utf-8,${encodeURIComponent(content)}`);
  expect(screen.getByRole("button", { name: "Preview" })).toHaveAttribute("aria-pressed", "true");
  // The markup is never part of the page: no SVG element, script, iframe or object.
  expect(view.container.querySelector("svg rect, script, iframe, object, embed, #bar")).toBeNull();
  expect(screen.queryByRole("note")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Source" }));
  expect(screen.queryByRole("img", { name: "chart.svg" })).not.toBeInTheDocument();
  expect(view.container.querySelector(".reader-source code")).toHaveTextContent("<script>alert(2)</script>");
  expect(view.container.querySelector("script, #bar")).toBeNull();
});

it("shows a truncated SVG as source, never a drawn prefix", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "workspaceRead").mockResolvedValue({ ...file, path: "big.svg", content: "<svg xmlns=\"http://www.w3.org/2000/svg\">", content_sha256: null, truncated: true, size_bytes: 400_000 });
  const view = render(<WorkspaceFiles bridge={bridge} loupistId="loupist" path="" filePath="big.svg" onNavigate={vi.fn()} onAsk={vi.fn()} />);
  await waitFor(() => expect(view.container.querySelector(".reader-source code")).toHaveTextContent("<svg"));
  expect(screen.queryByRole("img")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Preview" })).not.toBeInTheDocument();
});

it("opens directory entries and asks only through explicit action", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "workspaceList").mockResolvedValue({ path: "", truncated: false, entries: [{ kind: "directory", name: "reports", path: "reports", size_bytes: null }, { kind: "file", name: "analysis.py", path: "analysis.py", size_bytes: 14 }] });
  const read = vi.spyOn(bridge, "workspaceRead").mockResolvedValue(file);
  const ask = vi.fn();
  const navigate = vi.fn();
  const props = { bridge, loupistId: "loupist-quality", rootPath: "/research", path: "", onNavigate: navigate, onAsk: ask, onOpenLink: vi.fn() };
  const { rerender } = render(<WorkspaceFiles {...props} />);
  expect(await screen.findByRole("button", { name: "reports" })).toBeVisible();
  expect(screen.queryByText("null B")).not.toBeInTheDocument();
  await user.click(await screen.findByRole("button", { name: /analysis.py/ }));
  expect(navigate).toHaveBeenCalledWith("", "analysis.py");
  rerender(<WorkspaceFiles {...props} filePath="analysis.py" />);
  expect(await screen.findByText("print(17 * 23)")).toBeVisible();
  expect(read).toHaveBeenCalledWith({ loupist_id: "loupist-quality", path: "analysis.py" });
  expect(ask).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Ask about this" }));
  expect(ask).toHaveBeenCalledWith(file);
});

it("discards a delayed file from a previous Loupist and reports binary/truncated previews", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  let resolve!: (value: WorkspaceFile) => void;
  vi.spyOn(bridge, "workspaceRead").mockReturnValueOnce(new Promise((done) => { resolve = done; })).mockResolvedValue({ ...file, path: "report.pdf", binary: true, content: null, truncated: true });
  const open = vi.spyOn(bridge, "workspaceDocumentOpen").mockResolvedValue({ status: "requested", application_name: null });
  const props = { bridge, path: "", onNavigate: vi.fn(), onAsk: vi.fn(), onOpenLink: vi.fn() };
  const { rerender } = render(<WorkspaceFiles {...props} loupistId="first" filePath="analysis.py" />);
  rerender(<WorkspaceFiles {...props} loupistId="second" filePath="report.pdf" />);
  await screen.findByText("No preview for this kind of file.");
  await act(async () => resolve(file));
  expect(screen.queryByText("print(17 * 23)")).not.toBeInTheDocument();
  expect(screen.getByText(/Preview truncated/)).toBeVisible();
  await waitFor(() => expect(screen.getByRole("button", { name: "Ask about this" })).toBeEnabled());
  await userEvent.setup().click(await screen.findByRole("button", { name: "Open" }));
  expect(open).toHaveBeenCalledWith({ loupist_id: "second", path: "report.pdf" });
});

it("previews financial CSV as exact text with a header choice and an inspectable source", async () => {
  const user = userEvent.setup();
  const csv = { ...file, path: "prices.csv", media_type: "text/csv", content: 'symbol,company,value,note\n005930,"Samsung, Inc.",000123.4500,"line one\nline ""two"""\nAAPL,Apple,=1+1,stable' };
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "workspaceRead").mockResolvedValue(csv);
  const view = render(<WorkspaceFiles bridge={bridge} loupistId="loupist" path="" filePath="prices.csv" onNavigate={vi.fn()} onAsk={vi.fn()} />);
  const table = await screen.findByRole("table", { name: "Data preview for prices.csv" });
  for (const cell of ["005930", "Samsung, Inc.", "000123.4500", "=1+1"]) expect(within(table).getByRole("cell", { name: cell })).toBeVisible();
  expect(within(table).getByRole("cell", { name: /line one\s+line "two"/ }).textContent).toBe('line one\nline "two"');
  await user.click(screen.getByRole("checkbox", { name: "First row is header" }));
  expect(within(table).getByRole("columnheader", { name: "Column 1" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Source" }));
  expect(view.container.querySelector("pre code")).toHaveTextContent("000123.4500");
});

it("keeps the loaded file after a failed refresh", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "workspaceRead").mockResolvedValueOnce(file).mockRejectedValueOnce(new Error("File unavailable"));
  render(<WorkspaceFiles bridge={bridge} loupistId="loupist" path="" filePath="analysis.py" onNavigate={vi.fn()} onAsk={vi.fn()} />);
  await screen.findByText("print(17 * 23)");
  await user.click(screen.getByRole("button", { name: "Refresh files" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("File unavailable");
  expect(screen.getByText("print(17 * 23)")).toBeVisible();
});

it("omits an incomplete record from a truncated table and lets the source remain inspectable", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const content = 'symbol,value\nAAPL,123.40\nMSFT,"incomplete';
  vi.spyOn(bridge, "workspaceRead").mockResolvedValue({ ...file, path: "prices.csv", media_type: "text/csv", content, truncated: true });
  const view = render(<WorkspaceFiles bridge={bridge} loupistId="loupist" path="" filePath="prices.csv" onNavigate={vi.fn()} onAsk={vi.fn()} />);
  const table = await screen.findByRole("table");
  expect(within(table).getByRole("cell", { name: "AAPL" })).toBeVisible();
  expect(within(table).queryByText("MSFT")).not.toBeInTheDocument();
  expect(screen.getByText(/unfinished final record is omitted/)).toBeVisible();
  await userEvent.setup().click(screen.getByRole("button", { name: "Source" }));
  expect(view.container.querySelector("pre code")?.textContent).toBe(content);
});

it("shows an empty folder's next step without a filter for nothing", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const list = vi.spyOn(bridge, "workspaceList").mockResolvedValue({ path: "", truncated: false, entries: [] });
  render(<WorkspaceFiles bridge={bridge} loupistId="loupist" rootPath="/research" path="" onNavigate={vi.fn()} onAsk={vi.fn()} />);
  expect(await screen.findByText("This folder is empty.")).toBeVisible();
  expect(screen.getByText("Research files saved in this workspace will appear here.")).toBeVisible();
  expect(screen.queryByRole("textbox", { name: "Filter files" })).toBeNull();
  // Icon-only controls carry their name as a tooltip; the reveal tooltip also names the folder.
  expect(screen.getByRole("button", { name: "Refresh files" })).toHaveAttribute("title", "Refresh files");
  expect(screen.getByRole("button", { name: "Show in file manager" })).toHaveAttribute("title", "Show in file manager · /research");
  list.mockResolvedValue({ path: "", truncated: false, entries: [{ kind: "file", name: "notes.md", path: "notes.md", size_bytes: 12 }] });
  await user.click(screen.getByRole("button", { name: "Refresh files" }));
  expect(await screen.findByRole("textbox", { name: "Filter files" })).toBeVisible();
});

const workbookFile: WorkspaceFile = {
  ...file, path: "outputs/comparison.xlsx", content: null,
  media_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", binary: true,
};

it("reads a workbook and switches visible sheets while keeping their workbook indices", async () => {
  const user = userEvent.setup();
  const bridge: LoupistBridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "workspaceRead").mockResolvedValue(workbookFile);
  vi.spyOn(bridge, "workspaceDocumentHandler").mockResolvedValue({ status: "allowed", application_name: "Microsoft Excel" });
  const open = vi.spyOn(bridge, "workspaceDocumentOpen");
  const sheets = [
    { name: "Comparison", hidden: false, worksheet: true },
    { name: "Assumptions", hidden: true, worksheet: true },
    { name: "Forecast", hidden: false, worksheet: true },
  ];
  const summary: WorkbookRead = {
    status: "ready", sheets, selected_sheet: 0, first_row: 12, first_column: 26,
    rows_total: 203, columns_total: 4, truncated: true, omitted: ["charts", "images", "visual_styles"],
    rows: [
      [{ kind: "text", display: "Revenue", formula: false },
        { kind: "number", display: "12.5%", formula: true },
        { kind: "date", display: "2026-11-21", formula: false },
        { kind: "error", display: "#DIV/0!", formula: false }],
      [{ kind: "text", display: "Operating income", formula: false }, { kind: "empty", display: "", formula: true }],
    ],
  };
  let finish!: (result: WorkbookRead) => void;
  const read = vi.spyOn(bridge, "workspaceWorkbookRead")
    .mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }))
    .mockResolvedValue({ ...summary, selected_sheet: 2, first_row: 1, first_column: 1,
      rows_total: 1, columns_total: 1, truncated: false, rows: [[{ kind: "number", display: "125,000", formula: false }]] });
  render(<WorkspaceFiles bridge={bridge} loupistId="loupist" path="outputs" filePath={workbookFile.path} onNavigate={vi.fn()} onAsk={vi.fn()} />);
  expect(await screen.findByText("Reading the workbook…")).toHaveAttribute("role", "status");
  expect(await screen.findByRole("button", { name: "Open in Microsoft Excel" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "Show in file manager" })).toBeEnabled();
  expect(open).not.toHaveBeenCalled();
  await act(async () => finish(summary));

  const tabs = screen.getAllByRole("tab");
  expect(tabs.map(tab => tab.textContent?.trim())).toEqual(["Comparison", "Forecast", "Assumptions hidden"]);
  expect(tabs[0]).toHaveAttribute("aria-selected", "true");
  expect(tabs[2]).toBeDisabled();
  const comparison = screen.getByRole("table", { name: "Data preview for Comparison" });
  expect(within(comparison).getByRole("columnheader", { name: "AA" })).toBeVisible();
  expect(within(comparison).getByRole("rowheader", { name: "12" })).toBeVisible();
  const formula = within(comparison).getByRole("cell", { name: "12.5%" });
  expect(formula).toHaveAttribute("title", "Calculated by a formula. The saved result is shown.");
  expect(formula).toHaveAttribute("data-formula", "cached");
  const uncached = within(comparison).getByRole("cell", { name: "—" });
  expect(uncached).toHaveAttribute("title", "This formula has no saved result yet. Open the file in a spreadsheet application to calculate it.");
  expect(uncached).toHaveAttribute("data-formula", "uncached");
  expect(within(comparison).getByRole("cell", { name: "12.5%" })).toHaveClass("is-numeric");
  expect(within(comparison).getByRole("cell", { name: "2026-11-21" })).toBeVisible();
  expect(within(comparison).getByRole("cell", { name: "#DIV/0!" })).toHaveClass("is-error");
  expect(screen.getByText(/Values only/)).toHaveTextContent("Charts, images and formatting are not shown. Showing the first 2 of 203 rows.");

  await user.click(tabs[1]!);
  expect(read).toHaveBeenLastCalledWith({ loupist_id: "loupist", path: workbookFile.path, sheet_index: 2 }, expect.any(Function));
  expect(within(await screen.findByRole("table", { name: "Data preview for Forecast" })).getByRole("cell", { name: "125,000" })).toBeVisible();
});

it("explains why a workbook preview failed and shows an Open refusal inline", async () => {
  const user = userEvent.setup();
  const bridge: LoupistBridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "workspaceRead").mockResolvedValue(workbookFile);
  vi.spyOn(bridge, "workspaceWorkbookRead").mockResolvedValue({ status: "encrypted" });
  vi.spyOn(bridge, "workspaceDocumentHandler").mockResolvedValue({ status: "allowed", application_name: null });
  const open = vi.spyOn(bridge, "workspaceDocumentOpen").mockResolvedValue({ status: "refused", reason: "unsafe_document" });
  render(<WorkspaceFiles bridge={bridge} loupistId="loupist" path="outputs" filePath={workbookFile.path} onNavigate={vi.fn()} onAsk={vi.fn()} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("This workbook is password-protected.");
  await user.click(await screen.findByRole("button", { name: "Open" }));
  expect(open).toHaveBeenCalledWith({ loupist_id: "loupist", path: workbookFile.path });
  expect(await screen.findByText("This file cannot be opened safely.")).toBeVisible();
  expect(screen.getByRole("button", { name: "Open" })).toBeEnabled();
});

it.each([
  ["en", "Open in Numbers", "Couldn’t open this file in Microsoft Excel.", "Show in file manager"],
  ["ko", "Numbers에서 열기", "이 파일을 Microsoft Excel에서 열지 못했습니다.", "파일 관리자에서 보기"],
])("waits for the system launch result and offers to reveal the workbook on failure (%s)", async (language, openLabel, errorText, revealLabel) => {
  await i18n.changeLanguage(language);
  const user = userEvent.setup();
  const bridge: LoupistBridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "workspaceRead").mockResolvedValue(workbookFile);
  vi.spyOn(bridge, "workspaceWorkbookRead").mockResolvedValue({ status: "encrypted" });
  vi.spyOn(bridge, "workspaceDocumentHandler").mockResolvedValue({ status: "allowed", application_name: "Numbers" });
  let settle!: (result: WorkspaceDocumentOpen) => void;
  const open = vi.spyOn(bridge, "workspaceDocumentOpen").mockReturnValue(new Promise(resolve => { settle = resolve; }));
  const reveal = vi.spyOn(bridge, "workspaceReveal").mockResolvedValue(undefined);
  const view = render(<WorkspaceFiles bridge={bridge} loupistId="loupist" path="outputs" filePath={workbookFile.path} onNavigate={vi.fn()} onAsk={vi.fn()} />);
  try {
    const button = await screen.findByRole("button", { name: openLabel });
    await user.click(button);
    expect(button).toBeDisabled();
    expect(screen.queryByText(errorText)).not.toBeInTheDocument();
    const request = { loupist_id: "loupist", path: workbookFile.path };
    expect(open).toHaveBeenCalledExactlyOnceWith(request);

    const reason = "The application could not read the file.";
    await act(async () => settle({ status: "handler_unavailable", application_name: "Microsoft Excel", reason }));
    const message = await screen.findByText(errorText);
    expect(message).toBeVisible();
    const alert = within(message.closest<HTMLElement>("[role=alert]")!);
    expect(alert.getByText(reason)).toBeVisible();
    expect(button).toBeEnabled();
    await user.click(alert.getByRole("button", { name: revealLabel }));
    expect(reveal).toHaveBeenCalledExactlyOnceWith(request);
  } finally {
    view.unmount();
    await i18n.changeLanguage("en");
  }
});

it.each(["xls", "xlsb"])("explains an unsupported %s preview and retains the Open action", async extension => {
  const bridge: LoupistBridge = new MockLoupistBridge({ delay_ms: 0 });
  const path = `outputs/comparison.${extension}`;
  vi.spyOn(bridge, "workspaceRead").mockResolvedValue({ ...workbookFile, path });
  vi.spyOn(bridge, "workspaceWorkbookRead").mockResolvedValue({ status: "unsupported_format" });
  vi.spyOn(bridge, "workspaceDocumentHandler").mockResolvedValue({ status: "refused", reason: "unsupported_type" });
  render(<WorkspaceFiles bridge={bridge} loupistId="loupist" path="outputs" filePath={path} onNavigate={vi.fn()} onAsk={vi.fn()} />);
  expect(await screen.findByRole("alert")).toHaveTextContent(`${extension.toUpperCase()} workbooks cannot be previewed.`);
  expect(screen.getByRole("button", { name: "Open" })).toBeEnabled();
});
