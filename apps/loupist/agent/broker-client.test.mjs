import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, fstatSync, mkdtempSync, openSync, rmSync, writeFileSync } from "node:fs";
import net from "node:net";
import { randomUUID } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  loadBrokerConnection,
  requestBroker,
  setIndeterminateDeliveryHandlerForTests,
} from "./broker-client.mjs";

/** Hand the client a token line on an open descriptor, as Rust does at Pi launch. */
function loadTokenFromDescriptor(temporary, socketPath, token) {
  const path = join(temporary, `token-${randomUUID()}`);
  writeFileSync(path, `${token}\n`, { mode: 0o600 });
  const descriptor = openSync(path, "r");
  const env = { LOUPIST_BROKER_SOCKET: socketPath, LOUPIST_BROKER_TOKEN_FD: String(descriptor) };
  assert.equal(loadBrokerConnection(env), true);
  return { descriptor, env };
}

async function setupBroker(t, respond) {
  const temporary = mkdtempSync(join(tmpdir(), "loupist-broker-protocol-"));
  const socketPath =
    process.platform === "win32"
      ? `\\\\.\\pipe\\loupist-broker-protocol-${randomUUID()}`
      : join(temporary, "broker.sock");
  loadTokenFromDescriptor(temporary, socketPath, "broker-test-token");

  const sockets = new Set();
  const server = net.createServer((socket) => {
    sockets.add(socket);
    socket.once("close", () => sockets.delete(socket));
    let buffered = "";
    socket.setEncoding("utf8");
    socket.on("data", (chunk) => {
      buffered += chunk;
      const newline = buffered.indexOf("\n");
      if (newline < 0) return;
      const request = JSON.parse(buffered.slice(0, newline));
      respond(socket, request);
    });
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(socketPath, resolve);
  });
  t.after(async () => {
    for (const socket of sockets) socket.destroy();
    await new Promise((resolve) => server.close(resolve));
    rmSync(temporary, { recursive: true, force: true });
  });
  return (signal = new AbortController().signal) =>
    requestBroker("world.search", { query: "test" }, signal);
}

function sendTerminalAfterAck(socket, request, response, onAck = () => {}) {
  let buffered = "";
  const onData = (chunk) => {
    buffered += chunk;
    const newline = buffered.indexOf("\n");
    if (newline < 0) return;
    const ack = JSON.parse(buffered.slice(0, newline));
    assert.deepEqual(ack, {
      protocol: "loupist-tool/1",
      id: request.id,
      delivered: true,
    });
    socket.off("data", onData);
    onAck();
    socket.end(`${JSON.stringify({
      protocol: "loupist-tool/1",
      id: request.id,
      committed: true,
    })}\n`);
  };
  socket.removeAllListeners("data");
  socket.on("data", onData);
  socket.write(`${JSON.stringify(response)}\n`);
}

test("does not connect for an already-aborted call", async (t) => {
  let connected = false;
  const execute = await setupBroker(t, () => {
    connected = true;
  });
  const controller = new AbortController();
  controller.abort();
  await assert.rejects(execute(controller.signal), /Tool call aborted/);
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(connected, false);
});
test("settles a commit barrier that arrives after a delayed ACK write", async (t) => {
  const execute = await setupBroker(t, (socket, request) => {
    sendTerminalAfterAck(
      socket,
      request,
      {
        protocol: "loupist-tool/1",
        id: request.id,
        ok: true,
        result: { records: ["delayed-commit"] },
      },
      () => {
        Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 50);
      },
    );
  });
  assert.deepEqual(await execute(), { records: ["delayed-commit"] });
});

test("accepts one terminal result response", async (t) => {
  const tokens = [];
  const execute = await setupBroker(t, (socket, request) => {
    tokens.push(request.token);
    sendTerminalAfterAck(socket, request, {
      protocol: "loupist-tool/1",
      id: request.id,
      ok: true,
      result: { records: ["lens-quality"] },
    });
  });
  assert.deepEqual(await execute(), { records: ["lens-quality"] });
  assert.deepEqual(tokens, ["broker-test-token"]);
});

