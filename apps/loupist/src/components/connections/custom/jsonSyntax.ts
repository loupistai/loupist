type Expect = "value" | "firstItem" | "firstKey" | "key" | "colon" | "separator" | "end";
type Problem = "value" | "key" | "firstKey" | "colon" | "objectEnd" | "arrayEnd" | "end" | "string";

/** JSON.parse does the validation. WebKit omits offsets in its exceptions, so
 * locate the first unexpected token without displaying pasted credentials. */
export function jsonSyntaxIssue(text: string) {
  if (!text.trim()) return null;
  try { JSON.parse(text); return null; } catch { /* Locate the syntax error below. */ }

  let offset = 0;
  const issue = (reason: Problem) => {
    const lines = text.slice(0, offset).split(/\r\n?|\n/u);
    return { reason, line: lines.length, column: Array.from(lines.at(-1)!).length + 1 };
  };
  const spaces = /[\t\n\r ]*/y;
  const stringBody = /"(?:[^"\\\u0000-\u001f]|\\(?:["\\/bfnrt]|u[\da-fA-F]{4}))*/y;
  const scalar = /true|false|null|-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/y;
  const closing: string[] = [];
  let next: Expect = "value";
  const expectsKey = (state: Expect) => state === "firstKey" || state === "key";
  const afterValue = () => closing.length ? "separator" : "end";
  for (;;) {
    spaces.lastIndex = offset;
    spaces.exec(text);
    offset = spaces.lastIndex;
    const token = text[offset];
    if (next === "end") return issue("end");
    if (token && token === closing.at(-1) && (next === "separator" || next === "firstKey" || next === "firstItem")) {
      closing.pop();
      next = afterValue();
    } else if (next === "separator") {
      if (token !== ",") return issue(closing.at(-1) === "}" ? "objectEnd" : "arrayEnd");
      next = closing.at(-1) === "}" ? "key" : "value";
    } else if (next === "colon") {
      if (token !== ":") return issue("colon");
      next = "value";
    } else if (token === '"') {
      stringBody.lastIndex = offset;
      stringBody.exec(text);
      offset = stringBody.lastIndex;
      if (text[offset] !== '"') return issue("string");
      next = expectsKey(next) ? "colon" : afterValue();
    } else if (next === "firstKey" || next === "key") {
      return issue(next);
    } else if (token === "{" || token === "[") {
      closing.push(token === "{" ? "}" : "]");
      next = token === "{" ? "firstKey" : "firstItem";
    } else {
      scalar.lastIndex = offset;
      if (!scalar.exec(text)) return issue("value");
      offset = scalar.lastIndex;
      next = afterValue();
      continue;
    }
    offset++;
  }
}
