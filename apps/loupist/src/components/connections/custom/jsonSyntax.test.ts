import { jsonSyntaxIssue } from "./jsonSyntax";

it.each([
  ['{\r\n  "company" 9}', 2, 13, "colon"],
  ['["📈" null]', 1, 6, "arrayEnd"],
  ['{"nested": {"key": [] "b": 2}}', 1, 23, "objectEnd"],
  ['{"x": "\\q"}', 1, 8, "string"],
  ['{"x":true} false', 1, 12, "end"],
  ['[1,]', 1, 4, "value"],
  ['{"a":1,}', 1, 8, "key"],
] as const)("locates the first syntax problem in %s", (input, line, column, reason) => {
  expect(jsonSyntaxIssue(input)).toEqual({ line, column, reason });
});

it.each(['', 'null', '42', '{"nested":[false,-2.3e4,"\\u1234"]}'])("leaves well-formed values and the empty draft to the dialog", input => {
  expect(jsonSyntaxIssue(input)).toBeNull();
});
