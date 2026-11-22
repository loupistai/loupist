#!/bin/sh
set -eu
scripts=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
python="$scripts/../openbb/.venv/bin/python"
if [ ! -x "$python" ]; then
    python=python3
fi
exec "$python" "$scripts/check_hardened_helpers.py" "$@"
