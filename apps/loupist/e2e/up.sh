#!/bin/sh
set -eu
umask 077

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ARTIFACT_DIR="$SCRIPT_DIR/artifacts"
SESSION_INFO="$ARTIFACT_DIR/current-session.json"
E2E_STARTUP_TIMEOUT_MS=${LOUPIST_E2E_STARTUP_TIMEOUT_MS:-300000}
case "$E2E_STARTUP_TIMEOUT_MS" in
    *[!0-9]*|'')
        echo "LOUPIST_E2E_STARTUP_TIMEOUT_MS must be a positive integer." >&2
        exit 2 ;;
esac
if [ "$E2E_STARTUP_TIMEOUT_MS" -le 0 ]; then
    echo "LOUPIST_E2E_STARTUP_TIMEOUT_MS must be a positive integer." >&2
    exit 2
fi
LAUNCHER_LOG="$ARTIFACT_DIR/launcher.log"
DEV_WEBDRIVER_PORT=${LOUPIST_DEV_WEBDRIVER_PORT:-14440}
MODE=development
MANUAL_PROFILE=manual-preview
LIVE_PROFILE=live
PROFILE_NAME=
FRESH=0
NAMED_PROFILE=0
SAW_MANUAL=0
SAW_LIVE=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --manual) SAW_MANUAL=1; MODE=manual; shift ;;
        --live) SAW_LIVE=1; MODE=live; shift ;;
        --profile)
            [ "$#" -ge 2 ] || { echo "--profile requires a name" >&2; exit 2; }
            PROFILE_NAME=$2; NAMED_PROFILE=1; shift 2 ;;
        --fresh) FRESH=1; shift ;;
        *) echo "Usage: $0 [--manual [--profile <name> | --fresh] | --live [--profile <name>]]" >&2; exit 2 ;;
    esac
done
if [ "$SAW_MANUAL" = 1 ] && [ "$SAW_LIVE" = 1 ]; then
    echo "Choose either --manual or --live." >&2
    exit 2
fi
if [ "$MODE" = development ] && { [ "$NAMED_PROFILE" = 1 ] || [ "$FRESH" = 1 ]; }; then
    echo "--profile and --fresh require --manual or --live." >&2
    exit 2
fi
if [ "$MODE" = live ] && [ "$FRESH" = 1 ]; then
    echo "--fresh creates a manual profile. Use --live --profile <name> for a separate WebDriver library." >&2
    exit 2
fi
if [ "$NAMED_PROFILE" = 1 ] && [ "$FRESH" = 1 ]; then
    echo "Choose either --profile or --fresh." >&2
    exit 2
fi
if [ "$NAMED_PROFILE" = 1 ] && [ "$MODE" = manual ]; then
    MANUAL_PROFILE=$PROFILE_NAME
fi
if [ "$NAMED_PROFILE" = 1 ] && [ "$MODE" = live ]; then
    LIVE_PROFILE=$PROFILE_NAME
fi
if [ "$FRESH" = 1 ]; then
    MANUAL_PROFILE="manual-$(date +%Y%m%d%H%M%S)-$(node -e 'process.stdout.write(require("node:crypto").randomBytes(3).toString("hex"))')"
fi
if [ "$MODE" = manual ]; then
    node "$SCRIPT_DIR/wait-session.mjs" --manual-identifier "$MANUAL_PROFILE" >/dev/null
fi
if [ "$MODE" = live ]; then
    LIVE_PROFILE=$(node "$SCRIPT_DIR/wait-session.mjs" --live-profile-name "$LIVE_PROFILE")
fi

prepare_window() {
    if [ "$MODE" != manual ]; then
        node "$SCRIPT_DIR/agent-driver.mjs" window-default
    fi
}

describe_window() {
    if [ "$(uname -s)" = Darwin ]; then
        echo "Native app target: node apps/loupist/e2e/wait-session.mjs --native-app $SESSION_INFO"
        echo "Use its appBundlePath with this host's native app tool, then inspect the actual screenshot."
    fi
    if [ "$MODE" = manual ]; then
        echo "Loupist manual profile: $MANUAL_PROFILE (persistent separate data)."
        echo "Ordinary Tauri dev is running without WebDriver. Inspect the visible window with OS tools; process readiness is not a UI test result."
    elif [ "$MODE" = live ]; then
        echo "Loupist live WebDriver profile: $LIVE_PROFILE (separate from the development library)."
        echo "OpenAI sign-in uses the selected existing Pi directory in place."
        echo "Before a paid message: node apps/loupist/e2e/agent-driver.mjs live-preset"
    else
        echo "Inspect it with: node apps/loupist/e2e/agent-driver.mjs inspect"
    fi
    echo "Stop its recorded process tree with: apps/loupist/e2e/down.sh"
}

