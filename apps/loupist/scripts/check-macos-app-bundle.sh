#!/bin/sh
set -eu

APP_BUNDLE=${1:?usage: check-macos-app-bundle.sh /path/to/Loupist\ Terminal.app [--expected-bundle-version BUILD]}
EXPECTED_BUNDLE_VERSION=
if [ "$#" -eq 3 ] && [ "$2" = "--expected-bundle-version" ]; then
    EXPECTED_BUNDLE_VERSION=$3
elif [ "$#" -ne 1 ]; then
    echo "usage: check-macos-app-bundle.sh /path/to/Loupist.app [--expected-bundle-version BUILD]" >&2
    exit 2
fi
if [ ! -d "$APP_BUNDLE" ]; then
    echo "Loupist app bundle is missing: $APP_BUNDLE" >&2
    exit 1
fi
APP_BUNDLE=$(CDPATH= cd -- "$APP_BUNDLE" && pwd)

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
APP_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
REPO_ROOT=$(CDPATH= cd -- "$APP_ROOT/../.." && pwd)
TAURI_ROOT="$APP_ROOT/src-tauri"
PYTHON_BIN="$APP_ROOT/openbb/.venv/bin/python"
CONTENTS="$APP_BUNDLE/Contents"
MACOS_DIR="$CONTENTS/MacOS"
RESOURCE_DIR="$CONTENTS/Resources"
RUNTIME_DIR="$RESOURCE_DIR/pi-runtime"
AGENT_DIR="$RESOURCE_DIR/loupist-agent"
PI_BINARY="$RUNTIME_DIR/loupist-pi"
OPENBB_RUNTIME="$RUNTIME_DIR/openbb"
OPENBB_BINARY="$OPENBB_RUNTIME/loupist-openbb"
EMBED_ASSETS="$RESOURCE_DIR/embed_assets"
PI_VERSION=1.5.0

if [ "$(uname -s)" != Darwin ] || [ "$(uname -m)" != arm64 ]; then
    echo "packaged app inspection requires macOS arm64." >&2
    exit 1
fi
if [ ! -x "$PYTHON_BIN" ] || [ "$("$PYTHON_BIN" -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")')" != 3.12 ]; then
    echo "the pinned Python 3.12 staging environment is missing: $PYTHON_BIN" >&2
    exit 1
fi

for required in \
    "$MACOS_DIR/loupist" \
    "$PI_BINARY" \
    "$OPENBB_BINARY" \
    "$EMBED_ASSETS/config.json" \
    "$EMBED_ASSETS/model.safetensors" \
    "$EMBED_ASSETS/tokenizer.json" \
    "$EMBED_ASSETS/NOTICE" \
    "$RESOURCE_DIR/python-runtime/loupist-python-worker" \
    "$RESOURCE_DIR/python-runtime/python-snapshot.wasm" \
    "$RESOURCE_DIR/python-runtime/lib/python315.zip" \
    "$RESOURCE_DIR/python-runtime/mpl-data/matplotlibrc" \
    "$RESOURCE_DIR/python-runtime/.runtime.json" \
    "$RESOURCE_DIR/python-runtime/MANIFEST" \
    "$OPENBB_RUNTIME/_internal/loupist_openbb/runtime-manifest.json" \
    "$OPENBB_RUNTIME/runtime-manifest.json" \
    "$OPENBB_RUNTIME/THIRD_PARTY_LICENSES/python-distributions.json" \
    "$OPENBB_RUNTIME/uv.lock" \
    "$AGENT_DIR/SYSTEM.md" \
    "$AGENT_DIR/loupist-extension.mjs" \
    "$AGENT_DIR/tool-catalog.json" \
    "$AGENT_DIR/broker-client.mjs" \
    "$AGENT_DIR/workspace-tools.mjs" \
    "$AGENT_DIR/workspace-resources.mjs" \
    "$AGENT_DIR/pi-settings.mjs" \
    "$AGENT_DIR/session-continuity.mjs" \
    "$AGENT_DIR/loupist-native-search.mjs" \
    "$AGENT_DIR/native-search/common.mjs" \
    "$AGENT_DIR/native-search/codex.mjs" \
    "$AGENT_DIR/native-search/anthropic.mjs" \
    "$AGENT_DIR/native-search/xai.mjs" \
    "$AGENT_DIR/loupist-provider-extension.mjs" \
    "$AGENT_DIR/skills/research/SKILL.md" \
    "$AGENT_DIR/skills/world/SKILL.md" \
    "$AGENT_DIR/skills/lens/SKILL.md" \
    "$RESOURCE_DIR/LICENSE" \
    "$RESOURCE_DIR/NOTICE" \
    "$RESOURCE_DIR/THIRD_PARTY_NOTICES.md"; do
    if [ ! -s "$required" ]; then
        echo "packaged asset is missing or empty: $required" >&2
        exit 1
    fi
