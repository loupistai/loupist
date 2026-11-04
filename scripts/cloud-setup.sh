#!/bin/sh
# Provision a Linux cloud agent VM for the Loupist source gate. Safe to rerun.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

if [ "$(uname -s)" != Linux ]; then
    echo "cloud-setup.sh provisions Linux VMs; on macOS run apps/loupist/scripts/setup.sh" >&2
    exit 2
fi

ACTION=.github/actions/setup-toolchains/action.yml
NODE_VERSION=$(tr -d 'v \n' < .nvmrc)
RUST_VERSION=$(sed -n 's/^ *toolchain: *//p' "$ACTION" | head -n 1)
UV_VERSION=$(sed -n '/setup-uv/,/^ *- name:/s/^ *version: *//p' "$ACTION" | head -n 1)
TOOLS=${LOUPIST_TOOLS:-$HOME/.local/loupist-tools}
NODE_HOME="$TOOLS/node-v$NODE_VERSION"
mkdir -p "$TOOLS"

as_root() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
    elif command -v sudo >/dev/null 2>&1 && sudo -n true 2>/dev/null; then
        sudo -n "$@"
    else
        return 1
    fi
}

if command -v apt-get >/dev/null 2>&1 && ! pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
    as_root apt-get update -qq
    as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends \
        build-essential pkg-config libgtk-3-dev libwebkit2gtk-4.1-dev \
        libayatana-appindicator3-dev librsvg2-dev libssl-dev libxdo-dev \
        procps lsof curl xz-utils ca-certificates
fi

if [ ! -x "$NODE_HOME/bin/node" ]; then
    case $(uname -m) in
        x86_64) NODE_ARCH=x64 ;;
        aarch64 | arm64) NODE_ARCH=arm64 ;;
        *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
    esac
    ARCHIVE="node-v$NODE_VERSION-linux-$NODE_ARCH.tar.xz"
    DOWNLOAD=$(mktemp -d)
    curl -fsSL "https://nodejs.org/dist/v$NODE_VERSION/$ARCHIVE" -o "$DOWNLOAD/$ARCHIVE"
    curl -fsSL "https://nodejs.org/dist/v$NODE_VERSION/SHASUMS256.txt" -o "$DOWNLOAD/SHASUMS256.txt"
    (cd "$DOWNLOAD" && grep " $ARCHIVE\$" SHASUMS256.txt | sha256sum -c -)
    mkdir -p "$NODE_HOME.partial"
    tar -xJf "$DOWNLOAD/$ARCHIVE" --strip-components=1 -C "$NODE_HOME.partial"
    mv "$NODE_HOME.partial" "$NODE_HOME"
    rm -rf "$DOWNLOAD"
fi
PATH="$NODE_HOME/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
export PATH

if ! command -v rustup >/dev/null 2>&1; then
    curl -fsSL https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal \
        --default-toolchain none
fi
rustup toolchain install "$RUST_VERSION" --profile minimal --component clippy,rustfmt
rustup default "$RUST_VERSION"

if [ "$(uv --version 2>/dev/null | cut -d' ' -f2)" != "$UV_VERSION" ]; then
    curl -fsSL "https://astral.sh/uv/$UV_VERSION/install.sh" | env UV_NO_MODIFY_PATH=1 sh
fi

sh apps/loupist/scripts/setup.sh

# Cloud snapshots keep files but not exported variables, so expose the pinned
# tools where a new shell looks first.
for tool in node npm npx cargo cargo-clippy cargo-fmt rustc rustfmt rustup uv uvx; do
    target=$(command -v "$tool" 2>/dev/null) || continue
    [ "$target" = "/usr/local/bin/$tool" ] || as_root ln -sf "$target" "/usr/local/bin/$tool" || break
done
TOOL_PATH="$NODE_HOME/bin:$HOME/.cargo/bin:$HOME/.local/bin"
if [ -n "${CLAUDE_ENV_FILE:-}" ]; then
    echo "export PATH=\"$TOOL_PATH:\$PATH\"" >> "$CLAUDE_ENV_FILE"
fi
echo "Loupist cloud setup complete: node $(node --version), $(rustc --version), $(uv --version)"
echo "If a new shell finds another Node or Rust, run: export PATH=\"$TOOL_PATH:\$PATH\""