mkdir -p "$ARTIFACT_DIR"

if node "$SCRIPT_DIR/wait-session.mjs" --owner-live "$SESSION_INFO"; then
    if [ "$MODE" = manual ]; then
        MATCH_STATUS=0
        node "$SCRIPT_DIR/wait-session.mjs" --is-manual-session "$SESSION_INFO" --manual-profile "$MANUAL_PROFILE" || MATCH_STATUS=$?
    elif [ "$MODE" = live ]; then
        MATCH_STATUS=0
        node "$SCRIPT_DIR/wait-session.mjs" --is-live-session "$SESSION_INFO" --live-profile "$LIVE_PROFILE" || MATCH_STATUS=$?
    else
        MATCH_STATUS=0
        node "$SCRIPT_DIR/wait-session.mjs" --is-dev-session "$SESSION_INFO" || MATCH_STATUS=$?
    fi
    if [ "$MATCH_STATUS" -ne 0 ]; then
        echo "A different Loupist mode/profile owns the current session. Close it with apps/loupist/e2e/down.sh before switching." >&2
        exit 1
    fi
    node "$SCRIPT_DIR/wait-session.mjs" --recover "$SESSION_INFO"
    echo "Reusing the existing Loupist window."
    describe_window
    exit 0
fi

if [ "$MODE" = development ] && TAURI_WEBDRIVER_PORT="$DEV_WEBDRIVER_PORT" node "$SCRIPT_DIR/wait-session.mjs" --adopt-dev "$SESSION_INFO"; then
    echo "Attached to the existing Loupist development window."
    describe_window
    exit 0
fi

if node "$SCRIPT_DIR/wait-session.mjs" --check-port 1420; then
    echo "Port 1420 is already in use by an unrecorded window/server. Inspect its owner before closing it; no second window was launched." >&2
    exit 1
fi

rm -f -- "$SESSION_INFO"

# Start a new session so the app survives the calling shell's process group.
export LOUPIST_E2E_DETACH=1
: >"$LAUNCHER_LOG"
if [ "$MODE" = manual ]; then
    export TAURI_WEBDRIVER_PORT="$DEV_WEBDRIVER_PORT"
    LAUNCHER_PID=$(node "$SCRIPT_DIR/detach-launch.mjs" "$SCRIPT_DIR/run-dev-app.sh" "$LAUNCHER_LOG" --manual "$MANUAL_PROFILE")
elif [ "$MODE" = live ]; then
    LIVE_STATE=$(node "$SCRIPT_DIR/env-diagnostic.mjs" --print-live-state --profile "$LIVE_PROFILE")
    if [ -z "${LOUPIST_LIVE_PI_AGENT_DATA_DIR:-}" ]; then
        LOUPIST_LIVE_PI_AGENT_DATA_DIR=$(node "$SCRIPT_DIR/env-diagnostic.mjs" --print-pi-dir)
    fi
    mkdir -p "$LIVE_STATE"
    export LOUPIST_E2E_STATE_DIR="$LIVE_STATE"
    export LOUPIST_LIVE_PI_AGENT_DATA_DIR
    export LOUPIST_E2E_LIVE_PROFILE="$LIVE_PROFILE"
    echo "Loupist live state: $LIVE_STATE"
    echo "Selected existing Pi directory reused in place. Credential files were not read."
    LAUNCHER_PID=$(node "$SCRIPT_DIR/detach-launch.mjs" "$SCRIPT_DIR/run-app.sh" "$LAUNCHER_LOG")
else
    export TAURI_WEBDRIVER_PORT="$DEV_WEBDRIVER_PORT"
    LAUNCHER_PID=$(node "$SCRIPT_DIR/detach-launch.mjs" "$SCRIPT_DIR/run-dev-app.sh" "$LAUNCHER_LOG")
fi

if ! node "$SCRIPT_DIR/wait-session.mjs" --pid "$LAUNCHER_PID" --session "$SESSION_INFO" --timeout-ms "$E2E_STARTUP_TIMEOUT_MS"; then
    # detach-launch owns this new process group, including a launcher still building.
    # No `--`: dash's builtin kill rejects it and would leave the launcher running.
    kill -TERM "-$LAUNCHER_PID" 2>/dev/null || true
    echo "Loupist $MODE window failed to start." >&2
    tail -n 80 "$LAUNCHER_LOG" >&2
    exit 1
fi

prepare_window
echo "Loupist $MODE process is ready. Visible window verification is still required."
describe_window
