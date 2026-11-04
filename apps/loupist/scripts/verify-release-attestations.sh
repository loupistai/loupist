#!/bin/bash
set -euo pipefail

if [ "$#" -ne 4 ]; then
    echo "Usage: $0 <asset-directory> <owner/repository> <source-commit> <source-ref>" >&2
    exit 2
fi

ASSET_DIRECTORY=$1
REPOSITORY=$2
SOURCE_COMMIT=$3
SOURCE_REF=$4

if [ ! -d "$ASSET_DIRECTORY" ] || [ -L "$ASSET_DIRECTORY" ]; then
    echo "Asset directory must be a real directory." >&2
    exit 1
fi
if [[ ! "$REPOSITORY" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]]; then
    echo "Repository must be an owner/name pair." >&2
    exit 1
fi
if [[ ! "$SOURCE_COMMIT" =~ ^[0-9a-f]{40}$ ]]; then
    echo "Source commit must be a lowercase 40-digit SHA-1." >&2
    exit 1
fi
if [[ ! "$SOURCE_REF" =~ ^refs/tags/v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
    echo "Source ref must be a stable release tag ref." >&2
    exit 1
fi

VERSION="${SOURCE_REF#refs/tags/v}"
UPDATER="Loupist-${VERSION}-darwin-aarch64.app.tar.gz"
EXPECTED_NAMES=(
    "Loupist-${VERSION}-aarch64-apple-darwin.dmg"
    "Loupist-macOS-arm64.dmg"
    "$UPDATER"
    "${UPDATER}.sig"
    "Loupist-${VERSION}.spdx.json"
    "latest.json"
    "RELEASE-METADATA.json"
    "SHA256SUMS"
)
shopt -s nullglob
ASSETS=("$ASSET_DIRECTORY"/*)
for name in "${EXPECTED_NAMES[@]}"; do
    asset="$ASSET_DIRECTORY/$name"
    if [ ! -f "$asset" ] || [ -L "$asset" ]; then
        echo "Closed release set is missing attested file: $name" >&2
        exit 1
    fi
done
if [ "${#ASSETS[@]}" -ne "${#EXPECTED_NAMES[@]}" ]; then
    echo "Closed release set contains files outside the attested names." >&2
    exit 1
fi

SIGNER_WORKFLOW="$REPOSITORY/.github/workflows/release.yml"
for asset in "${ASSETS[@]}"; do
    if [ ! -f "$asset" ] || [ -L "$asset" ]; then
        echo "Attestation subject must be a regular file: $asset" >&2
        exit 1
    fi
    gh attestation verify "$asset" \
        --repo "$REPOSITORY" \
        --signer-workflow "$SIGNER_WORKFLOW" \
        --signer-digest "$SOURCE_COMMIT" \
        --source-digest "$SOURCE_COMMIT" \
        --source-ref "$SOURCE_REF" \
        --deny-self-hosted-runners \
        >/dev/null
done

echo "Verified provenance for all ${#ASSETS[@]} closed-set release assets."
