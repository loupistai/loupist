#!/bin/sh
set -eu
umask 077

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
# Keep one native GUI owner across checkouts and suite restarts.
if [ "${LOUPIST_GUI_LEASE_ENTRY:-}" != "$$:$0" ]; then
    exec python3 "$SCRIPT_DIR/native_ui_lease.py" "$0" "$@"
fi
APP_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
ARTIFACT_DIR="$SCRIPT_DIR/artifacts"
SESSION_INFO="$ARTIFACT_DIR/current-session.json"
APP_PID=
MODE=development
MANUAL_PROFILE=
if [ "${1:-}" = --manual ] && [ "$#" = 2 ]; then
    MODE=manual
    MANUAL_PROFILE=$2
elif [ "$#" != 0 ]; then
    echo "Usage: $0 [--manual <profile>]" >&2
    exit 2
fi

terminate_process_tree() (
    target_pid=$1
    case "$target_pid" in
        *[!0-9]*|'') return ;;
    esac
    for child_pid in $(pgrep -P "$target_pid" 2>/dev/null || true); do
        terminate_process_tree "$child_pid"
    done
    if kill -0 "$target_pid" 2>/dev/null; then
        kill "$target_pid" 2>/dev/null || true
    fi
)

for command in node pgrep; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "required command is missing: $command" >&2
        exit 1
    fi
done

NODE_BINARY=$(command -v node)
TAURI_CLI="$APP_ROOT/node_modules/@tauri-apps/cli/tauri.js"
if [ ! -f "$TAURI_CLI" ]; then
    echo "Loupist dependencies are not installed. Run: (cd apps/loupist && npm ci)" >&2
    exit 1
fi

# A bare Mach-O process is not discoverable by macOS app-oriented tools.
# Cargo's runner keeps the native process under the existing watcher/lease.
set --
if [ "$(uname -s)" = Darwin ]; then
    set -- -- --config "$SCRIPT_DIR/macos-dev-runner.toml"
fi

if [ -n "${TAURI_WEBDRIVER_PORT:-}" ]; then
    WEBDRIVER_PORT=$TAURI_WEBDRIVER_PORT
else
    WEBDRIVER_PORT=${LOUPIST_DEV_WEBDRIVER_PORT:-14440}
fi
case "$WEBDRIVER_PORT" in
    *[!0-9]*|'')
        echo "TAURI_WEBDRIVER_PORT must be an integer from 1024 to 65535." >&2
        exit 1
        ;;
esac
if [ "$WEBDRIVER_PORT" -lt 1024 ] || [ "$WEBDRIVER_PORT" -gt 65535 ]; then
    echo "TAURI_WEBDRIVER_PORT must be an integer from 1024 to 65535." >&2
    exit 1
fi

mkdir -p "$ARTIFACT_DIR"
if [ "$MODE" = manual ]; then
    MANUAL_IDENTIFIER=$("$NODE_BINARY" "$SCRIPT_DIR/wait-session.mjs" --manual-identifier "$MANUAL_PROFILE")
fi

cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    "$NODE_BINARY" "$SCRIPT_DIR/wait-session.mjs" --clear-session "$SESSION_INFO" --pid "$$"
    if [ -n "$APP_PID" ] && kill -0 "$APP_PID" 2>/dev/null; then
        terminate_process_tree "$APP_PID"
        wait "$APP_PID" 2>/dev/null || true
    fi
    exit "$status"
}
trap cleanup EXIT
if [ "${LOUPIST_E2E_DETACH:-}" != "1" ]; then
    trap 'exit 129' HUP
fi
trap 'exit 130' INT
trap 'exit 143' TERM

if [ "$MODE" = development ]; then
    WEBDRIVER_PORT=$("$NODE_BINARY" "$SCRIPT_DIR/wait-session.mjs" --resolve-port "$WEBDRIVER_PORT")
    export TAURI_WEBDRIVER_PORT="$WEBDRIVER_PORT"
fi

echo "Vite and Rust watch stay enabled. Stop this process when the session is complete."

cd "$APP_ROOT"
if [ "$MODE" = manual ]; then
    echo "Loupist ordinary development profile: $MANUAL_IDENTIFIER"
    echo "WebDriver is disabled; native file dialogs use the ordinary app delegate."
    /usr/bin/env -i \
        PATH="$PATH" \
        HOME="$HOME" \
        TMPDIR="${TMPDIR:-/tmp}" \
        LANG="${LANG:-C}" \
        "$NODE_BINARY" "$TAURI_CLI" dev \
        --config "{\"identifier\":\"$MANUAL_IDENTIFIER\"}" "$@" &
    APP_PID=$!
    "$NODE_BINARY" "$SCRIPT_DIR/wait-session.mjs" \
        --write-manual "$SESSION_INFO" --pid "$$" --app-pid "$APP_PID" --manual-profile "$MANUAL_PROFILE"
    if ! "$NODE_BINARY" "$SCRIPT_DIR/wait-session.mjs" --pid "$$" --session "$SESSION_INFO"; then
        echo "Loupist manual app did not remain running with its owned Vite server." >&2
        exit 1
    fi
    echo "Loupist manual process is running. Confirm its visible UI before reporting readiness."
    wait "$APP_PID"
    APP_PID=
    exit 0
fi

echo "Loupist development profile: com.monarchjuno.loupist"
echo "Loupist development WebDriver: http://127.0.0.1:$WEBDRIVER_PORT"
/usr/bin/env -i \
    PATH="$PATH" \
    HOME="$HOME" \
    TMPDIR="${TMPDIR:-/tmp}" \
    LANG="${LANG:-C}" \
    TAURI_WEBDRIVER_PORT="$WEBDRIVER_PORT" \
    "$NODE_BINARY" "$TAURI_CLI" dev \
    --features webdriver "$@" &
APP_PID=$!

if ! "$NODE_BINARY" "$SCRIPT_DIR/wait-session.mjs" \
    --wait-owned \
    --pid "$APP_PID" \
    --port "$WEBDRIVER_PORT"; then
    echo "Loupist development WebDriver did not become ready." >&2
    exit 1
fi

"$NODE_BINARY" "$SCRIPT_DIR/wait-session.mjs" \
    --write-session "$SESSION_INFO" \
    --pid "$$" \
    --port "$WEBDRIVER_PORT" \
    --profile development

echo "Loupist development window is ready."
echo "Inspect it with: node apps/loupist/e2e/agent-driver.mjs inspect"

wait "$APP_PID"
APP_PID=
