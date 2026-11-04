# Releases

Version, tags and the manual qualification before an update is published. CI gates stay in [CI and releases](ci-cd.md).

## Release

Version is SemVer. Tags are `vX.Y.Z` or `vX.Y.Z-rc.N` and must match Cargo, npm, lockfiles, Tauri, and the base macOS plist version. Use the checked-in version helper rather than hand-editing those copies:

```sh
python3 apps/loupist/scripts/set-version.py --version X.Y.Z-rc.N
python3 apps/loupist/scripts/set-version.py --check --version X.Y.Z-rc.N
```

The command rejects an already-diverged tree before writing any file, updates every authored version and lockfile copy together, and keeps `Info.plist` at the SemVer base version for RCs. Use `--dry-run` to review a transition first.

Release, qualification, and promotion each use a non-cancelling serialized queue.
`GITHUB_RUN_NUMBER` is a lower bound for the macOS `CFBundleVersion`, and the
release workflow allocates the larger of that number and one more than every
retained `RELEASE-METADATA.json` build counter. The product SemVer remains the
updater version; the separate build counter is recorded in `RELEASE-METADATA.json`
and the qualification workflow compares it to the signed candidate DMG. Every
published release therefore gets a new build number. A rerun of a failed run may
reuse its number, because the publish step refuses to replace an existing release,
so the failed attempt published nothing under that number.

1. Set the product to `X.Y.Z-rc.N`, commit it, push it to `main`, and wait for the exact commit's CI run to pass. Create `vX.Y.Z-rc.N`; the workflow publishes that non-draft prerelease.
2. On clean machines, complete the acceptance flow in `LOUPIST.md`: ordinary Chat retains Source material or revises Lens, and a later relevant turn uses that revision. A write that never changes later work does not qualify.
3. After RC acceptance, set the product to `X.Y.Z`, ensure the matching
   `CHANGELOG.md` heading has its final ISO date (replace `Unreleased` if
   present), commit, push, and wait for that exact commit's CI run to pass.
   Create `vX.Y.Z`; its workflow requires the matching published RC and creates
   the stable **draft** candidate.
4. Qualify that stable draft: matching RC → stable for the first stable release,
   then current Latest → strictly newer stable on macOS Apple Silicon. Scripts:
   `verify-release-assets.py`, `serve-update-candidate.py`,
   `release-qualification.py`.
5. Promote the qualified stable draft with `promote-release.yml`. `stable-release` is the environment that approves that promotion. Promotion does not build or upload files.

## Candidate update qualification

Run an in-app update qualification only on disposable, isolated macOS Apple Silicon
test machines. Do not redirect `github.com` or trust a test certificate on a
developer's daily-use machine or on a production network.

1. Download the unchanged stable draft assets and run `verify-release-assets.py`
   with the candidate tag and source commit. Record the SHA-256 printed by
   `release-qualification.py digest --release-metadata RELEASE-METADATA.json`.
2. In each isolated test machine, arrange for only that machine's
   `https://github.com` traffic to resolve to its loopback address, and trust a
   disposable TLS certificate whose subject alternative name is `github.com`.
   The local candidate server binds to `127.0.0.1` and needs port 443 because the
   signed app has a canonical, portless GitHub updater endpoint.
3. Serve the verified candidate assets for the duration of the test:

   ```sh
   sudo python3 apps/loupist/scripts/serve-update-candidate.py \
     --assets /absolute/path/to/candidate-assets \
     --certificate /absolute/path/to/github-com-test-cert.pem \
     --private-key /absolute/path/to/github-com-test-key.pem
   ```

4. On a clean machine, install the published predecessor. The matching RC is
   valid only for the first stable release; afterward the predecessor must be
   the current Latest release and the candidate version must be strictly newer.
   Use the app's explicit update flow to install the candidate, restart it, and
   verify both the installed version and retained local data. An unsupported
   older database must still be present and unchanged. Capture durable,
   credential-free HTTPS evidence for the macOS candidate.
5. On the clean signed macOS candidate, complete the `LOUPIST.md` Connections,
   Chat, and Knowledge flow, including a later turn that uses a retained Source
   or a revised World or Lens. Capture durable credential-free HTTPS evidence
   that identifies the candidate tag and candidate-set digest; OAuth or
   connector credentials must never appear in that evidence.
6. Dispatch `release-qualification.yml` from the candidate `vX.Y.Z` tag (not
   `main`) with the macOS update-evidence URL, product-acceptance URL,
   candidate-set digest, and both confirmation flags. Its receipt seals the exact
   tested asset set; use its successful run ID to dispatch `promote-release.yml` from that same candidate
   tag.

The public download link is `releases/latest/download/Loupist-macOS-arm64.dmg`; do not advertise a Windows installer. The app feed is `https://github.com/loupistai/loupist/releases/latest/download/latest.json`. Install and restart need explicit user confirmation.

Replace a bad stable release with a higher patch; do not downgrade storage or silently roll back.
