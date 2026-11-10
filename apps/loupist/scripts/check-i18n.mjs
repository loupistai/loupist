#!/usr/bin/env node
import ts from "typescript";
import { readdir, readFile } from "node:fs/promises";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const mode = process.argv[2] ?? "--enforce";
if (!["--report", "--enforce"].includes(mode) || process.argv.length > 3) {
  console.error("Usage: check-i18n.mjs [--report|--enforce]");
  process.exit(2);
}
const normalize = (text) => text.replace(/\s+/g, " ").trim();
const allowlist = JSON.parse(await readFile(join(root, "scripts/i18n-allowlist.json"), "utf8"));
if (!Array.isArray(allowlist) || allowlist.some((entry) => !entry.file || typeof entry.text !== "string" || !entry.reason?.trim())) {
  throw new Error("i18n allowlist entries require file, text and a non-empty reason");
}
const allowed = new Set(allowlist.map(({ file, text }) => JSON.stringify([file, normalize(text)])));
const attributes = new Set(["aria-label", "title", "placeholder", "alt", "label"]);
// A .ts module renders through the strings it returns, so its prose literals are checked.
// Contracts and the bridge carry wire values, the preview and tests carry fixtures.
const wireOrFixture = /^src\/(?:contracts|bridge|preview|test)\//;
// Prose: begins with a capital (or a script without case) and has at least two words.
const prose = (text) => /^[\p{Lu}\p{Lo}]/u.test(text) && /\p{L}\S*\s+\S*\p{L}/u.test(text);
const comparisons = new Set([ts.SyntaxKind.EqualsEqualsEqualsToken, ts.SyntaxKind.ExclamationEqualsEqualsToken, ts.SyntaxKind.EqualsEqualsToken, ts.SyntaxKind.ExclamationEqualsToken]);
const matchers = new Set(["startsWith", "endsWith", "includes", "indexOf", "split", "replace", "replaceAll"]);
/** A literal that cannot reach the screen as written: a key, a comparison with a host value, a diagnostic. */
function notRendered(node, source) {
  const parent = node.parent;
  if (ts.isImportDeclaration(parent) || ts.isExportDeclaration(parent) || ts.isExternalModuleReference(parent) || ts.isLiteralTypeNode(parent)) return true;
  if ((ts.isPropertyAssignment(parent) || ts.isPropertySignature(parent) || ts.isPropertyDeclaration(parent) || ts.isMethodDeclaration(parent) || ts.isEnumMember(parent)) && parent.name === node) return true;
  if (ts.isElementAccessExpression(parent) && parent.argumentExpression === node) return true;
  if (ts.isBinaryExpression(parent) && comparisons.has(parent.operatorToken.kind)) return true;
  if (ts.isCaseClause(parent)) return true;
  if (ts.isCallExpression(parent) || ts.isNewExpression(parent)) {
    const callee = parent.expression.getText(source);
    if (/(?:^|\.)(?:import|require)$|^console\.|(?:^|\.)t$|Error$/.test(callee) || parent.expression.kind === ts.SyntaxKind.ImportKeyword) return true;
    if (ts.isPropertyAccessExpression(parent.expression) && matchers.has(parent.expression.name.text)) return true;
  }
  // Values Rust writes and the UI recognizes are named HOST_*.
  if (ts.isVariableDeclaration(parent) && ts.isIdentifier(parent.name) && parent.name.text.startsWith("HOST_")) return true;
  return false;
}
const violations = [];
async function scan(directory) {
  for (const entry of (await readdir(directory, { withFileTypes: true })).sort((a, b) => a.name.localeCompare(b.name))) {
    const path = join(directory, entry.name);
    const file = relative(root, path).replaceAll("\\", "/");
    if (file === "src/i18n" || file === "src/preview/fixtures.ts") continue;
    if (entry.isDirectory()) { await scan(path); continue; }
    const module = file.endsWith(".ts") && !file.endsWith(".d.ts") && !file.endsWith(".test.ts") && !wireOrFixture.test(file);
    if (!module && (!file.endsWith(".tsx") || file.endsWith(".test.tsx"))) continue;
    const source = ts.createSourceFile(file, await readFile(path, "utf8"), ts.ScriptTarget.Latest, true, module ? ts.ScriptKind.TS : ts.ScriptKind.TSX);
    const reported = new Set();
    function report(node, value) {
      const text = normalize(value);
      if (!/\p{L}/u.test(text) || allowed.has(JSON.stringify([file, text])) || reported.has(node)) return;
      reported.add(node);
      const { line } = source.getLineAndCharacterOfPosition(node.getStart(source));
      violations.push({ file, line: line + 1, text });
    }
    function literal(node) {
      return node && (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node));
    }
    // Inspect values that can render, not predicates, translation keys, or
    // arguments to formatters. JSX inside a branch is visited separately.
    function renderedExpression(node) {
      if (!node) return;
      if (literal(node)) report(node, node.text);
      else if (ts.isParenthesizedExpression(node) || ts.isAsExpression(node) || ts.isNonNullExpression(node) || ts.isSatisfiesExpression(node)) {
        renderedExpression(node.expression);
      } else if (ts.isConditionalExpression(node)) {
        renderedExpression(node.whenTrue);
        renderedExpression(node.whenFalse);
      } else if (ts.isBinaryExpression(node)) {
        const operator = node.operatorToken.kind;
        if ([ts.SyntaxKind.BarBarToken, ts.SyntaxKind.QuestionQuestionToken, ts.SyntaxKind.PlusToken].includes(operator)) renderedExpression(node.left);
        if ([ts.SyntaxKind.AmpersandAmpersandToken, ts.SyntaxKind.BarBarToken, ts.SyntaxKind.QuestionQuestionToken, ts.SyntaxKind.PlusToken].includes(operator)) renderedExpression(node.right);
      } else if (ts.isTemplateExpression(node)) {
        report(node, [node.head.text, ...node.templateSpans.map((span) => span.literal.text)].join("…"));
        for (const span of node.templateSpans) renderedExpression(span.expression);
      }
    }
    function visit(node) {
      if (module && (literal(node) || ts.isTemplateExpression(node)) && !notRendered(node, source)) {
        const text = ts.isTemplateExpression(node) ? [node.head.text, ...node.templateSpans.map((span) => span.literal.text)].join("…") : node.text;
        if (prose(normalize(text))) report(node, text);
      }
      if (ts.isJsxText(node)) report(node, node.text);
      if (ts.isJsxAttribute(node) && attributes.has(node.name.getText(source))) {
        const value = node.initializer && ts.isJsxExpression(node.initializer) ? node.initializer.expression : node.initializer;
        renderedExpression(value);
      }
      if (ts.isJsxExpression(node) && (ts.isJsxElement(node.parent) || ts.isJsxFragment(node.parent))) {
        renderedExpression(node.expression);
      }
      if (ts.isCallExpression(node)) {
        const callee = node.expression.getText(source);
        // errorMessage fallback strings and React error-display setters are used by this app.
        const indexes = callee === "errorMessage" ? [1]
          : /^set\w*Error$/.test(callee) || /^(?:toast(?:\.(?:error|success|info|warning))?|showError|showToast)$/.test(callee) ? [0] : [];
        for (const index of indexes) if (literal(node.arguments[index])) report(node.arguments[index], node.arguments[index].text);
      }
      ts.forEachChild(node, visit);
    }
    visit(source);
  }
}
await scan(join(root, "src"));
const counts = new Map();
for (const { file } of violations) counts.set(file, (counts.get(file) ?? 0) + 1);
for (const [file, count] of [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))) console.log(`${String(count).padStart(4)}  ${file}`);
console.log(`Total: ${violations.length} violations in ${counts.size} files`);
if (mode === "--enforce") {
  for (const { file, line, text } of violations) console.error(`${file}:${line}: ${text}`);
  if (violations.length) process.exitCode = 1;
}
