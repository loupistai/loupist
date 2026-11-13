# Releases

Version, tags and the manual qualification before an update is published. CI gates stay in [CI and releases](ci-cd.md).

## Release

Version is SemVer. Tags are `vX.Y.Z` or `vX.Y.Z-rc.N` and must match Cargo, npm, lockfiles, Tauri, and the base macOS plist version. Use the checked-in version helper rather than hand-editing those copies:

```sh
python3 apps/loupist/scripts/set-version.py --version X.Y.Z-rc.N
python3 apps/loupist/scripts/set-version.py --check --version X.Y.Z-rc.N
```

The command rejects an already-diverged tree before writing any file, updates every authored version and lockfile copy together, and keeps `Info.plist` at the SemVer base version for RCs. Use `--dry-run` to review a transition first.

The release workflow also runs `apps/loupist/scripts/check-market-calendar.mjs`: a
tag is refused while `market-calendar.json` lacks the current year for an exchange.
Add each exchange's holidays only from its own published calendar.

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

## Signing modes and rehearsal

`apps/loupist/release-profile.json` is part of the tag: `channel` is `beta` or
`stable`, and `macos_signing.mode` is `self-signed` or `developer-id`.
The default Beta pins the public certificate's SHA-1 and SHA-256 and common name;
`apps/loupist/release-signing-certificate.pem` contains only that public certificate.
Switching modes requires a source change and a new tag, never a mutable repository
variable. Local staging with no mode keeps its existing behavior.

| Mode | Release credentials | Package assertions |
| --- | --- | --- |
| Self-signed | Three updater secrets and `LOUPIST_SELF_SIGNED_CERTIFICATE`, `LOUPIST_SELF_SIGNED_CERTIFICATE_PASSWORD` | Pinned certificate root and leaf, no Apple team or stapled ticket, per-executable runtime flags and entitlements |
| Developer ID | Three updater secrets and six `LOUPIST_APPLE_*` secrets | Apple chain and team, notarization and stapling, existing sidecar entitlements |

