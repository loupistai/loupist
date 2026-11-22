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
release workflow takes the larger of that number and one more than the highest
`RELEASE-METADATA.json` counter among up to 1,000 releases its read-only token
can list (published releases and prereleases, excluding drafts). The product SemVer remains the
updater version; the separate build counter is recorded in `RELEASE-METADATA.json`
and the qualification workflow compares it to the signed candidate DMG. A failed job may already
have created the release because verification follows publication. Before recovery,
inspect the tag’s release ID and assets and, for a published release, repeat the
read-only check with `apps/loupist/scripts/verify-published-release.sh TAG`.

1. Set the product to `X.Y.Z-rc.N`, commit it, push it to `main`, and wait for the exact commit's CI run to pass. Create `vX.Y.Z-rc.N`; the workflow publishes that non-draft prerelease.
2. On clean machines, complete the acceptance flow in `LOUPIST.md`: ordinary Chat retains Source material or revises Lens, and a later relevant turn uses that revision. A write that never changes later work does not qualify.
3. After RC acceptance, set the product to `X.Y.Z`. In `CHANGELOG.md`, manually
   rename `## Unreleased` to `## X.Y.Z - YYYY-MM-DD` using the release date.
   The stable workflow only validates the dated heading. Commit both changes,
   push, and wait for that exact commit's CI run to pass.
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
The 0.1.3 Beta selects Developer ID. Before a real tag, replace the team placeholder in `apps/loupist/release-profile.json` and both publisher instructions in `installation.md`. The tag pins the public team through import, artifact verification and qualification. Its three certificate fields are null so the
team can renew its Apple-issued leaf. Self-signed mode requires a common name
and both fingerprints in the tag’s profile.
Switching modes requires a source change and a new tag, never a mutable repository
variable. Local staging with no mode keeps its existing behavior.

| Mode | Release credentials | Package assertions |
| --- | --- | --- |
| Self-signed | Three updater secrets and `LOUPIST_SELF_SIGNED_CERTIFICATE`, `LOUPIST_SELF_SIGNED_CERTIFICATE_PASSWORD` | Pinned certificate root and leaf, no Apple team or stapled ticket, per-executable runtime flags and entitlements |
| Developer ID | Three updater secrets and six `LOUPIST_APPLE_*` secrets | Apple anchor, Developer ID Application certificate, configured team and secure timestamps, exact runtime flags and entitlements, stapled app and DMG |

