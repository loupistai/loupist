#!/bin/sh
set -eu
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
# Locked setup runs under the same resource locks as local checks.
exec "$SCRIPT_DIR/check.sh" all --install "$@"
