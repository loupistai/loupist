import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { describeViolations, readViolationReports, RECORDER_PATH, releasePolicy, REPORT_PATH, serveReleaseRenderer } from "./release-renderer.mjs";

test("the release renderer serves pages under the shipped policy, records violations and keeps to its folder", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "loupist-release-renderer-test-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  await mkdir(join(root, "assets"));
  await writeFile(join(root, "index.html"), "<!doctype html><html><head><title>App</title></head><body></body></html>");
  await writeFile(join(root, "assets/app.js"), "export {};");
  const reports = join(root, "reports.jsonl");
  const server = await serveReleaseRenderer(root, { reportFile: reports });
  t.after(() => server.close());

  const page = await fetch(`${server.url}/`);
  // The policy is tauri.conf.json's, unchanged; the report-uri only says where to tell.
  assert.equal(page.headers.get("content-security-policy"), `${releasePolicy()}; report-uri ${REPORT_PATH}`);
  assert.doesNotMatch(releasePolicy(), /blob:|unsafe-eval/);
  // The recorder runs before every other script of the page.
  assert.match(await page.text(), new RegExp(`<head><script src="${RECORDER_PATH}"></script><title>`));
  assert.match(await (await fetch(`${server.url}${RECORDER_PATH}`)).text(), /securitypolicyviolation/);
  // Scripts and other assets carry no policy of their own, as Tauri serves them.
  assert.equal((await fetch(`${server.url}/assets/app.js`)).headers.get("content-security-policy"), null);
  // An escaping path resolves inside the folder, where nothing by that name exists.
  assert.equal((await fetch(`${server.url}/..%2f..%2fetc%2fpasswd`)).status, 404);
  assert.equal((await fetch(`${server.url}/missing.js`)).status, 404);

  const report = { "csp-report": { "document-uri": `${server.url}/`, "effective-directive": "img-src", "blocked-uri": "blob", "source-file": `${server.url}/assets/app.js`, "line-number": 3 } };
  assert.equal((await fetch(`${server.url}${REPORT_PATH}`, { method: "POST", headers: { "Content-Type": "application/csp-report" }, body: JSON.stringify(report, null, 2) })).status, 204);
  assert.equal((await readFile(reports, "utf8")).trim().split("\n").length, 1);
  const violations = readViolationReports(reports);
  assert.deepEqual(violations.map(({ directive, blocked }) => [directive, blocked]), [["img-src", "blob"]]);
  assert.deepEqual(describeViolations([...violations, ...violations]), [`img-src blocked blob from ${server.url}/assets/app.js:3 on /`]);
  assert.deepEqual(readViolationReports(join(root, "none.jsonl")), []);
});
