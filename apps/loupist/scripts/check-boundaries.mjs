#!/usr/bin/env node
import ts from "typescript";
import { realpathSync } from "node:fs";
import { readdir, readFile } from "node:fs/promises";
import { dirname, join, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const isTest = (file) => /(?:^|\/)(?:__tests__|tests?|__mocks__)(?:\/|$)|\.(?:test|spec)\.[cm]?[jt]sx?$/.test(file);

export function inspectBoundaries(file, text) {
  file = file.replaceAll("\\", "/");
  const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true);
  const findings = [];
  function inspect(node) {
    if (!node || !ts.isStringLiteralLike(node)) return;
    const specifier = node.text;
    const local = specifier.startsWith(".") ? posix.join(posix.dirname(file), specifier)
      : specifier.startsWith("@/") ? `src/${specifier.slice(2)}` : specifier;
    const target = posix.normalize(local).replace(/\.(?:[cm]?[jt]sx?)$/, "");
    function report(rule) {
      findings.push({ file, line: source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1, rule, specifier });
    }
    if (!file.startsWith("src/bridge/") && /^@tauri-apps\/(?:api\/|plugin-)/.test(specifier)) report("boundaries/tauri-only-in-bridge");
    if (file.startsWith("src/components/") && target === "src/bridge/tauriBridge") report("boundaries/components-typed-bridge");
    const mockImplementation = file.startsWith("src/bridge/mock/") || file === "src/bridge/mockBridge.ts";
    if (!isTest(file) && !file.startsWith("src/preview/") && !mockImplementation
      && (/^(?:src\/preview|src\/bridge\/mock)(?:\/|$)/.test(target)
        || target === "src/bridge/mockBridge")) report("boundaries/no-preview-in-production");
  }
  function visit(node) {
    if (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) inspect(node.moduleSpecifier);
    else if (ts.isImportEqualsDeclaration(node) && ts.isExternalModuleReference(node.moduleReference)) inspect(node.moduleReference.expression);
    else if (ts.isImportTypeNode(node) && ts.isLiteralTypeNode(node.argument)) inspect(node.argument.literal);
    else if (ts.isCallExpression(node) && (node.expression.kind === ts.SyntaxKind.ImportKeyword
      || (ts.isIdentifier(node.expression) && node.expression.text === "require"))) inspect(node.arguments[0]);
    ts.forEachChild(node, visit);
  }
  visit(source);
  return findings;
}

export async function checkBoundaries(root) {
  const findings = [];
  async function scan(directory) {
    for (const entry of (await readdir(join(root, directory), { withFileTypes: true })).sort((a, b) => a.name.localeCompare(b.name))) {
      const file = `${directory}/${entry.name}`;
      if (entry.isDirectory()) await scan(file);
      else if (/\.[cm]?tsx?$/.test(file)) findings.push(...inspectBoundaries(file, await readFile(join(root, file), "utf8")));
    }
  }
  await scan("src");
  return findings;
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const mode = process.argv[2] ?? "--enforce";
  if (!["--report", "--enforce"].includes(mode) || process.argv.length > 3) {
    console.error("Usage: check-boundaries.mjs [--report|--enforce]");
    process.exitCode = 2;
  } else {
    const findings = await checkBoundaries(resolve(dirname(fileURLToPath(import.meta.url)), ".."));
    for (const hit of findings) console.log(`${hit.file}:${hit.line}: ${hit.rule}: ${hit.specifier}`);
    console.log(`Boundaries: ${findings.length} violation(s)`);
    if (mode === "--enforce" && findings.length) process.exitCode = 1;
  }
}
