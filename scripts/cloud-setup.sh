#!/bin/sh
# Provision a Linux cloud agent VM for the Loupist source gate. Safe to rerun.
set -eu

if [ "${1:-}" = --quiet ]; then
    LOG=${TMPDIR:-/tmp}/loupist-cloud-setup.log
    if sh "$0" > "$LOG" 2>&1; then
        tail -n 2 "$LOG"
        exit 0
    fi
    status=$?
    tail -n 20 "$LOG" >&2
    exit "$status"
fi

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

fail() {
    echo "cloud-setup: $*" >&2
    exit 1
}

[ "$(uname -s)" = Linux ] || fail "this provisions Linux VMs; on macOS run apps/loupist/scripts/setup.sh"

ACTION=.github/actions/setup-toolchains/action.yml
NODE_VERSION=$(tr -d 'v \n' < .nvmrc)
RUST_VERSION=$(sed -n 's/^ *toolchain: *//p' "$ACTION" | head -n 1)
UV_VERSION=$(sed -n '/setup-uv/,/^ *- name:/s/^ *version: *//p' "$ACTION" | head -n 1)
TOOLS=${LOUPIST_TOOLS:-$HOME/.local/loupist-tools}
NODE_HOME="$TOOLS/node-v$NODE_VERSION"
case $(uname -m) in
    x86_64) NODE_ARCH=x64 TRIPLE=x86_64-unknown-linux-gnu ;;
    aarch64 | arm64) NODE_ARCH=arm64 TRIPLE=aarch64-unknown-linux-gnu ;;
    *) fail "unsupported architecture: $(uname -m)" ;;
esac
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$TOOLS" "$HOME/.local/bin"

as_root() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
    elif command -v sudo >/dev/null 2>&1 && sudo -n true 2>/dev/null; then
        sudo -n "$@"
    else
        return 1
    fi
}

download() {
    curl -fsSL --retry 3 "$1" -o "$2"
}

require_sha256() {
    [ -n "$2" ] && [ "$(sha256sum "$1" | cut -d' ' -f1)" = "$2" ] \
        || fail "checksum mismatch for $(basename "$1")"
}

if command -v apt-get >/dev/null 2>&1 \
    && { ! pkg-config --exists webkit2gtk-4.1 2>/dev/null || ! command -v xvfb-run >/dev/null 2>&1; }; then
    as_root true 2>/dev/null \
        || fail "Tauri's Linux build libraries are missing; run as root or with passwordless sudo, or install the packages in docs/dev/checks.md"
    as_root apt-get update -qq
    as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends \
        build-essential pkg-config libgtk-3-dev libwebkit2gtk-4.1-dev \
        libayatana-appindicator3-dev librsvg2-dev libssl-dev libxdo-dev \
        procps lsof xvfb curl xz-utils ca-certificates
fi

if [ ! -x "$NODE_HOME/bin/node" ]; then
    ARCHIVE="node-v$NODE_VERSION-linux-$NODE_ARCH.tar.xz"
    download "https://nodejs.org/dist/v$NODE_VERSION/$ARCHIVE" "$WORK/$ARCHIVE"
    download "https://nodejs.org/dist/v$NODE_VERSION/SHASUMS256.txt" "$WORK/SHASUMS256.txt"
    require_sha256 "$WORK/$ARCHIVE" "$(awk -v f="$ARCHIVE" '$2 == f { print $1 }' "$WORK/SHASUMS256.txt")"
    mkdir -p "$NODE_HOME.partial"
    tar -xJf "$WORK/$ARCHIVE" --strip-components=1 -C "$NODE_HOME.partial"
    mv "$NODE_HOME.partial" "$NODE_HOME"
fi
PATH="$NODE_HOME/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
export PATH

if ! command -v rustup >/dev/null 2>&1; then
    RUSTUP="https://static.rust-lang.org/rustup/dist/$TRIPLE/rustup-init"
    download "$RUSTUP" "$WORK/rustup-init"
    download "$RUSTUP.sha256" "$WORK/rustup-init.sha256"
    require_sha256 "$WORK/rustup-init" "$(cut -d' ' -f1 "$WORK/rustup-init.sha256")"
    chmod +x "$WORK/rustup-init"
    "$WORK/rustup-init" -y --no-modify-path --profile minimal --default-toolchain none
fi
rustup toolchain install "$RUST_VERSION" --profile minimal --component clippy,rustfmt
rustup default "$RUST_VERSION"

if [ "$(uv --version 2>/dev/null | cut -d' ' -f2)" != "$UV_VERSION" ]; then
    UV_ARCHIVE="uv-$TRIPLE.tar.gz"
    UV_URL="https://github.com/astral-sh/uv/releases/download/$UV_VERSION/$UV_ARCHIVE"
    download "$UV_URL" "$WORK/$UV_ARCHIVE"
    download "$UV_URL.sha256" "$WORK/$UV_ARCHIVE.sha256"
    require_sha256 "$WORK/$UV_ARCHIVE" "$(cut -d' ' -f1 "$WORK/$UV_ARCHIVE.sha256")"
    tar -xzf "$WORK/$UV_ARCHIVE" -C "$WORK"
    install -m 0755 "$WORK/uv-$TRIPLE/uv" "$WORK/uv-$TRIPLE/uvx" "$HOME/.local/bin/"
fi

[ "$(node --version)" = "v$NODE_VERSION" ] || fail "node is $(node --version), expected v$NODE_VERSION"
[ "$(rustc --version | cut -d' ' -f2)" = "$RUST_VERSION" ] || fail "rustc is not $RUST_VERSION"
[ "$(uv --version | cut -d' ' -f2)" = "$UV_VERSION" ] || fail "uv is not $UV_VERSION"

sh apps/loupist/scripts/setup.sh

# Cloud snapshots keep files but not exported variables, so the pinned tools
# replace any image copies where a new shell looks first.
for tool in node npm npx cargo cargo-clippy cargo-fmt rustc rustfmt rustup uv uvx; do
    target=$(command -v "$tool" 2>/dev/null) || continue
    [ "$target" = "/usr/local/bin/$tool" ] || as_root ln -sf "$target" "/usr/local/bin/$tool" || break
done
TOOL_PATH="$NODE_HOME/bin:$HOME/.cargo/bin:$HOME/.local/bin"
if [ -n "${CLAUDE_ENV_FILE:-}" ]; then
    echo "export PATH=\"$TOOL_PATH:\$PATH\"" >> "$CLAUDE_ENV_FILE"
fi
echo "If a new shell finds another Node or Rust, run: export PATH=\"$TOOL_PATH:\$PATH\""
echo "Loupist cloud setup complete: node $(node --version), $(rustc --version), $(uv --version)"
