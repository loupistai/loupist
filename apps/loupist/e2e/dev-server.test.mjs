import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { startDevServer, waitForDevServer } from "./dev-server.mjs";

test("Vite keeps each OS-assigned port bound and reports the actual serving URL", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "loupist-vite-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  await writeFile(join(root, "index.html"), "<p>owned test server</p>");
  const config = { root, configFile: false, logLevel: "silent", server: { watch: null }, optimizeDeps: { noDiscovery: true } };
  const first = await startDevServer(0, config);
  t.after(() => first.server.close());
  const second = await startDevServer(0, config);
  t.after(() => second.server.close());
  assert.notEqual(first.url, second.url);
  assert.match(await (await fetch(first.url)).text(), /owned test server/);
  assert.match(await (await fetch(second.url)).text(), /owned test server/);
  assert.equal((await fetch(`${first.url}/@vite/client`)).status, 200);
  const port = Number(new URL(first.url).port);
  await assert.rejects(startDevServer(port, config), /EADDRINUSE/);
  const receipt = join(root, "ready.json");
  await writeFile(receipt, JSON.stringify({ pid: process.pid, url: first.url }));
  assert.equal(await waitForDevServer(process.pid, receipt, 10_000), first.url);
  await writeFile(receipt, JSON.stringify({ pid: process.pid + 1, url: first.url }));
  await assert.rejects(waitForDevServer(process.pid, receipt, 10_000), /Invalid Vite receipt/);
});

test("explicit dev ports reject invalid values and the shared development port", async () => {
  for (const port of [-1, 1, 1420, 65536, NaN, 2.5]) {
    await assert.rejects(startDevServer(port), /LOUPIST_E2E_DEV_PORT/);
  }
});
