// The Vite listener owns its ephemeral port until the suite shuts it down.
import { createServer as createViteServer } from "vite";
import { createServer } from "node:http";
import { realpathSync } from "node:fs";
import { once } from "node:events";
import { readFile, rename, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const appRoot = dirname(dirname(fileURLToPath(import.meta.url)));

export async function startDevServer(port = 0, config = {}) {
  if (!Number.isInteger(port) || (port !== 0 && (port < 1024 || port > 65535 || port === 1420))) {
    throw new Error("LOUPIST_E2E_DEV_PORT must be 0 or an integer from 1024 to 65535 other than 1420.");
  }
  const listener = createServer();
  const vite = await createViteServer({
    root: appRoot, ...config,
    server: { ...config.server, middlewareMode: true, hmr: { server: listener } },
  });
  listener.on("request", vite.middlewares);
  const close = async () => {
    listener.closeAllConnections();
    if (listener.listening) await new Promise((done, reject) => listener.close((error) => error ? reject(error) : done()));
    await vite.close();
  };
  try {
    // Vite 7 treats port 0 as its default. Its documented middleware mode lets
    // our HTTP server bind 0 directly while Vite serves and transforms the app.
    listener.listen(port, "127.0.0.1");
    await once(listener, "listening");
    const address = listener.address();
    return { server: { close }, url: `http://127.0.0.1:${address.port}` };
  } catch (error) {
    await close();
    throw error;
  }
}

export async function waitForDevServer(pid, receipt, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    process.kill(pid, 0); // Fail immediately when this suite's server exited.
    try {
      const value = JSON.parse(await readFile(receipt, "utf8"));
      if (value.pid !== pid || !/^http:\/\/127\.0\.0\.1:\d+$/u.test(value.url)) throw new Error("Invalid Vite receipt");
      return value.url;
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
    await new Promise((done) => setTimeout(done, 100));
  }
  throw new Error("Timed out waiting for this suite's Vite server");
}

if (process.argv[1] && realpathSync(resolve(process.argv[1])) === fileURLToPath(import.meta.url)) {
  const [mode, value, receipt, timeout] = process.argv.slice(2);
  if (mode === "start") {
    const { server, url } = await startDevServer(Number(value));
    // Atomic publication prevents the waiter from reading a partial JSON record.
    await writeFile(`${receipt}.tmp`, JSON.stringify({ pid: process.pid, url }), { mode: 0o600 });
    await rename(`${receipt}.tmp`, receipt);
    for (const signal of ["SIGTERM", "SIGINT", "SIGHUP"]) {
      process.once(signal, async () => { await server.close(); process.exit(0); });
    }
  } else if (mode === "wait") {
    console.log(await waitForDevServer(Number(value), receipt, Number(timeout)));
  } else {
    throw new Error("usage: dev-server.mjs start <port> <receipt> | wait <pid> <receipt> <timeout-ms>");
  }
}