The updater secrets are `LOUPIST_UPDATER_PUBLIC_KEY`,
`LOUPIST_TAURI_SIGNING_PRIVATE_KEY`, and
`LOUPIST_TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. All modes retain updater signatures,
the closed asset manifest, checksums, and build provenance. `RELEASE-METADATA.json`
schema 1 records `macos_signing`: mode, notarized, team ID or null, certificate
SHA-256, and the app's designated requirement. A release with an Apple Team ID
admits only
helpers with that team; a teamless signed release requires the helper's actual
leaf certificate SHA-256 to match the running app's own signature. Ad-hoc apps
cannot establish a shared signer and refuse all bundled helpers; ad-hoc mode is
therefore not selectable for a release. Unsigned development/CI staging remains
available, without claiming release runtime trust. Qualification compares these
facts to both the tag's profile and the mounted app. No unnotarized build claims
Apple notarization; Gatekeeper output is recorded without gating on runner policy.

Before tagging, after a green exact-commit main run and the read-only setup audit:

```sh
gh workflow run release.yml --ref main -f dry_run=true
# Optional: -f run_full_ci=true
```

The rehearsal builds with a certificate and updater key generated inside the job,
imports the certificate through the real base64/keychain path, stages and signs,
verifies the mounted DMG and runs sidecar checks, assembles and verifies all assets,
and runs the app's headless `--check-bundled-helpers` entry point from the mounted
image through the real Pi, MCP and Python host launch paths. It uploads
`loupist-assembled-release` and `loupist-signing-report` for one day.
It uses `release-dry-run`, receives no release secrets, and publishes or attests
nothing. Metadata mode `dry-run-ephemeral` is accepted only with the explicit
`verify-release-assets.py --allow-dry-run` flag and cannot be qualified/promoted.
Tag identity, RC sequencing, changelog and exact-commit CI gates are skipped only
for rehearsal. Version consistency, market calendar, resource/configuration checks
and build-number allocation still run. A normal tag run repeats full CI; rehearsal
does so only when requested. A dry run must be green before the first tag, and
after workflow, staging or Tauri configuration changes.

For a tag whose push started nothing, dispatch the existing tag explicitly:
`gh workflow run release.yml --ref vX.Y.Z-rc.N -f dry_run=false`.
A non-dry dispatch from a branch is refused. A burned RC tag may be skipped only
if every skipped number exists as a tag without a published release, and the new
number exceeds every published RC. Never move or delete the burned tag. An
installed RC whose next candidate was burned can still update through the stable
feed, though it will not see a later RC. A burned stable tag requires a higher
patch version. Publication asks for `release` approval separately from building;
inspect build verification before approving it.

Keep the updater key and bundle identifier when moving to Developer ID. Change
the profile to `developer-id` (certificate fields may be null for an unpinned
Developer ID leaf), configure its six secrets, remove the Beta About line and
update install instructions in that same release. Qualify the new path through
RCs: its notarization and sidecar signing have not yet been exercised on a runner.
The identity change can ask once for access to each saved Data/custom connection
secret and GitHub sharing token. Choosing Always Allow settles it. AI sign-ins
are files and are unaffected. The ad-hoc release fallback was removed because it
cannot satisfy helper trust.

## Candidate update qualification

Run an in-app update qualification only on disposable, isolated macOS Apple Silicon
test machines, including a disposable macOS virtual machine on Apple Silicon
with its own hosts file and keychain. Record the method used. Do not redirect `github.com` or trust a test certificate on a
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
   verify both the installed version and retained local data. When the
   candidate raises a schema version, the predecessor's catalog and Loupist
   folders must open upgraded, each with its `upgrade-backup` beside it (see
   [Storage and downgrades](#storage-and-downgrades)). An unsupported older
   database must still be present and unchanged. Capture durable,
   credential-free HTTPS evidence for the macOS candidate.
5. On the clean signed macOS candidate, complete the `LOUPIST.md` Data connections,
   Chat, and Knowledge flow, including a later turn that uses a retained Source
   or a revised Map or Lens. Capture durable credential-free HTTPS evidence
   that identifies the candidate tag and candidate-set digest; OAuth or
   connector credentials must never appear in that evidence.
6. Dispatch `release-qualification.yml` from the candidate `vX.Y.Z` tag (not
   `main`) with the macOS update-evidence URL, product-acceptance URL,
   candidate-set digest, `update_check`, `product_acceptance`, and
   `unperformed_checks`. Its receipt seals the exact
   tested asset set; use its successful run ID to dispatch `promote-release.yml` from that same candidate
   tag.

Qualification schema 2 records the channel and signing mode, method and result
(`passed` or `not_performed`) for each check, evidence URLs, the sealed candidate
digest, and one plain-text `unperformed_checks` line. It never labels a substitute
RC update as a test of the candidate. The workflow prints the receipt in its summary.

| Input | Allowed values |
| --- | --- |
| `update_check` | `candidate-on-isolated-machine`, `candidate-in-virtual-machine`, `not-performed-on-candidate` |
| `product_acceptance` | `clean-isolated-machine`, `clean-virtual-machine`, `separate-user-account`, `not-performed` |

`channel: stable` requires the first value of each choice. Beta requires product
acceptance; it can record an unperformed candidate update with an explicit
explanation and substitute evidence (for example, a real RC-to-RC update before
promotion and RC-to-stable immediately afterward). VM methods are available for
Beta; the stable channel retains its stricter
isolated-machine choices. An evidence URL is required
for each performed check; an unperformed update may carry a substitute evidence
URL or be empty. Beta always requires a non-empty `unperformed_checks` line,
including an explicit statement when no checks were omitted. Other channels
require it whenever either choice differs from its first value. A VM records its
actual method, never isolated-machine.
For Beta promotion the current draft title must end in `(Beta)` and its body must
contain the whole explanation line, allowing whitespace normalization and the
`- Not checked:` presentation prefix; a substring does not qualify. RC notes say
they are unqualified candidates and contain no acceptance placeholders.
Edit the stable draft notes before qualification and
promotion. Neither workflow replaces the candidate bytes. Their first end-to-end
execution requires a real draft and cannot be established by the build rehearsal.

Example Beta qualification after acceptance in a separate macOS account:

```sh
gh workflow run release-qualification.yml --ref v0.1.0 \
  -f candidate_tag=v0.1.0 -f predecessor_tag=v0.1.0-rc.2 \
  -f macos_candidate_set_sha256=<SHA256-of-SHA256SUMS> \
  -f update_check=not-performed-on-candidate \
  -f "macos_update_evidence_url=https://github.com/loupistai/loupist/issues/<number>" \
  -f product_acceptance=separate-user-account \
  -f "macos_product_acceptance_evidence_url=https://github.com/loupistai/loupist/issues/<number>" \
  -f unperformed_checks='<actual omitted checks and substitute test>'
gh workflow run promote-release.yml --ref v0.1.0 \
  -f candidate_tag=v0.1.0 -f qualification_run_id=<successful-run-id>
```

The public download link is `releases/latest/download/Loupist-macOS-arm64.dmg`; do not advertise a Windows installer. The app feed is `https://github.com/loupistai/loupist/releases/latest/download/latest.json`. Install and restart need explicit user confirmation. A release build sets `requireSignedVersion`, and the app treats any other updater configuration as unsupported. The signature's trusted comment has to carry the same version as `latest.json`. A rewritten feed cannot pair a new version number with an older signed package.

Replace a bad stable release with a higher patch; do not downgrade storage or silently roll back.

## Storage and downgrades

Installed data is upgraded, never reset. A release that changes the store or
Knowledge schema adds the next step to the ladder in `store/upgrade.rs`; it never
edits an earlier step or the frozen baseline fixture, because installs at every
earlier version still run them. Before the first step on each database, Loupist
writes `<database>.upgrade-backup-<UTC stamp>` beside it (the catalog in the app
data folder and `loupist.sqlite3` in each Loupist folder's `.loupist/`). After the
upgrade succeeds it keeps that backup and the newest earlier one, and deletes
older backups of the same database.
[Schema versions and upgrades](runtime/storage-authority.md#schema-versions-and-upgrades)
owns the rules.

Downgrades are not supported. A build refuses data a newer build saved: the
catalog shows a native dialog and the app quits, and a folder lists as
`newer_version`. Neither is changed. A value added to an extensible enumeration
needs no schema step, so an older build of the same schema could meet a value it
does not know; this is another reason a bad release is replaced by a higher patch.
To return a machine to an older build, quit Loupist and copy each backup over its
database first; changes made after the upgrade are lost. Only the two newest
backups remain, so this reaches back at most two schema-changing releases.
