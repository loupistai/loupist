# Loupist {{version}}{{channel_suffix}}

Loupist for macOS 13 or newer on Apple Silicon. It never trades.

## What's in it
{{changelog}}

This fallback build has an ad-hoc code signature and is not notarized by Apple. First launch requires Open Anyway in System Settings → Privacy & Security.
Every update changes the code identity: macOS may ask again for access to each saved Data connection secret.
Updates still use the Loupist updater key and require consent. Verify downloads with SHA256SUMS, RELEASE-METADATA.json and `gh attestation verify <file> --repo loupistai/loupist`.

Before promotion, record product acceptance, update evidence, and any unperformed_checks verbatim from the qualification receipt here.
