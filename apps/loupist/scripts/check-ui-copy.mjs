#!/usr/bin/env node
import ts from "typescript";
import { realpathSync } from "node:fs";
import { readdir, readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const jargon = /\b(revision id|digest|receipt|envelope|byte offset|span selector|worker|sidecar|stdio|JSON-RPC)\b/gi;
const advice = {
  en: /\b(you should (buy|sell|hold)|we recommend|our recommendation|buy now|sell now|guaranteed returns?)\b/gi,
  ko: /(매수하세요|매도하세요|추천 종목|수익 보장|꼭 사세요|꼭 파세요)/g,
};
// A Korean particle depends on the word before it. After a placeholder, write
// both forms for the koreanParticles post-processor ("{{name}}이(가)") or
// reword; a single bare form is wrong for half the values. The app's own name
// ends in a vowel: 루피스트가, 루피스트를, 루피스트는, 루피스트와, 루피스트로.
const particle = {
  ko: /\}\}[”’"']?(?:이|가|은|는|을|를|와|과|으로|로|이에요|예요)(?![가-힣(])|루피스트(?:이|은|을|과|으로)(?![가-힣])/g,
};

export function validateAllowlist(entries) {
  if (!Array.isArray(entries) || entries.some((entry) =>
    !entry || typeof entry.key !== "string" || !entry.key.trim()
    || typeof entry.term !== "string" || !entry.term.trim()
    || typeof entry.reason !== "string" || !entry.reason.trim()
    || !new RegExp(`^(?:${jargon.source})$`, "i").test(entry.term))) {
    throw new Error("UI copy allowlist entries require key, a banned jargon term, and a non-empty reason");
  }
  return entries;
}

// Keys include locale and namespace (en.settings.connection.label), so an
// exception cannot silently authorize the same term in another translation.
export function inspectCatalog(file, text, locale, namespace, entries = []) {
  validateAllowlist(entries);
  JSON.parse(text); // Fail closed on invalid JSON before walking its syntax tree.
  const source = ts.parseJsonText(file, text);
  const findings = [];
  function visit(node, keys) {
    if (ts.isStringLiteral(node)) {
      const key = [locale, namespace, ...keys].join(".");
      const rules = [["ui-copy/advice", advice[locale]], ["ui-copy/jargon", jargon], ["ui-copy/particle", particle[locale]]];
      for (const [rule, pattern] of rules.filter(([, pattern]) => pattern)) {
        for (const match of node.text.matchAll(pattern)) {
          const exception = rule === "ui-copy/jargon" && entries.find((entry) =>
            entry.key === key && entry.term.toLowerCase() === match[0].toLowerCase());
          findings.push({ file, line: source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1,
            rule, key, term: match[0], value: node.text, allowed: Boolean(exception), reason: exception?.reason });
        }
      }
    } else if (ts.isObjectLiteralExpression(node)) {
      for (const property of node.properties) {
        if (ts.isPropertyAssignment(property)) visit(property.initializer, [...keys, property.name.text]);
      }
    } else if (ts.isArrayLiteralExpression(node)) {
      node.elements.forEach((value, index) => visit(value, [...keys, String(index)]));
    }
  }
  for (const statement of source.statements) {
    if (ts.isExpressionStatement(statement)) visit(statement.expression, []);
  }
  return findings;
}

export async function checkUiCopy(root) {
  const entries = validateAllowlist(JSON.parse(await readFile(join(root, "scripts/ui-copy-allowlist.json"), "utf8")));
  const findings = [];
  for (const locale of ["en", "ko"]) {
    const directory = join(root, "src/i18n/locales", locale);
    for (const name of (await readdir(directory)).filter((name) => name.endsWith(".json")).sort()) {
      const file = `src/i18n/locales/${locale}/${name}`;
      findings.push(...inspectCatalog(file, await readFile(join(root, file), "utf8"), locale, name.slice(0, -5), entries));
    }
  }
  return findings;
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length > 2) {
    console.error("Usage: check-ui-copy.mjs");
    process.exitCode = 2;
  } else {
    const findings = await checkUiCopy(resolve(dirname(fileURLToPath(import.meta.url)), ".."));
    for (const hit of findings) console.log(`${hit.file}:${hit.line}: ${hit.rule} ${hit.key}: ${JSON.stringify(hit.term)} in ${JSON.stringify(hit.value)}${hit.allowed ? ` [allowlisted: ${hit.reason}]` : ""}`);
    const errors = findings.filter((hit) => !hit.allowed).length;
    console.log(`UI copy: ${findings.length} hit(s), ${errors} error(s), ${findings.length - errors} allowlisted`);
    if (errors) process.exitCode = 1;
  }
}
