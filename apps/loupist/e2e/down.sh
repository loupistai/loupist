#!/bin/sh
set -eu
umask 077

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
SESSION_INFO="$SCRIPT_DIR/artifacts/current-session.json"

if [ ! -s "$SESSION_INFO" ]; then
    echo "No recorded Loupist session is running."
    exit 0
fi

node "$SCRIPT_DIR/wait-session.mjs" --stop-session "$SESSION_INFO"
echo "Recorded Loupist session stopped. Its persistent data was preserved."
