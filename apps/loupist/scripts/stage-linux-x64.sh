#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
APP_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)

"$SCRIPT_DIR/stage-pi-linux-x64.sh"
"$SCRIPT_DIR/stage-openbb-linux-x64.sh"
PYTHON_BIN="$APP_ROOT/openbb/.venv/bin/python"
"$PYTHON_BIN" "$SCRIPT_DIR/stage-embed-assets.py" --ensure
"$PYTHON_BIN" "$SCRIPT_DIR/stage_python_runtime.py" --ensure

echo "Loupist Linux x86_64 staging is complete."
