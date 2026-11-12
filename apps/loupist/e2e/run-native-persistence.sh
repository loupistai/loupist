#!/bin/sh
set -eu
umask 077

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
# Keep one native GUI owner across checkouts and suite restarts.
if [ "${LOUPIST_GUI_LEASE_ENTRY:-}" != "$$:$0" ]; then
    exec python3 "$SCRIPT_DIR/native_ui_lease.py" "$0" "$@"
fi
STATE_ROOT=
SESSION_INFO=
RUN_LOG=
APP_PID=
E2E_STARTUP_TIMEOUT_MS=${LOUPIST_E2E_STARTUP_TIMEOUT_MS:-300000}

case "$E2E_STARTUP_TIMEOUT_MS" in
    *[!0-9]*|'')
        echo "LOUPIST_E2E_STARTUP_TIMEOUT_MS must be a positive integer." >&2
        exit 1
        ;;
esac
if [ "$E2E_STARTUP_TIMEOUT_MS" -le 0 ]; then
    echo "LOUPIST_E2E_STARTUP_TIMEOUT_MS must be a positive integer." >&2
    exit 1
fi

# No secret and no network: the e2e catalog marker opens the shell without a
# provider sign-in. Nothing in this suite sends a model request.
export LOUPIST_E2E_PROVIDER_READY=1
# Each launch lets Vite bind its own private port.

cleanup_app() {
    if [ -n "$APP_PID" ] && kill -0 "$APP_PID" 2>/dev/null; then
        kill "$APP_PID" 2>/dev/null || true
        wait "$APP_PID" 2>/dev/null || true
    fi
    APP_PID=
    rm -f -- "$SESSION_INFO"
}

wait_for_dev_server_exit() {
    DEV_PORT="$1" node <<'NODE'
import net from "node:net";

const port = Number(process.env.DEV_PORT);
const deadline = Date.now() + 15_000;
while (Date.now() < deadline) {
  let open = false;
  for (const host of ["127.0.0.1", "::1"]) {
    if (await new Promise((resolve) => {
      const socket = net.createConnection({ host, port });
      socket.setTimeout(200);
      socket.once("connect", () => {
        socket.destroy();
        resolve(true);
      });
      const closed = () => {
        socket.destroy();
        resolve(false);
      };
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
throw new Error(`Loupist dev server on port ${port} did not exit within 15 seconds`);
NODE
}

cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    cleanup_app
    rm -f -- "$RUN_LOG"
    rm -rf -- "$STATE_ROOT"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

STATE_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/loupist-persistence-e2e.XXXXXX")
SESSION_INFO="$STATE_ROOT/session.json"
RUN_LOG=$(mktemp "${TMPDIR:-/tmp}/loupist-persistence-log.XXXXXX")

# run_phase <seed|restart|copy> <state folder>
run_phase() {
    phase=$1
    phase_root="$STATE_ROOT/$2"
    mkdir -p "$phase_root"
    cp "$SCRIPT_DIR/fixtures/journeys/holdings.csv" "$phase_root/holdings.csv"
    : >"$RUN_LOG"
    rm -f -- "$SESSION_INFO"
    LOUPIST_E2E_STATE_DIR="$phase_root" \
        LOUPIST_E2E_PICK_FILE="$phase_root/holdings.csv" \
        LOUPIST_E2E_SESSION_INFO="$SESSION_INFO" \
        "$SCRIPT_DIR/run-app.sh" >"$RUN_LOG" 2>&1 &
    APP_PID=$!

    if ! node "$SCRIPT_DIR/wait-session.mjs" \
        --pid "$APP_PID" \
        --session "$SESSION_INFO" \
        --timeout-ms "$E2E_STARTUP_TIMEOUT_MS"; then
        cat "$RUN_LOG" >&2
        echo "Loupist exited before persistence phase $phase was ready." >&2
        exit 1
    fi

    DEV_PORT=$(node -e 'const s=JSON.parse(require("node:fs").readFileSync(process.argv[1],"utf8")); console.log(new URL(s.devUrl).port)' "$SESSION_INFO")

    if ! node "$SCRIPT_DIR/native-persistence.mjs" "$SESSION_INFO" "$phase" "$phase_root"; then
        tail -n 100 "$RUN_LOG" >&2
        exit 1
    fi
    cleanup_app
    if [ "${LOUPIST_E2E_REUSE_DEV_SERVER:-}" != "1" ]; then
        wait_for_dev_server_exit "$DEV_PORT"
    fi
}

# The fixture adds a sealed World and a saved Chat turn to the first launch only.
export LOUPIST_E2E_KNOWLEDGE_SEED=1
run_phase seed seed
unset LOUPIST_E2E_KNOWLEDGE_SEED
run_phase restart seed
mkdir -p "$STATE_ROOT/copy"
cp -R "$STATE_ROOT/seed/workspace" "$STATE_ROOT/copy/workspace"
run_phase copy copy
echo "Loupist native persistence passed."
