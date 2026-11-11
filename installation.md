# Install Loupist

Loupist is installed as a desktop application, not as a Codex or Claude plugin. The first release supports macOS 13 or newer on Apple Silicon only.

## macOS 13 or newer on Apple Silicon

Signed installers are published through [GitHub Releases](https://github.com/loupistai/loupist/releases). Until a release is available, use the [development setup](apps/loupist/README.md).

1. Download the signed `.dmg` for `aarch64-apple-darwin` from the latest `loupistai/loupist` GitHub Release.
2. Verify the published SHA-256 checksum.
3. Drag Loupist to Applications and launch it normally. The release must pass Apple notarization and stapling checks.

Windows, Intel Macs, and Linux are not supported in the first release. Windows implementation is retained for future development; no Windows installer or release date is offered.

## Updates

Loupist checks the stable signed update feed at most once per day and also provides **Check for updates** in Settings. Download, installation, and restart always require user confirmation. Prereleases are never offered on the stable channel.

Data from Loupist 0.0.1 is upgraded in place on first open, after a backup is written beside the database. Earlier development formats are refused and left unchanged.
