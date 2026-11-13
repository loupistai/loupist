# Loupist {{version}}{{channel_suffix}}

Loupist is a local-first financial research desktop app for macOS 13 or newer on Apple Silicon. It never trades.

**Download:** Loupist-macOS-arm64.dmg (below).
**First launch:** this Beta is not notarized by Apple. macOS blocks the first launch; allow it once in
System Settings → Privacy & Security → Open Anyway (macOS 13 and 14: Control-click → Open).
Full steps: https://github.com/loupistai/loupist/blob/main/installation.md

## What's in it
{{changelog}}

## Signing and verification
- Code signature: Loupist's own self-signed certificate, SHA-256 `{{certificate_sha256}}`. Not an Apple Developer ID. Not notarized.
- Updates: signed with the Loupist updater key and installed only after you confirm.
- Checksums: `SHA256SUMS`. Build provenance: `gh attestation verify <file> --repo loupistai/loupist`.
- `RELEASE-METADATA.json` records the source commit, the workflow run and the signing mode.

{{qualification}}

한국어 설치 안내: https://github.com/loupistai/loupist/blob/main/installation.md
