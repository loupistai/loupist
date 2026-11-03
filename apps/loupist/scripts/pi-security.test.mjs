import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { verifyPiDependencies } from "./verify-pi-dependencies.mjs";
import { withSecurityServer } from "./verify-pi-security.mjs";

test("Pi installs the patched dependencies instead of its published shrinkwrap", () => {
  verifyPiDependencies();
});

test("Pi model matching and HTTP survive hostile inputs and still handle normal responses", { timeout: 20_000 }, async () => {
  const { pi } = verifyPiDependencies();
  const scripts = dirname(fileURLToPath(import.meta.url));
  const code = `
    const { configureHttpDispatcher } = await import(${JSON.stringify(pathToFileURL(join(pi.root, "dist/core/http-dispatcher.js")).href)});
    const { resolveModelScopeWithDiagnostics } = await import(${JSON.stringify(pathToFileURL(join(pi.root, "dist/core/model-resolver.js")).href)});
    const { probePiSecurity } = await import(${JSON.stringify(pathToFileURL(join(scripts, "pi-security-probe.mjs")).href)});
    configureHttpDispatcher(1000);
    console.log(JSON.stringify(await probePiSecurity(resolveModelScopeWithDiagnostics, process.argv[1])));
  `;
  await withSecurityServer((url) => new Promise((done, reject) => {
    const child = spawn(process.execPath, ["--input-type=module", "-e", code, url], {
      env: { HTTP_PROXY: "", HTTPS_PROXY: "", ALL_PROXY: "", NO_PROXY: "127.0.0.1" },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let output = "";
    child.stdout.on("data", (data) => { output = (output + data).slice(-4_000); });
    child.stderr.on("data", (data) => { output = (output + data).slice(-4_000); });
    child.on("error", reject);
    const timer = setTimeout(() => child.kill("SIGKILL"), 15_000);
    child.on("close", (code) => {
      clearTimeout(timer);
      try { assert.equal(code, 0, output); assert.match(output, /"recovery":"passed"/u); done(); }
      catch (error) { reject(error); }
    });
  }));
});
