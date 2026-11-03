#!/usr/bin/env node
import ts from "typescript";
import { realpathSync } from "node:fs";
import { readdir, readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// PB-W10.4 acceptance item 2; exact CSS identifiers avoid matching successors
// such as --gg-shadow-sm or unrelated names such as --paperwork.
const legacy = /--(?:ink|ink-soft|muted|muted-strong|line|line-strong|app-bg|navigation-bg|paper|paper-soft|surface-[a-z]+|user-bubble|overlay|primary-bg|primary-ink|danger|danger-soft|warning-soft|code-bg|focus-ring|accent-soft|shadow)(?![\w-])/g;
const largeIconSize = /\b(?:width|height|inline-size|block-size)\s*:\s*(?:(?:[2-9]\d|\d{3,})(?:\.\d+)?px|var\(--gg-(?:size-control-(?:sm|md|lg)|size-field|space-(?:[5-9]|10))\))/gi;
const colors = /#[\da-f]{8}\b|#[\da-f]{6}\b|#[\da-f]{4}\b|#[\da-f]{3}\b|\b(?:rgba?|hsla?|oklch)\s*\(/gi;

export function inspectDesignTokens(file, text) {
  file = file.replaceAll("\\", "/");
  const css = file.endsWith(".css");
  const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true);
  const exceptions = new Set();
  const comments = [];
  function comment(start, end) {
    comments.push([start, end]);
    const value = text.slice(start, end);
    // The reason must be on the same physical line as the flagged value.
    for (const match of value.matchAll(/(?:\/\*|\/\/)\s*token-exception:\s*([^\r\n]*?)(?:\*\/|$)/g)) {
      if (match[1].trim()) exceptions.add(source.getLineAndCharacterOfPosition(start + match.index).line + 1);
    }
  }
  if (css) {
    for (const match of text.matchAll(/\/\*[\s\S]*?\*\//g)) comment(match.index, match.index + match[0].length);
  } else {
    // Use parsed node boundaries so template substitutions and regex literals
    // cannot confuse a standalone scanner into treating string text as comments.
    const seen = new Set();
    function collect(node) {
      const ranges = [...(ts.getLeadingCommentRanges(text, node.pos) ?? []), ...(ts.getTrailingCommentRanges(text, node.end) ?? [])];
      if (ts.isJsxExpression(node)) ranges.push(...(ts.getTrailingCommentRanges(text, node.getStart(source) + 1) ?? []));
      for (const range of ranges) {
        if (!seen.has(range.pos)) { seen.add(range.pos); comment(range.pos, range.end); }
      }
      ts.forEachChild(node, collect);
    }
    collect(source);
  }
  let clean = text;
  for (const [start, end] of comments.sort((a, b) => b[0] - a[0])) clean = clean.slice(0, start) + clean.slice(start, end).replace(/[^\r\n]/g, " ") + clean.slice(end);
  const findings = [];
  function report(rule, position, value) {
    const line = source.getLineAndCharacterOfPosition(position).line + 1;
    if (rule !== "design-tokens/legacy-alias" && exceptions.has(line)) return;
    findings.push({ file, line, rule, value });
  }
  for (const match of clean.matchAll(legacy)) report("design-tokens/legacy-alias", match.index, match[0]);
  // Aliases are forbidden even in the token file; raw token definitions are not.
  if (file === "src/styles/tokens.css") return findings;
  function scanColors(value, offset) {
    for (const match of value.matchAll(colors)) report("design-tokens/raw-color", offset + match.index, match[0]);
  }
  function scanCss(value, offset) {
    scanColors(value, offset);
    for (const match of value.matchAll(/\bz-index\s*:\s*([+-]?\d+(?:\.\d+)?)(?![\w.%])/gi)) report("design-tokens/literal-z-index", offset + match.index, match[0]);
    // Owner rule (tokens.md §5.1): icons are functional and small; an svg sized
    // at 20px or more is a decorative icon unless the line carries an exception.
    for (const rule of value.matchAll(/([^{}]*)\{([^{}]*)\}/g)) {
      if (!/\bsvg\b/i.test(rule[1])) continue;
      const body = offset + rule.index + rule[1].length + 1;
      for (const match of rule[2].matchAll(largeIconSize)) report("design-tokens/decorative-icon-size", body + match.index, match[0].trim());
    }
  }
  if (css) scanCss(clean, 0);
  else {
    function visit(node) {
      if (ts.isStringLiteralLike(node) || ts.isTemplateHead(node) || ts.isTemplateMiddle(node) || ts.isTemplateTail(node)) {
        scanCss(clean.slice(node.getStart(source), node.end), node.getStart(source));
      }
      if (ts.isPropertyAssignment(node) && ["zIndex", "z-index"].includes(node.name.text)) {
        const value = node.initializer.getText(source);
        if (/^(?:[+-]?\d+(?:\.\d+)?|["'][+-]?\d+(?:\.\d+)?["'])$/.test(value)) report("design-tokens/literal-z-index", node.initializer.getStart(source), value);
      }
      ts.forEachChild(node, visit);
    }
    visit(source);
  }
  return findings.sort((a, b) => a.line - b.line || a.rule.localeCompare(b.rule));
}

export async function checkDesignTokens(root) {
  const findings = [];
  async function scan(directory) {
    for (const entry of (await readdir(join(root, directory), { withFileTypes: true })).sort((a, b) => a.name.localeCompare(b.name))) {
      const file = `${directory}/${entry.name}`;
      if (entry.isDirectory()) await scan(file);
      // Tests assert concrete computed values, so they may contain raw colors and sizes.
      else if (/\.(?:css|tsx?)$/.test(file) && !/\.test\.tsx?$/.test(file)) findings.push(...inspectDesignTokens(file, await readFile(join(root, file), "utf8")));
    }
  }
  await scan("src");
  return findings;
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const mode = process.argv[2] ?? "--report";
  if (!["--report", "--enforce"].includes(mode) || process.argv.length > 3) {
    console.error("Usage: check-design-tokens.mjs [--report|--enforce]");
    process.exitCode = 2;
  } else {
    const findings = await checkDesignTokens(resolve(dirname(fileURLToPath(import.meta.url)), ".."));
    for (const hit of findings) console.log(`${hit.file}:${hit.line}: ${hit.rule}: ${hit.value}`);
    console.log(`Design tokens (${mode.slice(2)}): ${findings.length} violation(s)`);
    if (mode === "--enforce" && findings.length) process.exitCode = 1;
  }
}
