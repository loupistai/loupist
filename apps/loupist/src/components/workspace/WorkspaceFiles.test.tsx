import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { WorkspaceFile } from "../../types";
import { WorkspaceFiles } from "./WorkspaceFiles";

const file: WorkspaceFile = { path: "analysis.py", content: "print(17 * 23)", content_sha256: "abc123", media_type: "text/plain", size_bytes: 14, truncated: false, binary: false };

it("opens a generated chart in the reader and offers recovery for corrupt images", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const image = { ...file, path: "returns.png", content: null, binary: true, media_type: "image/png", image_data_url: "data:image/png;base64,iVBORw0KGgo=" };
  vi.spyOn(bridge, "workspaceRead").mockResolvedValue(image);
  const reveal = vi.spyOn(bridge, "workspaceReveal").mockResolvedValue(undefined);
  render(<WorkspaceFiles bridge={bridge} loupistId="loupist" path="" filePath="returns.png" onNavigate={vi.fn()} onAsk={vi.fn()} onOpenLink={vi.fn()} />);
  const chart = await screen.findByRole("img", { name: "returns.png" });
  expect(chart).toHaveAttribute("src", image.image_data_url);
  fireEvent.error(chart);
  expect(screen.getByRole("status")).toHaveTextContent("Could not display this image");
  await userEvent.setup().click(screen.getByRole("button", { name: "Open in file manager" }));
  expect(reveal).toHaveBeenCalledWith({ loupist_id: "loupist", path: "returns.png" });
});

it.each(["https://example.com/chart.png", "data:image/svg+xml;base64,PHN2Zz4=", "data:text/html;base64,PHNjcmlwdD4="])("never loads a remote or active-content image payload %s", async (image_data_url) => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "workspaceRead").mockResolvedValue({ ...file, path: "chart.png", content: null, binary: true, image_data_url });
  render(<WorkspaceFiles bridge={bridge} loupistId="loupist" path="" filePath="chart.png" onNavigate={vi.fn()} onAsk={vi.fn()} onOpenLink={vi.fn()} />);
  await screen.findByText(/No text preview/);
  expect(screen.queryByRole("img")).not.toBeInTheDocument();
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
  const props = { bridge, path: "", onNavigate: vi.fn(), onAsk: vi.fn(), onOpenLink: vi.fn() };
  const { rerender } = render(<WorkspaceFiles {...props} loupistId="first" filePath="analysis.py" />);
  rerender(<WorkspaceFiles {...props} loupistId="second" filePath="report.pdf" />);
  await screen.findByText(/No text preview/);
  await act(async () => resolve(file));
  expect(screen.queryByText("print(17 * 23)")).not.toBeInTheDocument();
  expect(screen.getByText(/Preview truncated/)).toBeVisible();
  await waitFor(() => expect(screen.getByRole("button", { name: "Ask about this" })).toBeEnabled());
});

it("previews financial CSV as exact text with a header choice and an inspectable source", async () => {
  const user = userEvent.setup();
  const csv = { ...file, path: "prices.csv", media_type: "text/csv", content: 'symbol,company,value,note\n005930,"Samsung, Inc.",000123.4500,"line one\nline ""two"""\nAAPL,Apple,=1+1,stable' };
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "workspaceRead").mockResolvedValue(csv);
  const view = render(<WorkspaceFiles bridge={bridge} loupistId="loupist" path="" filePath="prices.csv" onNavigate={vi.fn()} onAsk={vi.fn()} onOpenLink={vi.fn()} />);
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
  render(<WorkspaceFiles bridge={bridge} loupistId="loupist" path="" filePath="analysis.py" onNavigate={vi.fn()} onAsk={vi.fn()} onOpenLink={vi.fn()} />);
  await screen.findByText("print(17 * 23)");
  await user.click(screen.getByRole("button", { name: "Refresh files" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("File unavailable");
  expect(screen.getByText("print(17 * 23)")).toBeVisible();
});

it("omits an incomplete record from a truncated table and lets the source remain inspectable", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const content = 'symbol,value\nAAPL,123.40\nMSFT,"incomplete';
  vi.spyOn(bridge, "workspaceRead").mockResolvedValue({ ...file, path: "prices.csv", media_type: "text/csv", content, truncated: true });
  const view = render(<WorkspaceFiles bridge={bridge} loupistId="loupist" path="" filePath="prices.csv" onNavigate={vi.fn()} onAsk={vi.fn()} onOpenLink={vi.fn()} />);
  const table = await screen.findByRole("table");
  expect(within(table).getByRole("cell", { name: "AAPL" })).toBeVisible();
  expect(within(table).queryByText("MSFT")).not.toBeInTheDocument();
  expect(screen.getByText(/unfinished final record is omitted/)).toBeVisible();
  await userEvent.setup().click(screen.getByRole("button", { name: "Source" }));
  expect(view.container.querySelector("pre code")?.textContent).toBe(content);
});
