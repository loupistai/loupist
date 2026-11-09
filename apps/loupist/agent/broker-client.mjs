import { closeSync, readFileSync, readSync, unlinkSync } from "node:fs";
import net from "node:net";

const PROTOCOL = "loupist-tool/1";
const MAX_TOKEN_BYTES = 512;
// Keep this equal to the Rust broker's per-frame ceiling. Counting resets after
// each newline-delimited response.
const MAX_RESPONSE_FRAME_BYTES = 5 * 1024 * 1024;

// Exit codes let the host tell which post-ACK failure ended the run.
const INDETERMINATE_EXIT_CODES = new Map([
  ["Tool broker closed before committing the response", 71],
  ["Tool broker returned a malformed commit barrier", 72],
  ["Tool broker connection failed", 73],
  ["Tool broker closed without a response", 74],
  ["Tool broker returned more than one response", 75],
]);

let indeterminateDeliveryHandler = (error) => {
  process.stderr.write(`Loupist broker delivery became indeterminate: ${error.message}\n`);
  process.exit(INDETERMINATE_EXIT_CODES.get(error.message) ?? 70);
};

// A lost commit barrier after the client ACK cannot be represented as an
// ordinary Tool failure: Rust may already have committed the turn-local
// result. Production therefore terminates the agent run so its entire
// ToolCapture is discarded. Tests replace the non-returning exit handler.
export function setIndeterminateDeliveryHandlerForTests(handler) {
  const previous = indeterminateDeliveryHandler;
  indeterminateDeliveryHandler = handler;
  return () => {
    indeterminateDeliveryHandler = previous;
  };
}

let brokerConnection;

/**
 * Read one newline-terminated token line without waiting for EOF: another
 * process that briefly inherited the pipe's write end could hold EOF back.
 */
function readTokenLine(descriptor) {
  const buffer = Buffer.alloc(MAX_TOKEN_BYTES + 1);
  let length = 0;
  while (length < buffer.length) {
    const read = readSync(descriptor, buffer, length, buffer.length - length, null);
    if (read === 0) break;
    const end = buffer.subarray(length, length + read).indexOf(0x0a);
    if (end !== -1) {
      if (end + 1 !== read) {
        throw new Error("Loupist tool broker token is invalid: data follows the token line");
      }
      return buffer.subarray(0, length + end).toString("utf8");
    }
    length += read;
  }
  throw new Error(`Loupist tool broker token is invalid: expected one line of at most ${MAX_TOKEN_BYTES} bytes`);
}

/**
 * Take the per-turn broker token once, at extension load. Rust passes it on
 * an inherited pipe (`LOUPIST_BROKER_TOKEN_FD`, one newline-terminated line)
 * or, off Unix, a one-shot file, never in the environment or argv that
 * same-user processes can read. The descriptor is closed here, before any
 * native tool runs, so no child process inherits it.
 */
export function loadBrokerConnection(env = process.env) {
  const socketPath = env.LOUPIST_BROKER_SOCKET;
  const descriptor = env.LOUPIST_BROKER_TOKEN_FD;
  const file = env.LOUPIST_BROKER_TOKEN_FILE;
  delete env.LOUPIST_BROKER_TOKEN_FD;
  delete env.LOUPIST_BROKER_TOKEN_FILE;
  brokerConnection = undefined;
  let token;
  if (descriptor !== undefined) {
    if (!/^[0-9]{1,4}$/u.test(descriptor) || Number(descriptor) < 3) {
      throw new Error("Loupist tool broker token is invalid");
    }
    try {
      token = readTokenLine(Number(descriptor));
    } finally {
      closeSync(Number(descriptor));
    }
  } else if (file !== undefined) {
    try {
      token = readFileSync(file, "utf8");
    } finally {
      try {
        unlinkSync(file);
      } catch {
        // Rust removes a remaining file when Pi exits.
      }
    }
  } else {
    return false;
  }
  if (
    !socketPath ||
    !token ||
    Buffer.byteLength(token, "utf8") > MAX_TOKEN_BYTES ||
    /[\0\r\n]/u.test(token)
  ) {
    throw new Error("Loupist tool broker token is invalid");
  }
  brokerConnection = { socketPath, token };
  return true;
}

function brokerConfig() {
  if (!brokerConnection) {
    throw new Error("Loupist tool broker is unavailable");
  }
  return brokerConnection;
}

function hasExactKeys(value, expected) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const keys = Object.keys(value);
  return (
    keys.length === expected.length &&
    expected.every((key) => Object.hasOwn(value, key))
  );
}

