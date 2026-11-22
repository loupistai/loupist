# Loupist {{version}}{{channel_suffix}}

Loupist is an open-source AI research desktop for investors, for macOS 13 or newer on Apple Silicon. It learns as it works, checks the figures it cites against their sources, and keeps your research on your Mac.

Download **Loupist-macOS-arm64.dmg** from the assets below. Open the disk image, drag Loupist into Applications, and open it there. The app is signed with Developer ID and notarized by Apple. If macOS asks whether to open an app downloaded from the internet, click **Open**.

By downloading Loupist you accept the [disclaimer](https://github.com/loupistai/loupist/blob/main/README.md#disclaimer).

If you’re updating from 0.1.2, macOS asks once for access to each saved Data connection key because the signing identity has changed. Enter your Mac login password and choose **Always Allow**. Later updates from the same Developer ID team should retain access without another prompt. If you chose **Deny**, the key stays saved and you can choose **Try again** in Settings → Data connections. A saved GitHub sharing token can also prompt once. AI sign-ins are unaffected.

## What's in it

{{changelog}}

## Verify the download

Compare the disk image’s SHA-256 with `SHA256SUMS`. The source commit, signing certificate fingerprint and Apple team are in `RELEASE-METADATA.json`. To verify the build provenance, run:

```sh
gh attestation verify ~/Downloads/Loupist-macOS-arm64.dmg --repo loupistai/loupist \
  --signer-workflow loupistai/loupist/.github/workflows/release.yml
```

Updates use the existing Loupist updater key and install after you confirm. For more detail, see the [installation guide](https://github.com/loupistai/loupist/blob/main/installation.md).

{{qualification}}
