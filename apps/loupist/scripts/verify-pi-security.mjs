import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import net from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath, pathToFileURL } from "node:url";
import { gzipSync } from "node:zlib";

const scripts = dirname(fileURLToPath(import.meta.url));

export async function withSecurityServer(run) {
  const sockets = new Set();
  const compressed = gzipSync(JSON.stringify({ ok: true }));
  const server = net.createServer((socket) => {
    sockets.add(socket);
    socket.on("error", () => {});
    socket.on("close", () => sockets.delete(socket));
    socket.once("data", (request) => {
      if (request.toString().startsWith("GET /excessive-encodings ")) {
        socket.end("HTTP/1.1 200 OK\r\nContent-Encoding: gzip, gzip, gzip, gzip, gzip, gzip\r\nContent-Length: 100\r\nConnection: close\r\n\r\n");
      } else {
        socket.end(Buffer.concat([
          Buffer.from(`HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\nContent-Length: ${compressed.length}\r\nConnection: close\r\n\r\n`),
          compressed,
        ]));
      }
    });
  });
  await new Promise((done, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", done); });
  try {
    return await run(`http://127.0.0.1:${server.address().port}`);
  } finally {
    for (const socket of sockets) socket.destroy();
    await new Promise((done) => server.close(done));
  }
}

export async function verifyPiSecurity(executable, runtime) {
  const temporary = mkdtempSync(join(tmpdir(), "loupist-pi-security-"));
  const resultFile = join(temporary, "result.json");
  try {
    return await withSecurityServer(async (baseUrl) => {
      // Do not inherit provider credentials, user config, proxy or preload hooks.
      const env = Object.fromEntries(["PATH", "SystemRoot", "WINDIR", "TEMP", "TMP", "TMPDIR"]
        .filter((name) => process.env[name]).map((name) => [name, process.env[name]]));
      Object.assign(env, {
        HOME: temporary, USERPROFILE: temporary, PI_CODING_AGENT_DIR: join(temporary, "agent"),
        PI_PACKAGE_DIR: resolve(runtime), PI_SKIP_VERSION_CHECK: "1", PI_TELEMETRY: "0",
        GEMINI_API_KEY: "loupist-security-fixture", LOUPIST_SECURITY_URL: baseUrl,
        LOUPIST_SECURITY_RESULT: resultFile,
      });
      const child = spawn(resolve(executable), [
        "--mode", "rpc", "--no-session", "--no-builtin-tools", "--no-extensions",
        "--extension", join(scripts, "fixtures/pi-security-extension.mjs"),
        "--no-skills", "--no-prompt-templates", "--no-themes", "--no-context-files",
        "--provider", "google", "--model", "gemini-2.5-flash",
      ], { cwd: temporary, env, stdio: ["pipe", "pipe", "pipe"] });
      let failure;
      let diagnostics = "";
      child.on("error", (error) => { failure = error; });
      child.stdin.on("error", () => {});
      for (const stream of [child.stdout, child.stderr]) {
        stream.on("data", (data) => { diagnostics = (diagnostics + data.toString()).slice(-4_000); });
      }
      const exited = new Promise((done) => child.once("close", done));
      child.stdin.write(`${JSON.stringify({ id: "security-probe", type: "prompt", message: "/loupist-security-probe" })}\n`);
      try {
        const deadline = Date.now() + 20_000;
        while (!existsSync(resultFile) && Date.now() < deadline && child.exitCode === null && !failure) await delay(25);
        if (failure) throw failure;
        assert.ok(existsSync(resultFile), `Pi security probe did not finish: ${diagnostics}`);
        const result = JSON.parse(readFileSync(resultFile, "utf8"));
        assert.deepEqual(result, {
          patterns: "passed", compressedHttp: "passed", recovery: "passed", httpImplementation: "bun-builtin",
        });
        assert.equal(child.exitCode, null, "Pi exited after rejecting the malicious response");
        return result;
      } finally {
        child.kill();
        const timer = setTimeout(() => child.kill("SIGKILL"), 1_000);
        await exited;
        clearTimeout(timer);
      }
    });
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [executable, runtime] = process.argv.slice(2);
  if (!executable || !runtime) throw new Error("Usage: node verify-pi-security.mjs <executable> <runtime>");
  console.log(JSON.stringify(await verifyPiSecurity(executable, runtime)));
}