The updater secrets are `LOUPIST_UPDATER_PUBLIC_KEY`,
`LOUPIST_TAURI_SIGNING_PRIVATE_KEY`, and
`LOUPIST_TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. All modes retain updater signatures,
the closed asset manifest, checksums, and build provenance. `RELEASE-METADATA.json`
schema 1 records `macos_signing`: mode, notarized, team ID or null, certificate
SHA-256, and the app's designated requirement. A release with an Apple Team ID
requires both the app and its helpers to satisfy a strict requirement for the Apple anchor, Developer ID Application leaf extension and the same team OU; a teamless signed release requires the helper's actual
leaf certificate SHA-256 to match the running app's own signature. The app finds
its resources and that signature from its own executable path with symlinks
resolved once at start, so an app opened through a symlinked folder (`/tmp`) still
finds its helpers; Tauri's `resource_dir` refuses such a path. Ad-hoc apps
cannot establish a shared signer and refuse all bundled helpers; ad-hoc mode is
therefore not selectable for a release. Unsigned development/CI staging remains
available, without claiming release runtime trust. Qualification compares these
facts to both the tag's profile and the mounted app. For teamless builds, Gatekeeper output is recorded without gating on runner policy.

Before tagging, after a green exact-commit main run and the read-only setup audit:

```sh
gh workflow run release.yml --ref main -f dry_run=true
# Optional: -f run_full_ci=true
```

The rehearsal builds with a certificate and updater key generated inside the job,
imports the certificate through the real base64/keychain path, signs every materialized
Mach-O with the same inside-out traversal as a tag, checks library load paths,
verifies the mounted DMG and runs sidecar checks, assembles and verifies all assets,
and runs the app's headless `--check-bundled-helpers` entry point from the mounted
image through the real Pi, MCP and Python host launch paths. It extracts the
updater archive and compares its sealed app with the build. Teamless signing omits
hardened runtime on Pi and OpenBB so they can load their libraries. The rehearsal
records that Apple submission and stapling were skipped. It uploads
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

Keep the updater key and bundle identifier across the transition from 0.1.2.
The six Apple secrets in the `release` environment are:

- `LOUPIST_APPLE_CERTIFICATE` (base64-encoded .p12) and `LOUPIST_APPLE_CERTIFICATE_PASSWORD`
- `LOUPIST_APPLE_SIGNING_IDENTITY` (the full `Developer ID Application: Name (TEAMID)` common name) and `LOUPIST_APPLE_TEAM_ID`
- `LOUPIST_APPLE_ID` and `LOUPIST_APPLE_PASSWORD` (an app-specific password)

Import matches the configured name and team to the leaf with the private key,
checks the signing chain and uses that leaf’s SHA-1 identifier for codesign.
If the new keychain lacks the issuing intermediate, import can fetch the pinned
public Developer ID intermediates from Apple. The rehearsal checks its disposable leaf's
common name and OU, imports the public intermediates, and verifies the disposable
leaf still lacks trusted identity status. The published DER for the Developer ID G2
intermediate hashes to SHA-256
`f16cd3c54c7f83cea4bf1a3e6a0819c8aaa8e4a1528fd144715f350643d2df3a`.
[Apple's Developer ID G2 intermediate](https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer).

The signed build stages managed standalone Python for OpenBB and signs each
Mach-O after symlinks have been materialized. Libraries, executable bundles and
helpers are signed inside out. Tauri builds the app bundle without signing. The release
script seals the outer app with hardened runtime and no entitlements. Only Pi and the
WASI worker receive their respective executable-memory entitlements. A post-build
step stores the Apple credentials once in the temporary keychain. On macOS arm64,
notarytool 1.0.0 (37) accepted a dummy password on piped stdin with no controlling
terminal (`start_new_session=True`). With `--no-validate`, it exited 0, reported
“Credentials saved to Keychain.” on stdout and printed the app-specific-password
prompt on stderr. The rehearsal repeats this before building and checks the saved
profile by account and service in the job keychain. A real tag uses Apple's default
credential validation. The release script
uses that keychain profile to submit the app ZIP and staple the accepted app. It
archives the stapled app for the updater and copies it into a drag-to-Applications
disk image. It signs and submits the DMG, then staples it. Both submissions print
their IDs before a bounded wait and fetch Apple's log on rejection. Tauri's signer
binds the final updater signature to the release version. Build verification and qualification compare the
extracted updater’s sealed app to the reference app and validate its stapled ticket.

Developer ID signing and notarization have not yet been exercised on a Loupist
runner. **Pending qualification:** record the first real tag, the Gatekeeper dialog
on first opening a quarantined download, startup of all hardened helpers, and the
once-per-key prompts when updating from 0.1.2. Only the following update signed by
the same team can establish that choosing Always Allow prevents the prompt from
returning. Check a saved GitHub sharing token as well. AI sign-ins are files.

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
   or a revised World or Lens. Capture durable credential-free HTTPS evidence
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

The public download link is `releases/latest/download/Loupist-macOS-arm64.dmg`; do not advertise a Windows installer. The app feed is `https://github.com/loupistai/loupist/releases/latest/download/latest.json`. Install and restart need explicit user confirmation: a native alert attached as a sheet to the Loupist window (brought forward first), worded in the interface language from the renderer's `confirmation` text while the host bounds it and appends the versions line itself; the Updates page says to answer it there. The offer's release date reaches the renderer as RFC 3339 and shows as a local date. A release build sets `requireSignedVersion`, and the app treats any other updater configuration as unsupported. The signature's trusted comment has to carry the same version as `latest.json`. A rewritten feed cannot pair a new version number with an older signed package.

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