test("takes the broker token once from its descriptor and closes it", (t) => {
  const temporary = mkdtempSync(join(tmpdir(), "loupist-broker-token-"));
  t.after(() => rmSync(temporary, { recursive: true, force: true }));
  const { descriptor, env } = loadTokenFromDescriptor(temporary, "broker.sock", "one-turn-token");
  assert.throws(() => fstatSync(descriptor), { code: "EBADF" });
  assert.deepEqual(env, { LOUPIST_BROKER_SOCKET: "broker.sock" });

  for (const token of ["", "\n", "no-newline", "two\nlines", "x".repeat(513), `${"x".repeat(513)}\n`]) {
    const path = join(temporary, "invalid");
    writeFileSync(path, token);
    const invalid = openSync(path, "r");
    assert.throws(
      () => loadBrokerConnection({ LOUPIST_BROKER_SOCKET: "broker.sock", LOUPIST_BROKER_TOKEN_FD: String(invalid) }),
      /token is invalid/,
    );
    assert.throws(() => fstatSync(invalid), { code: "EBADF" });
  }
  const longest = loadTokenFromDescriptor(temporary, "broker.sock", "x".repeat(512));
  assert.throws(() => fstatSync(longest.descriptor), { code: "EBADF" });
  for (const descriptorText of ["0", "2", "-1", "3x"]) {
    assert.throws(
      () => loadBrokerConnection({ LOUPIST_BROKER_SOCKET: "broker.sock", LOUPIST_BROKER_TOKEN_FD: descriptorText }),
      /token is invalid/,
    );
  }

  // Off Unix, Rust writes a one-shot file instead; it is gone after load.
  const file = join(temporary, "token-file");
  writeFileSync(file, "file-token", { mode: 0o600 });
  assert.equal(loadBrokerConnection({ LOUPIST_BROKER_SOCKET: "broker.sock", LOUPIST_BROKER_TOKEN_FILE: file }), true);
  assert.equal(existsSync(file), false);

  // Without a handoff the client stays unavailable instead of reading the environment.
  assert.equal(loadBrokerConnection({ LOUPIST_BROKER_SOCKET: "broker.sock", LOUPIST_BROKER_TOKEN: "env-token" }), false);
  assert.throws(() => requestBroker("world.search", {}, undefined), /broker is unavailable/);
});

test("a child process inherits no broker descriptor after load", { skip: process.platform === "win32", timeout: 15000 }, async () => {
  const clientUrl = new URL("./broker-client.mjs", import.meta.url).href;
  const script = `
    import { fstatSync } from "node:fs";
    import { spawnSync } from "node:child_process";
    const { loadBrokerConnection } = await import(${JSON.stringify(clientUrl)});
    const loaded = loadBrokerConnection();
    let closed = false;
    try { fstatSync(3); } catch (error) { closed = error.code === "EBADF"; }
    const child = spawnSync("/bin/sh", ["-c", "cat <&3"], { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
    process.stdout.write(JSON.stringify({ loaded, closed, env: JSON.stringify(process.env), grandchild: child.stdout }));
  `;
  const child = spawn(process.execPath, ["--input-type=module", "-e", script], {
    env: { PATH: process.env.PATH, LOUPIST_BROKER_SOCKET: "broker.sock", LOUPIST_BROKER_TOKEN_FD: "3" },
    stdio: ["ignore", "pipe", "pipe", "pipe"],
  });
  // Keep the write end open, as a process that inherited it before
  // close-on-exec would: the client must stop at the newline, not wait for EOF.
  child.stdio[3].write("pipe-token-7d2a\n");
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (chunk) => { stdout += chunk; });
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  const closed = new Promise((resolve) => child.on("close", resolve));
  const status = await new Promise((resolve) => child.on("exit", resolve));
  child.stdio[3].destroy();
  await closed;
  assert.equal(status, 0, stderr);
  const observed = JSON.parse(stdout);
  assert.equal(observed.loaded, true);
  assert.equal(observed.closed, true);
  assert.equal(observed.env.includes("pipe-token-7d2a"), false);
  assert.equal(observed.grandchild.includes("pipe-token-7d2a"), false);
});