export function requestBroker(method, params, signal, callId) {
  if (signal?.aborted) {
    return Promise.reject(new Error("Tool call aborted"));
  }
  const { socketPath, token } = brokerConfig();
  const id = crypto.randomUUID();

  return new Promise((resolve, reject) => {
    let settled = false;
    let received = "";
    let terminal;
    let deliveryAcknowledged = false;
    const socket = net.createConnection({ path: socketPath, allowHalfOpen: true });

    const finish = (error, value) => {
      if (settled) return;
      settled = true;
      signal?.removeEventListener("abort", onAbort);
      socket.destroy();
      if (error) reject(error);
      else resolve(value);
    };

    const fail = (error) => {
      if (deliveryAcknowledged) indeterminateDeliveryHandler(error);
      finish(error);
    };

    const onAbort = () => {
      if (!deliveryAcknowledged) finish(new Error("Tool call aborted"));
    };
    signal?.addEventListener("abort", onAbort, { once: true });
    if (signal?.aborted) {
      onAbort();
      return;
    }
    socket.setEncoding("utf8");
    socket.on("connect", () => {
      const request = {
        protocol: PROTOCOL,
        id,
        token,
        method,
        params,
        ...(typeof callId === "string" && callId ? { call_id: callId } : {}),
      };
      socket.write(`${JSON.stringify(request)}\n`);
    });
    socket.on("data", (chunk) => {
      received += chunk;
      while (!settled) {
        const newline = received.indexOf("\n");
        if (newline < 0) {
          if (Buffer.byteLength(received, "utf8") > MAX_RESPONSE_FRAME_BYTES) {
            fail(new Error("Tool broker response exceeded the size limit"));
          }
          return;
        }
        const frame = received.slice(0, newline);
        received = received.slice(newline + 1);
        if (Buffer.byteLength(frame, "utf8") > MAX_RESPONSE_FRAME_BYTES) {
          fail(new Error("Tool broker response exceeded the size limit"));
          return;
        }

        let parsed;
        try {
          parsed = JSON.parse(frame);
        } catch {
          fail(new Error("Tool broker returned malformed JSON"));
          return;
        }

        if (deliveryAcknowledged) {
          if (
            hasExactKeys(parsed, ["protocol", "id", "committed"]) &&
            parsed.protocol === PROTOCOL &&
            parsed.id === id &&
            parsed.committed === true
          ) {
            finish(terminal.error, terminal.value);
          } else {
            fail(new Error("Tool broker returned a malformed commit barrier"));
          }
          return;
        }

        const isError = hasExactKeys(parsed, ["protocol", "id", "ok", "error"]);
        const isResult = hasExactKeys(parsed, ["protocol", "id", "ok", "result"]);
        if (!isError && !isResult) {
          fail(new Error("Tool broker returned a malformed response"));
        } else if (parsed.protocol !== PROTOCOL || parsed.id !== id) {
          fail(new Error("Tool broker response identity mismatch"));
        } else if (isError) {
          if (parsed.ok !== false) {
            fail(new Error("Tool broker returned a malformed response"));
          } else {
            const detail = parsed.error ?? {};
            const message = typeof detail.message === "string" && detail.message
              ? detail.message
              : "Tool broker rejected the call";
            const next = typeof detail.next === "string" && detail.next ? ` ${detail.next}` : "";
            terminal = {
              error: new Error(`${message}${next}`),
            };
          }
        } else if (parsed.ok !== true) {
          fail(new Error("Tool broker returned a malformed response"));
        } else {
          terminal = { value: parsed.result };
        }
        if (!settled && terminal) {
          if (received.length > 0) {
            fail(new Error("Tool broker returned more than one response"));
            return;
          }
          deliveryAcknowledged = true;
          signal?.removeEventListener("abort", onAbort);
          // Write the ACK without half-closing. `socket.end(ack)` can deliver
          // FIN before the ACK line on Unix sockets, so Rust never reaches
          // the commit barrier and the client exits 71.
          socket.write(`${JSON.stringify({
            protocol: PROTOCOL,
            id,
            delivered: true,
          })}\n`);
          return;
        }
      }
    });
    socket.on("error", () => fail(new Error("Tool broker connection failed")));
    // Complete frames are consumed by the data handler, so an end before
    // settlement always means the commit barrier never arrived.
    socket.on("end", () => {
      if (!settled) fail(new Error("Tool broker closed before committing the response"));
    });
    socket.on("close", () => {
      if (!settled) fail(new Error("Tool broker closed without a response"));
    });
  });
}
