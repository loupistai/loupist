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
SESSION_INFO=${LOUPIST_E2E_SESSION_INFO:-"$ARTIFACT_DIR/current-session.json"}
case "$SESSION_INFO" in
    /*) ;;
    *) echo "LOUPIST_E2E_SESSION_INFO must be absolute." >&2; exit 1 ;;
esac
APP_PID=
CREATED_STATE_ROOT=
LIVE_PI_AGENT_DATA_DIR=

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

for command in node npm cargo pgrep; do
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

cleanup() {
    status=$?
    trap - EXIT HUP INT TERM
    "$NODE_BINARY" "$SCRIPT_DIR/wait-session.mjs" --clear-session "$SESSION_INFO" --pid "$$"
    if [ -n "$APP_PID" ] && kill -0 "$APP_PID" 2>/dev/null; then
        terminate_process_tree "$APP_PID"
        wait "$APP_PID" 2>/dev/null || true
    fi
    if [ -n "$CREATED_STATE_ROOT" ]; then
        rm -rf -- "$CREATED_STATE_ROOT"
    fi
    exit "$status"
}
trap cleanup EXIT
if [ "${LOUPIST_E2E_DETACH:-}" != "1" ]; then
    trap 'exit 129' HUP
fi
trap 'exit 130' INT
trap 'exit 143' TERM

if [ -n "${LOUPIST_E2E_STATE_DIR:-}" ]; then
    case "$LOUPIST_E2E_STATE_DIR" in
        /*) ;;
        *)
            echo "LOUPIST_E2E_STATE_DIR must be absolute." >&2
            exit 1
            ;;
    esac
    E2E_STATE_ROOT="$LOUPIST_E2E_STATE_DIR"
    mkdir -p "$E2E_STATE_ROOT"
else
    E2E_STATE_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/loupist-e2e.XXXXXX")
    CREATED_STATE_ROOT="$E2E_STATE_ROOT"
fi
E2E_APP_DATA_DIR="$E2E_STATE_ROOT/app-data"
if [ -n "${LOUPIST_E2E_WORKSPACE_DIR:-}" ]; then
    case "$LOUPIST_E2E_WORKSPACE_DIR" in
        /*) ;;
        *)
            echo "LOUPIST_E2E_WORKSPACE_DIR must be absolute." >&2
            exit 1
            ;;
    esac
    E2E_WORKSPACE_DIR="$LOUPIST_E2E_WORKSPACE_DIR"
else
    # The E2E fixture explicitly registers a real folder outside app storage.
    # Existing caller-owned state and workspace contents are preserved.
    E2E_WORKSPACE_DIR="$E2E_STATE_ROOT/workspace"
    if [ -L "$E2E_WORKSPACE_DIR" ]; then
        echo "The E2E workspace must be a real directory." >&2
        exit 1
    fi
    mkdir -p "$E2E_WORKSPACE_DIR"
fi
if [ ! -d "$E2E_WORKSPACE_DIR" ] || [ -L "$E2E_WORKSPACE_DIR" ]; then
    echo "LOUPIST_E2E_WORKSPACE_DIR must be an existing real directory." >&2
    exit 1
fi
mkdir -p "$ARTIFACT_DIR"

if [ -n "${LOUPIST_LIVE_PI_AGENT_DATA_DIR:-}" ]; then
    case "$LOUPIST_LIVE_PI_AGENT_DATA_DIR" in
        /*) ;;
        *)
            echo "LOUPIST_LIVE_PI_AGENT_DATA_DIR must be absolute." >&2
            exit 1
            ;;
    esac
    if [ ! -d "$LOUPIST_LIVE_PI_AGENT_DATA_DIR" ] || [ -L "$LOUPIST_LIVE_PI_AGENT_DATA_DIR" ]; then
        echo "LOUPIST_LIVE_PI_AGENT_DATA_DIR must be a real directory." >&2
        exit 1
    fi
    LIVE_PI_AGENT_DATA_DIR="$LOUPIST_LIVE_PI_AGENT_DATA_DIR"
fi

E2E_KNOWLEDGE_SEED=
if [ "${LOUPIST_E2E_KNOWLEDGE_SEED:-}" = "1" ]; then
    E2E_KNOWLEDGE_SEED=1
fi

E2E_PROVIDER_READY=
if [ -n "${LOUPIST_E2E_PROVIDER_READY:-}" ]; then
    if [ "$LOUPIST_E2E_PROVIDER_READY" != "1" ]; then
        echo "LOUPIST_E2E_PROVIDER_READY must be 1 when set." >&2
        exit 1
    fi
    E2E_PROVIDER_READY=1
fi

E2E_PICK_FILE=
if [ -n "${LOUPIST_E2E_PICK_FILE:-}" ]; then
    case "$LOUPIST_E2E_PICK_FILE" in
        /*) ;;
        *)
            echo "LOUPIST_E2E_PICK_FILE must be an absolute path." >&2
            exit 1
            ;;
    esac
    if [ -L "$LOUPIST_E2E_PICK_FILE" ] || [ ! -f "$LOUPIST_E2E_PICK_FILE" ]; then
        echo "LOUPIST_E2E_PICK_FILE must be an existing file." >&2
        exit 1
    fi
    E2E_PICK_FILE=$LOUPIST_E2E_PICK_FILE
fi


if [ -n "${LOUPIST_E2E_PORT:-}" ]; then
    case "$LOUPIST_E2E_PORT" in
        *[!0-9]*|'')
            echo "LOUPIST_E2E_PORT must be an integer from 1024 to 65535." >&2
            exit 1
            ;;
    esac
    if [ "$LOUPIST_E2E_PORT" -lt 1024 ] || [ "$LOUPIST_E2E_PORT" -gt 65535 ]; then
        echo "LOUPIST_E2E_PORT must be an integer from 1024 to 65535." >&2
        exit 1
    fi
    WEBDRIVER_PORT=$LOUPIST_E2E_PORT
else
    WEBDRIVER_PORT=$("$NODE_BINARY" -e '
      const net = require("node:net");
      const server = net.createServer();
      server.unref();
      server.on("error", (error) => { console.error(error.message); process.exit(1); });
      server.listen(0, "127.0.0.1", () => {
        const address = server.address();
        console.log(address.port);
        server.close();
      });
    ')
fi
WEBDRIVER_PORT=$("$NODE_BINARY" "$SCRIPT_DIR/wait-session.mjs" --resolve-port "$WEBDRIVER_PORT")
export TAURI_WEBDRIVER_PORT="$WEBDRIVER_PORT"

if [ "${LOUPIST_E2E_REUSE_DEV_SERVER:-}" = "1" ] && [ -n "${LOUPIST_E2E_DEV_PORT:-}" ]; then
    echo "LOUPIST_E2E_DEV_PORT cannot be combined with LOUPIST_E2E_REUSE_DEV_SERVER." >&2
    exit 1
fi

E2E_DEV_PORT=
E2E_DEV_URL=http://localhost:1420
if [ -n "${LOUPIST_E2E_DEV_PORT:-}" ]; then
    case "$LOUPIST_E2E_DEV_PORT" in
        *[!0-9]*|'')
            echo "LOUPIST_E2E_DEV_PORT must be an integer from 1024 to 65535 other than 1420." >&2
            exit 1
            ;;
    esac
    if [ "$LOUPIST_E2E_DEV_PORT" -lt 1024 ] || [ "$LOUPIST_E2E_DEV_PORT" -gt 65535 ] || [ "$LOUPIST_E2E_DEV_PORT" -eq 1420 ]; then
        echo "LOUPIST_E2E_DEV_PORT must be an integer from 1024 to 65535 other than 1420." >&2
        exit 1
    fi
    if [ "$LOUPIST_E2E_DEV_PORT" = "$WEBDRIVER_PORT" ]; then
        WEBDRIVER_PORT=$("$NODE_BINARY" -e '
          const net = require("node:net");
          const banned = new Set(process.argv.slice(1).map(Number));
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
              if (!banned.has(port)) {
                process.stdout.write(String(port));
                return;
              }
            }
            process.exit(1);
          })().catch((error) => {
            console.error(error.message);
            process.exit(1);
          });
        ' "$LOUPIST_E2E_DEV_PORT" 1420)
        export TAURI_WEBDRIVER_PORT="$WEBDRIVER_PORT"
    fi
    E2E_DEV_PORT=$LOUPIST_E2E_DEV_PORT
    E2E_DEV_URL="http://127.0.0.1:$E2E_DEV_PORT"
fi

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

echo "Loupist E2E profile: com.monarchjuno.loupist.e2e"
echo "Loupist E2E state: $E2E_APP_DATA_DIR"
echo "Loupist E2E workspace: $E2E_WORKSPACE_DIR"
echo "Loupist E2E WebDriver: http://127.0.0.1:$WEBDRIVER_PORT"
echo "Loupist E2E dev server: $E2E_DEV_URL"
echo "Stop this process when the test session is complete."

E2E_CONFIG=src-tauri/tauri.e2e.conf.json
set --
if [ "$(uname -s)" = Darwin ]; then
    set -- -- --config "$SCRIPT_DIR/macos-dev-runner.toml"
fi
if [ "${LOUPIST_E2E_REUSE_DEV_SERVER:-}" = "1" ]; then
    # Explicitly reuse this workspace's running Vite server; app data and the
    # native WebDriver port remain isolated for the suite.
    E2E_CONFIG=$("$NODE_BINARY" -e '
const fs = require("node:fs");
const config = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
config.build = { beforeDevCommand: "", devUrl: "http://localhost:1420" };
process.stdout.write(JSON.stringify(config));
' "$APP_ROOT/src-tauri/tauri.e2e.conf.json")
elif [ -n "$E2E_DEV_PORT" ]; then
    E2E_CONFIG=$("$NODE_BINARY" -e '
const fs = require("node:fs");
const config = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
const port = process.argv[2];
config.build = {
  beforeDevCommand: `npm run dev -- --host 127.0.0.1 --port ${port} --strictPort`,
  devUrl: `http://127.0.0.1:${port}`,
};
process.stdout.write(JSON.stringify(config));
' "$APP_ROOT/src-tauri/tauri.e2e.conf.json" "$E2E_DEV_PORT")
fi

cd "$APP_ROOT"
# The embedded WebDriver is third-party code in the app process. Keep only the
# minimum developer runtime environment and never inherit shell credentials,
# proxies, cloud-test settings, or Node/Rust injection flags.
SPAWNED_AT_MS=$("$NODE_BINARY" -e 'process.stdout.write(String(Date.now()))')
if [ -n "$LIVE_PI_AGENT_DATA_DIR" ]; then
    /usr/bin/env -i \
        PATH="$PATH" \
        HOME="$HOME" \
        TMPDIR="${TMPDIR:-/tmp}" \
        LANG="${LANG:-C}" \
        LOUPIST_E2E_APP_DATA_DIR="$E2E_APP_DATA_DIR" \
        LOUPIST_E2E_WORKSPACE_DIR="$E2E_WORKSPACE_DIR" \
        LOUPIST_LIVE_PI_AGENT_DATA_DIR="$LIVE_PI_AGENT_DATA_DIR" \
        LOUPIST_E2E_KNOWLEDGE_SEED="$E2E_KNOWLEDGE_SEED" \
        LOUPIST_E2E_PROVIDER_READY="$E2E_PROVIDER_READY" \
        LOUPIST_E2E_PICK_FILE="$E2E_PICK_FILE" \
        TAURI_WEBDRIVER_PORT="$WEBDRIVER_PORT" \
        "$NODE_BINARY" "$TAURI_CLI" dev \
        --no-watch \
        --features e2e \
        --config "$E2E_CONFIG" "$@" &
else
    /usr/bin/env -i \
        PATH="$PATH" \
        HOME="$HOME" \
        TMPDIR="${TMPDIR:-/tmp}" \
        LANG="${LANG:-C}" \
        LOUPIST_E2E_APP_DATA_DIR="$E2E_APP_DATA_DIR" \
        LOUPIST_E2E_WORKSPACE_DIR="$E2E_WORKSPACE_DIR" \
        LOUPIST_E2E_KNOWLEDGE_SEED="$E2E_KNOWLEDGE_SEED" \
        LOUPIST_E2E_PROVIDER_READY="$E2E_PROVIDER_READY" \
        LOUPIST_E2E_PICK_FILE="$E2E_PICK_FILE" \
        TAURI_WEBDRIVER_PORT="$WEBDRIVER_PORT" \
        "$NODE_BINARY" "$TAURI_CLI" dev \
        --no-watch \
        --features e2e \
        --config "$E2E_CONFIG" "$@" &
fi
APP_PID=$!

if ! "$NODE_BINARY" "$SCRIPT_DIR/wait-session.mjs" \
    --wait-owned \
    --pid "$APP_PID" \
    --port "$WEBDRIVER_PORT" \
    --timeout-ms "$E2E_STARTUP_TIMEOUT_MS"; then
    echo "Loupist E2E WebDriver did not become ready." >&2
    exit 1
fi

if [ -n "${LOUPIST_E2E_LIVE_PROFILE:-}" ]; then
    "$NODE_BINARY" "$SCRIPT_DIR/wait-session.mjs" \
        --write-session "$SESSION_INFO" \
        --pid "$$" \
        --port "$WEBDRIVER_PORT" \
        --profile e2e \
        --live-profile "$LOUPIST_E2E_LIVE_PROFILE" \
        --spawned-at-ms "$SPAWNED_AT_MS" \
        --dev-url "$E2E_DEV_URL"
else
    "$NODE_BINARY" "$SCRIPT_DIR/wait-session.mjs" \
        --write-session "$SESSION_INFO" \
        --pid "$$" \
        --port "$WEBDRIVER_PORT" \
        --profile e2e \
        --spawned-at-ms "$SPAWNED_AT_MS" \
        --dev-url "$E2E_DEV_URL"
fi

echo "Loupist E2E WebDriver is ready."
echo "Inspect it with: node apps/loupist/e2e/agent-driver.mjs inspect"

wait "$APP_PID"
APP_PID=
