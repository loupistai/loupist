#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

"$SCRIPT_DIR/stage-pi-macos-arm64.sh"
"$SCRIPT_DIR/stage-openbb-macos-arm64.sh"
"$SCRIPT_DIR/../openbb/.venv/bin/python" "$SCRIPT_DIR/stage-embed-assets.py"
"$SCRIPT_DIR/check-package-prerequisites.sh"

echo "Loupist macOS arm64 staging is complete."
