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

it("uses a short focus to distinguish duplicate names and leaves a unique name alone", () => {
  const first = { ...MOCK_LOUPISTS[0], name: "Research", philosophy: "Oil demand", workspace_path: "/home/research/one" };
  const second = { ...MOCK_LOUPISTS[1], name: "Research", philosophy: "Chip prices", workspace_path: "/home/research/two" };
  expect(loupistOptionLabel(first, [first])).toBe("Research");
  expect(loupistOptionLabel(first, [first, second])).toBe("Research · Oil demand");
  expect(loupistOptionLabel(second, [second, first])).toBe("Research · Chip prices");
});

it.each(["", "Companies I follow", "One deliberately long focus with the same opening text ".repeat(3)])(
  "numbers identical focus excerpts consistently even when the list is reordered", (focus) => {
    const first = { ...MOCK_LOUPISTS[0], id: "a", name: "Research", philosophy: focus };
    const second = { ...first, id: "b", philosophy: focus.length > 40 ? focus + "elsewhere" : focus };
    expect(loupistOptionLabel(first, [first, second])).toBe("Research · Profile 1");
    expect(loupistOptionLabel(second, [second, first])).toBe("Research · Profile 2");
  },
);
