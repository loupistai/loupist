import { workspaceFolderName, loupistOptionLabel } from "./loupistFolder";
import { MOCK_LOUPISTS } from "../bridge/mock/fixtures";

it.each([
  ["/research/반도체", "반도체"],
  ["/research/long folder/", "long folder"],
  ["C:\\Users\\research\\Semiconductors", "Semiconductors"],
  ["C:\\", "C:"],
])("uses the actual folder name in %s", (path, expected) => {
  expect(workspaceFolderName(path)).toBe(expected);
});

it("keeps renamed profiles in the same parent folder distinguishable", () => {
  const first = { ...MOCK_LOUPISTS[0], name: "Research", workspace_path: "/home/research/one" };
  const second = { ...MOCK_LOUPISTS[1], name: "Research", workspace_path: "/home/research/two" };
  expect(loupistOptionLabel(first, [first, second])).toBe("Research · /home/research/one");
  expect(loupistOptionLabel(second, [first, second])).toBe("Research · /home/research/two");
});

it("disambiguates settings options without changing the folder name", () => {
  const first = { ...MOCK_LOUPISTS[0], name: "research", workspace_path: "/home/personal/research" };
  const second = { ...MOCK_LOUPISTS[1], name: "research", workspace_path: "/home/work/research" };
  expect(loupistOptionLabel(first, [first])).toBe("research");
  expect(loupistOptionLabel(first, [first, second])).toBe("research · /home/personal");
  expect(loupistOptionLabel(second, [first, second])).toBe("research · /home/work");
});
