#!/bin/sh
set -eu
umask 077

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
# Keep one native GUI owner across checkouts and suite restarts.
if [ "${LOUPIST_GUI_LEASE_ENTRY:-}" != "$$:$0" ]; then
    exec python3 "$SCRIPT_DIR/native_ui_lease.py" "$0" "$@"
fi

if [ "$#" -ne 0 ]; then
    echo "usage: $0" >&2
    exit 2
fi

# The live Pi directory is presence-checked by the diagnostic. Do not print it.
if [ -z "${LOUPIST_LIVE_PI_AGENT_DATA_DIR:-}" ]; then
    LOUPIST_LIVE_PI_AGENT_DATA_DIR=$(node "$SCRIPT_DIR/env-diagnostic.mjs" --print-pi-dir)
fi
export LOUPIST_LIVE_PI_AGENT_DATA_DIR

# This suite uses the real account. The credential-free catalog marker stays off.
unset LOUPIST_E2E_PROVIDER_READY || true

# Model and effort defaults live in live-preset.mjs; optional overrides pass through.

E2E_STARTUP_TIMEOUT_MS=${LOUPIST_E2E_STARTUP_TIMEOUT_MS:-300000}
case "$E2E_STARTUP_TIMEOUT_MS" in
    *[!0-9]*|'')
        echo "LOUPIST_E2E_STARTUP_TIMEOUT_MS must be a positive integer." >&2
        exit 1
        ;;
esac

SESSION_INFO=
STATE_ROOT=
RUN_LOG=
APP_PID=

cleanup_app() {
    if [ -n "$APP_PID" ] && kill -0 "$APP_PID" 2>/dev/null; then
        kill "$APP_PID" 2>/dev/null || true
        wait "$APP_PID" 2>/dev/null || true
    fi
    APP_PID=
    rm -f -- "$SESSION_INFO"
}

wait_for_dev_server_exit() {
    LOUPIST_DEV_PORT=$1 node <<'NODE'
import net from "node:net";

const port = Number(process.env.LOUPIST_DEV_PORT);
const deadline = Date.now() + 15_000;
while (Date.now() < deadline) {
  let open = false;
  for (const host of ["127.0.0.1", "::1"]) {
    if (await new Promise((resolve) => {
      const socket = net.createConnection({ host, port });
      socket.setTimeout(200);
      socket.once("connect", () => { socket.destroy(); resolve(true); });
      const closed = () => { socket.destroy(); resolve(false); };
      socket.once("error", closed);
      socket.once("timeout", closed);
    })) {
      open = true;
      break;
    }
  }
  if (!open) process.exit(0);
  await new Promise((resolve) => setTimeout(resolve, 100));
}
throw new Error("Loupist dev server did not exit within 15 seconds");
NODE
}

cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    cleanup_app
    if [ -n "$RUN_LOG" ]; then
        if [ "$status" -ne 0 ]; then
            cat "$RUN_LOG" >&2
        fi
        rm -f -- "$RUN_LOG"
    fi
    if [ -n "$STATE_ROOT" ]; then
        rm -rf -- "$STATE_ROOT"
    fi
    exit "$status"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

STATE_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/loupist-live-journeys-e2e.XXXXXX")
SESSION_INFO="$STATE_ROOT/session.json"
RUN_LOG=$(mktemp "${TMPDIR:-/tmp}/loupist-live-journeys-log.XXXXXX")

# Journey 7 connects a custom provider from onboarding. It runs on an empty Pi
# directory inside the state root, so the real account directory is never written.
if [ "${LOUPIST_E2E_JOURNEYS:-}" = "7" ] || [ "${LOUPIST_E2E_JOURNEYS:-}" = "compaction" ]; then
    mkdir -m 700 "$STATE_ROOT/pi"
    LOUPIST_LIVE_PI_AGENT_DATA_DIR="$STATE_ROOT/pi"
fi

LOUPIST_E2E_STATE_DIR="$STATE_ROOT" \
    LOUPIST_E2E_SESSION_INFO="$SESSION_INFO" \
    LOUPIST_LIVE_PI_AGENT_DATA_DIR="$LOUPIST_LIVE_PI_AGENT_DATA_DIR" \
    "$SCRIPT_DIR/run-app.sh" >"$RUN_LOG" 2>&1 &
APP_PID=$!

if ! node "$SCRIPT_DIR/wait-session.mjs" \
    --pid "$APP_PID" \
    --session "$SESSION_INFO" \
    --timeout-ms "$E2E_STARTUP_TIMEOUT_MS"; then
    cat "$RUN_LOG" >&2
    echo "Loupist exited before the live journey session was ready." >&2
    exit 1
fi

DEV_PORT=$(node -e 'const s=JSON.parse(require("node:fs").readFileSync(process.argv[1],"utf8")); console.log(new URL(s.devUrl).port)' "$SESSION_INFO")

node "$SCRIPT_DIR/native-live-journeys.mjs" "$SESSION_INFO"
cleanup_app
# A reused dev server belongs to its owner and stays up.
if [ "${LOUPIST_E2E_REUSE_DEV_SERVER:-}" != "1" ]; then
    wait_for_dev_server_exit "$DEV_PORT"
fi
echo "Loupist native release journeys passed."
