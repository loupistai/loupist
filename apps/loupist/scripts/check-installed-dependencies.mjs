// Compare npm's installed inventory with the checked-in lock, without a network call.
import { existsSync, readFileSync, realpathSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

export function dependencyIssues(locked, installed) {
  const expected = locked.packages;
  const actual = installed.packages;
  if (!expected || !actual) return ["missing package inventory"];
  const issues = [];
  for (const [path, pkg] of Object.entries(expected)) {
    if (!path || pkg.optional || pkg.link) continue;
    if (!actual[path]) issues.push(`missing ${path}`);
  }
  for (const [path, pkg] of Object.entries(actual)) {
    if (!path) continue;
    const wanted = expected[path];
    if (!wanted || ["version", "resolved", "integrity", "link"].some((field) => pkg[field] !== wanted[field])) issues.push(`stale ${path}`);
  }
  return issues;
}

export function checkInstalled(directory) {
  const root = resolve(directory);
  const locked = JSON.parse(readFileSync(resolve(root, "package-lock.json"), "utf8"));
  const installed = JSON.parse(readFileSync(resolve(root, "node_modules/.package-lock.json"), "utf8"));
  const issues = dependencyIssues(locked, installed);
  for (const path of Object.keys(installed.packages ?? {})) {
    if (path && !existsSync(resolve(root, path))) issues.push(`missing ${path}`);
  }
  if (issues.length) throw new Error(`Locked dependencies differ in ${directory}: ${issues.slice(0, 5).join(", ")}; run setup.sh first`);
}

if (process.argv[1] && realpathSync(resolve(process.argv[1])) === fileURLToPath(import.meta.url)) {
  for (const directory of process.argv.slice(2)) checkInstalled(directory);
}
