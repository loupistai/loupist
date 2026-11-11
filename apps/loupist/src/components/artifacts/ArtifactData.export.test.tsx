import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ArtifactData } from "./ArtifactData";
import type { ChartDataset } from "../../types";

const dataset = {
  columns: [{ id: "name", label: "Name", kind: "string" }, { id: "margin", label: "Margin", kind: "number" }],
  rows: [["Hanul", 14.8], ["Morrow, Inc.", 9.1]],
  lineage: { kind: "agent_authored", upstream_receipts: [] },
} as unknown as ChartDataset;

it("ignores a copy result from a data page the reader has left", async () => {
  let resolve!: () => void;
  vi.spyOn(navigator.clipboard, "writeText").mockImplementationOnce(() => new Promise<void>(done => { resolve = done; }));
  render(<ArtifactData dataset={{ ...dataset, rows: Array.from({ length: 30 }, (_, index) => [String(index), index]) }} />);
  fireEvent.click(screen.getByRole("button", { name: "Copy rows" }));
  fireEvent.click(screen.getByRole("button", { name: "Next data rows" }));
  await act(async () => { resolve(); });
  expect(screen.getByRole("button", { name: "Copy rows" })).toBeVisible();
  expect(screen.queryByRole("alert")).toBeNull();
});

it("saves every row as CSV through the native save dialog", async () => {
  const user = userEvent.setup();
  const onExport = vi.fn(async () => "/Users/me/margins.csv");
  render(<ArtifactData dataset={dataset} title="Margins: Q3" onExport={onExport} />);
  await user.click(screen.getByRole("button", { name: "Save as CSV…" }));
  expect(onExport).toHaveBeenCalledWith({ suggestedName: "Margins Q3.csv", kind: "csv", bytes: expect.any(Array) });
  const [{ bytes }] = onExport.mock.calls[0] as unknown as [{ bytes: number[] }];
  expect(new TextDecoder().decode(new Uint8Array(bytes))).toBe('Name,Margin\r\nHanul,14.8\r\n"Morrow, Inc.",9.1');
});

it("offers no save without a way to write files", () => {
  render(<ArtifactData dataset={dataset} />);
  expect(screen.queryByRole("button", { name: "Save as CSV…" })).not.toBeInTheDocument();
});
