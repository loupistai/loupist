import { workspaceFileLink } from "./workspaceFileLink";

it("resolves generated result links against the selected workspace", () => {
  expect(workspaceFileLink("reports/result.csv")).toBe("reports/result.csv");
  expect(workspaceFileLink("./reports/annual%20review.md#L12", "/research")).toBe("reports/annual review.md");
  expect(workspaceFileLink("/research/scripts/model.py:34", "/research")).toBe("scripts/model.py");
  expect(workspaceFileLink("../data.csv", "/research", "reports")).toBe("data.csv");
  expect(workspaceFileLink("C:\\Research\\report.csv", "C:\\Research")).toBe("report.csv");
});
it("rejects external schemes and paths outside the workspace", () => {
  for (const path of ["https://example.com/report.csv", "javascript:alert(1)", "file:///research/report.csv", "../outside.txt", "/research-other/file", "/etc/passwd", "%00file", "//example.com/file", "%bad", "report.csv?secret=1"]) {
    expect(workspaceFileLink(path, "/research")).toBeNull();
  }
});

it.each([
  ["C:/Research", "C:/research/report.csv"],
  [String.raw`\\?\C:\Research`, String.raw`\\?\c:\research\report.csv`],
  [String.raw`\\?\UNC\Server\Share\Research`, String.raw`\\server\share\research\report.csv`],
])("resolves Windows disk and UNC links for root %s", (root, path) => {
  expect(workspaceFileLink(path, root)).toBe("report.csv");
});

it("preserves containment while normalizing Windows root forms", () => {
  const disk = String.raw`\\?\C:\Research`;
  const unc = String.raw`\\?\UNC\Server\Share\Research`;
  for (const path of ["C:/Research-other/report.csv", "C:/Research/../secret.csv", "D:/Research/report.csv", String.raw`\\.\C:\Research\report.csv`, "C:/Research/report.csv?secret=1"]) {
    expect(workspaceFileLink(path, disk)).toBeNull();
  }
  for (const path of ["//server/share-other/Research/report.csv", "//server/share/Research/../../secret.csv", "//other/share/Research/report.csv"]) {
    expect(workspaceFileLink(path, unc)).toBeNull();
  }
  expect(workspaceFileLink("/Research/report.csv", "/research")).toBeNull();
  expect(workspaceFileLink("/reports/result.csv", "/")).toBe("reports/result.csv");
});