done

if [ -e "$MACOS_DIR/loupist-core" ]; then
    echo "packaged app contains the retired core sidecar." >&2
    exit 1
fi

for executable in "$MACOS_DIR/loupist" "$PI_BINARY" "$OPENBB_BINARY"; do
    if [ ! -x "$executable" ] || ! LC_ALL=C file "$executable" | grep -Fq 'Mach-O 64-bit executable arm64'; then
        echo "packaged executable is not macOS arm64: $executable" >&2
        exit 1
    fi
done
set -- \
    --tauri-config "$TAURI_ROOT/tauri.conf.json" \
    --source-plist "$TAURI_ROOT/Info.plist" \
    --bundle-plist "$CONTENTS/Info.plist"
if [ -n "$EXPECTED_BUNDLE_VERSION" ]; then
    set -- "$@" --expected-bundle-version "$EXPECTED_BUNDLE_VERSION"
fi
"$PYTHON_BIN" "$SCRIPT_DIR/check-macos-bundle-version.py" "$@"
if [ "$(plutil -extract CFBundleIdentifier raw -o - "$CONTENTS/Info.plist")" != com.monarchjuno.loupist ] || \
    [ "$(plutil -extract CFBundleExecutable raw -o - "$CONTENTS/Info.plist")" != loupist ] || \
    [ "$(plutil -extract CFBundleDisplayName raw -o - "$CONTENTS/Info.plist")" != 'Loupist' ] || \
    [ "$(plutil -extract LSMinimumSystemVersion raw -o - "$CONTENTS/Info.plist")" != 13.0 ]; then
    echo "packaged Info.plist identity is invalid." >&2
    exit 1
fi

"$PYTHON_BIN" "$SCRIPT_DIR/check_embed_assets.py" \
    --source "$REPO_ROOT/src/knowledge/embed_assets" \
    --packaged "$EMBED_ASSETS"

diff -qr "$APP_ROOT/agent" "$AGENT_DIR" >/dev/null
cmp -s "$REPO_ROOT/LICENSE" "$RESOURCE_DIR/LICENSE"
cmp -s "$REPO_ROOT/NOTICE" "$RESOURCE_DIR/NOTICE"
cmp -s "$APP_ROOT/THIRD_PARTY_NOTICES.md" "$RESOURCE_DIR/THIRD_PARTY_NOTICES.md"
if [ -e "$RUNTIME_DIR/openbb-runtime" ]; then
    echo "packaged app contains the obsolete OpenBB runtime path." >&2
    exit 1
fi
if [ -n "$(find "$RUNTIME_DIR" -type l -print -quit)" ]; then
    echo "packaged Pi runtime contains a symbolic link." >&2
    exit 1
fi
if [ -n "$(find "$RUNTIME_DIR" -type f -path '*.dist-info/direct_url.json' -print -quit)" ]; then
    echo "packaged Pi runtime contains local build-path metadata." >&2
    exit 1
fi

PACKAGED_PI_SHA256=$(LC_ALL=C shasum -a 256 "$PI_BINARY" | awk '{print $1}')
PINNED_PI_SHA256=$(tr -d '[:space:]' <"$RUNTIME_DIR/.pi-executable.sha256")
if [ "$PACKAGED_PI_SHA256" != "$PINNED_PI_SHA256" ]; then
    echo "packaged Pi binary differs from its pinned digest." >&2
    exit 1
fi

sh "$SCRIPT_DIR/check-macos-minimum-version.sh" 13.0 "$CONTENTS"
find "$OPENBB_RUNTIME" -type f -exec sh -c '
    for binary do
        description=$(LC_ALL=C file -b "$binary")
        case "$description" in
            *Mach-O*)
                case "$description" in
                    *arm64*) ;;
                    *)
                        echo "packaged runtime contains a Mach-O without arm64 support: $binary" >&2
                        exit 1
                        ;;
                esac
                codesign --verify --strict --verbose=2 "$binary"
                ;;
        esac
    done
' sh {} +
"$PYTHON_BIN" "$SCRIPT_DIR/stage_python_runtime.py" \
    --verify \
    --require-signature \
    --smoke \
    --app-binary "$MACOS_DIR/loupist" \
    --runtime "$RESOURCE_DIR/python-runtime"
"$PYTHON_BIN" "$SCRIPT_DIR/check-sidecars.py" \
    --pi "$PI_BINARY" \
    --pi-version "$PI_VERSION" \
    --pi-runtime "$RUNTIME_DIR" \
    --provider-extension "$AGENT_DIR/loupist-provider-extension.mjs" \
    --openbb "$OPENBB_BINARY" \
    --openbb-runtime "$OPENBB_RUNTIME"

echo "Loupist packaged macOS app contents passed."
