# Install Loupist

Loupist is installed as a desktop application, not as a Codex or Claude plugin. The first release supports macOS 13 or newer on Apple Silicon only.

## macOS 13 or newer on Apple Silicon

Signed installers will be published through [GitHub Releases](https://github.com/loupistai/loupist/releases). Until a release is available, use the [development setup](apps/loupist/README.md).

1. Download the signed `.dmg` for `aarch64-apple-darwin` from the latest `loupistai/loupist` GitHub Release.
2. Verify the published SHA-256 checksum.
3. Drag Loupist to Applications and launch it normally. The release must pass Apple notarization and stapling checks. On first launch, acknowledge the AI notice, connect an AI account or choose Look around first, and create a Loupist to reach Bot.

Windows, Intel Macs, and Linux are not supported in the first release.

## Updates

Loupist checks the stable signed update feed daily after a successful check, with shorter retries after failures. Settings → Updates also provides **Check for updates**. Download, installation, and restart always require user confirmation. Prereleases are never offered on the stable channel.

Compatible development databases are upgraded in place on first open, after a private backup is written beside the database. Formats older than the supported baseline, and databases from newer versions, are refused and left unchanged.
