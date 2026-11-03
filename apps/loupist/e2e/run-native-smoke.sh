#!/bin/sh
set -eu
umask 077

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
# Keep one native GUI owner across checkouts and suite restarts.
if [ "${LOUPIST_GUI_LEASE_ENTRY:-}" != "$$:$0" ]; then
    exec python3 "$SCRIPT_DIR/native_ui_lease.py" "$0" "$@"
fi
RUN_LOG=
SESSION_INFO=
APP_PID=
SMOKE_ARGS=
E2E_STARTUP_TIMEOUT_MS=${LOUPIST_E2E_STARTUP_TIMEOUT_MS:-300000}

case "${1:-}" in
    --full)
        SMOKE_ARGS=--full
        ;;
    "" )
        ;;
    *)
        echo "usage: $0 [--full]" >&2
        exit 1
        ;;
esac

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

# No secret and no network: the e2e catalog marker lets this credential-free
# suite pass the ChatGPT step and open the shell. Live journeys do not set it.
export LOUPIST_E2E_PROVIDER_READY=1
if [ "${LOUPIST_E2E_REUSE_DEV_SERVER:-}" != "1" ] && [ -z "${LOUPIST_E2E_DEV_PORT:-}" ]; then
    LOUPIST_E2E_DEV_PORT=$(node -e '
      const net = require("node:net");
      const alloc = () => new Promise((resolve, reject) => {
        const server = net.createServer();
        server.unref();
        server.once("error", reject);
        server.listen(0, "127.0.0.1", () => {
          const port = server.address().port;
          server.close(() => resolve(port));
        });
      });
      (async () => {
        for (let attempt = 0; attempt < 20; attempt += 1) {
          const port = await alloc();
          if (port !== 1420) {
            process.stdout.write(String(port));
            return;
          }
        }
        process.exit(1);
      })().catch((error) => { console.error(error.message); process.exit(1); });
    ')
    export LOUPIST_E2E_DEV_PORT
fi

cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    if [ -n "$APP_PID" ] && kill -0 "$APP_PID" 2>/dev/null; then
        kill "$APP_PID" 2>/dev/null || true
        wait "$APP_PID" 2>/dev/null || true
    fi
    rm -f -- "$SESSION_INFO"
    rm -f -- "$RUN_LOG"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

RUN_LOG=$(mktemp "${TMPDIR:-/tmp}/loupist-native-e2e.XXXXXX")
SESSION_INFO="$RUN_LOG.session.json"
rm -f -- "$SESSION_INFO"
LOUPIST_E2E_SESSION_INFO="$SESSION_INFO" "$SCRIPT_DIR/run-app.sh" >"$RUN_LOG" 2>&1 &
APP_PID=$!

if ! node "$SCRIPT_DIR/wait-session.mjs" \
    --pid "$APP_PID" \
    --session "$SESSION_INFO" \
    --timeout-ms "$E2E_STARTUP_TIMEOUT_MS"; then
    cat "$RUN_LOG" >&2
    echo "Loupist exited before the native E2E session was ready." >&2
    exit 1
fi

node "$SCRIPT_DIR/native-smoke.mjs" "$SESSION_INFO" $SMOKE_ARGS
