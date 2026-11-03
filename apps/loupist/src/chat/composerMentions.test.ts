import { extractMentionedConnectionIds, knownComposerMentions, splitComposerMentions } from "./composerMentions";

const known = knownComposerMentions([{ id: "research" }, { id: "world" }], [{ id: "sec.edgar" }, { id: "world-bank.indicators" }]);
const text = (value: string) => [{ type: "text", value }];

it("marks only exact $ Skills and @ connections after a start or space", () => {
  expect(splitComposerMentions("$research hello @sec.edgar next", known)).toEqual([
    { type: "mention", kind: "skill", value: "$research" },
    { type: "text", value: " hello " },
    { type: "mention", kind: "connection", value: "@sec.edgar" },
    { type: "text", value: " next" },
  ]);
  for (const value of ["Samsung earnings", "write research@example.com", "$res", "$reearch", " @"]) expect(splitComposerMentions(value, known)).toEqual(text(value));
});

it("collects unique @ connection ids in prompt order, ignoring skills and emails", () => {
  expect(extractMentionedConnectionIds("@sec.edgar then @fred.macro and @sec.edgar again")).toEqual(["sec.edgar", "fred.macro"]);
  expect(extractMentionedConnectionIds("$research write research@example.com")).toEqual([]);
});