test("rejects one terminal error response", async (t) => {
  const execute = await setupBroker(t, (socket, request) => {
    sendTerminalAfterAck(socket, request, {
      protocol: "loupist-tool/1",
      id: request.id,
      ok: false,
      error: { code: "method_denied", message: "broker denied the call" },
    });
  });
  await assert.rejects(execute(), /broker denied the call/);
});

test("rejects a delivered response when the commit barrier never arrives", async (t) => {
  const fatal = [];
  const restore = setIndeterminateDeliveryHandlerForTests((error) => fatal.push(error.message));
  t.after(restore);
  const execute = await setupBroker(t, (socket, request) => {
    let buffered = "";
    socket.removeAllListeners("data");
    socket.on("data", (chunk) => {
      buffered += chunk;
      if (buffered.includes("\n")) socket.end();
    });
    socket.write(`${JSON.stringify({
      protocol: "loupist-tool/1",
      id: request.id,
      ok: true,
      result: { records: [] },
    })}\n`);
  });
  await assert.rejects(execute(), /before committing/);
  assert.deepEqual(fatal, ["Tool broker closed before committing the response"]);
});

test("makes a malformed post-ACK commit barrier turn-fatal", async (t) => {
  const fatal = [];
  const restore = setIndeterminateDeliveryHandlerForTests((error) => fatal.push(error.message));
  t.after(restore);
  const execute = await setupBroker(t, (socket, request) => {
    socket.removeAllListeners("data");
    socket.once("data", () => {
      socket.end(`${JSON.stringify({
        protocol: "loupist-tool/1",
        id: request.id,
        committed: false,
      })}\n`);
    });
    socket.write(`${JSON.stringify({
      protocol: "loupist-tool/1",
      id: request.id,
      ok: true,
      result: { records: [] },
    })}\n`);
  });
  await assert.rejects(execute(), /malformed commit barrier/);
  assert.deepEqual(fatal, ["Tool broker returned a malformed commit barrier"]);
});

test("rejects legacy acknowledgement fields and malformed identities", async (t) => {
  await t.test("legacy phase", async (t) => {
    const execute = await setupBroker(t, (socket, request) => {
      socket.end(`${JSON.stringify({
        protocol: "loupist-tool/1",
        id: request.id,
        phase: "result",
        ok: true,
        result: {},
      })}\n`);
    });
    await assert.rejects(execute(), /malformed response/);
  });
  await t.test("wrong id", async (t) => {
    const execute = await setupBroker(t, (socket) => {
      socket.end(`${JSON.stringify({
        protocol: "loupist-tool/1",
        id: "wrong",
        ok: true,
        result: {},
      })}\n`);
    });
    await assert.rejects(execute(), /identity mismatch/);
  });
});

test("accepts a large bounded result and rejects an oversized frame", async (t) => {
  await t.test("bounded", async (t) => {
    const text = "x".repeat(2 * 1024 * 1024);
    const execute = await setupBroker(t, (socket, request) => {
      sendTerminalAfterAck(socket, request, {
        protocol: "loupist-tool/1",
        id: request.id,
        ok: true,
        result: { text },
      });
    });
    assert.equal((await execute()).text.length, text.length);
  });
  await t.test("oversized", async (t) => {
    const execute = await setupBroker(t, (socket, request) => {
      socket.end(`${JSON.stringify({
        protocol: "loupist-tool/1",
        id: request.id,
        ok: true,
        result: { text: "x".repeat(5 * 1024 * 1024) },
      })}\n`);
    });
    await assert.rejects(execute(), /exceeded the size limit/);
  });
});
