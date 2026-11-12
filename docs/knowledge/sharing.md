# Sharing a pack

A pack is a folder, ZIP or public GitHub repo of selected Memory. Scan and license checks run before anything leaves the machine. Adding someone else's pack is [Pack add](pack-add.md). GitHub and the Gallery are [Publishing](publishing.md).

## This release

Pack sharing stays in the program. The user app leaves out Share on Lens and in Settings › Loupists, the Gallery, "use a link or file", pack review, the list of packs already added, and suggestions from those packs. A `loupist://import` link is dismissed, and no dialog opens. There is no settings switch, and no address opens those screens. Map and Lens reading stay, including "Shared · {pack}" on a name or page that already came from a pack. Map image and replay export stay under More → Export. Importing a local Loupist folder stays. Rust (`src-tauri/src/sharing/`, `src-tauri/src/knowledge/shared_pack.rs`) still owns the format, scans, network calls and storage. The screens in `src/components/sharing/` stay, and the development preview harness can still open them. Set `SHARING_UI_IN_RELEASE` in `src/sharing/releaseVisibility.ts` to show the entry points again.

## Sharing

A Loupist can be shared as a `loupist-pack/1` pack: a folder, a ZIP, a public GitHub
repository, or a gist holding one method or principle. Rust (`src-tauri/src/sharing/`,
`src-tauri/src/knowledge/shared_pack.rs`) owns the format, scans, network calls and
credentials; the renderer receives counts, findings (a class and item label, never the
matched value) and validated text. [Pack bounds and scans](../capabilities/packs.md#shared-loupist-packs)
own file and text rules; [Trust](../architecture.md#trust) owns GitHub, deep links and how
shared text reaches the model.

When the entry points are shown, Share opens from "What {Loupist} knows" and each Settings › Loupists row; Share on a
method in the Lens reader, or on a principle on "What {Loupist} knows", shares that item
alone. [This release](#this-release) leaves those controls out of the user app. A pack carries the Profile (name,
focus, tone, avatar), a standing perspective other than the default, active methods and
principles, and the Map's names, relationships and Sources, each section switchable.
Sources travel as references (title, host, URL, EDGAR or DART filing ID, DOI, publication
date) without bytes or quotes. Record IDs never leave: items carry keys derived from the
pack ID, and `lens:` links keep only their label. The export reads nothing else, so
holdings, what you follow, Journal, Chats, notes, retained bytes, datasets, artifacts,
Skills, the Loupist's instructions, connections, schedules and settings never enter a
pack. An oversized method stays in the selection until explicitly left out. Export validates the finished pack with the importer’s bounds and refuses content that would not import, including Markdown over 64 KiB or a JSONL record over 8 KiB; the error names the oversized items by title and key. Stored Source URLs are normalized before validation, retaining URLs with stray surrounding whitespace.

Source export checks both recorded authorship and the Rust
[provider data policy](../capabilities/packs.md#provider-data-policy-for-packs).
Only `public_record` publisher Sources, explicit user-authored statements and imported
pack references with stored redistribution rights may leave. A user-owned capture is
not necessarily user-authored: uploads, workspace files, brokerage reads and custom
connections stay private. Web pages remain references only, without their text.
Commercial, exchange, scraped and unknown provider data is restricted even when the
capture says `publisher`. A relationship leaves with eligible Sources only; one whose
every Source is private or restricted is left out and counted. A name leaves only with
a relationship.

Before a save or publish, Rust scans every string that would leave. A store, SQL,
row-decoding, provider-auth-file, custom-connection-file or credential read error
blocks sharing; an absent credential stays empty. If saved credentials cannot be
checked, sharing asks the person to unlock the Keychain or reconnect and try again;
it never scans with a partial credential list. Saved credentials of eight or more
characters match substrings. Credentials of four to seven characters match only
whole tokens or the string ends. For an ASCII credential, only ASCII letters and
digits join it into a longer token; attached Korean particles and escaped control
characters such as `\n` are separators. Non-ASCII credentials retain Unicode
alphanumeric boundaries. The credential scan checks the original text and one
percent-decoded form, so URL-encoded separators cannot hide a saved key. Values
shorter than four characters are not matched and do not block sharing. Ordinary
short settings such as ports, locales and modes are not credential substrings.
GitHub Keychain read failures also block the scan; short finance credential fields
enter the same saved-credential matching rules instead of being discarded. Credentials
(including every stored provider key or sign-in token, custom-connection secret,
connector credential and the GitHub token), account numbers and a holding's exact amount
next to its name block until that item is left out; an email, phone number, the macOS
full name or home path warns and can be accepted ("It's fine"). Export rebuilds the pack
and refuses unresolved findings (`findings_unresolved`). The first share mints the
Loupist's pack ID, and the version rises when the content hash changes, including
license and attribution changes. An unchanged saved share recovers its actual
license from the content hash for the summary, so a nondefault license does not
spuriously advance the version. Rust defaults new shares to `CC-BY-NC-SA-4.0` and
computes allowed licenses over the selected items and their stored origins: CC0 and BY
allow any public pack license, BY-SA only BY-SA, BY-NC either BY-NC or BY-NC-SA, and
BY-NC-SA only BY-NC-SA. `share_summary` accepts sections and excluded item keys and
returns `allowedLicenses` as `{id, url}` entries with legal-code URLs, `defaultLicense`
and typed `licenseConflict` item details.
Export refuses an empty intersection or an incompatible requested license with
`license_conflict`; it never silently changes an explicit choice. Credits in the
manifest and README retain origin pack names, authors, licenses, links and versions. Shared labels from earlier stored rows also drop hidden characters when delivered to the model.
NC READMEs state the non-commercial term. See [Licensing](../../LICENSING.md).

`LicenseRef-Loupist-Personal` marks non-redistributable content and requires an
HTTPS `termsUrl`. Import accepts it, while the public share flow, GitHub publishing
and Gallery listing refuse it with `license_not_shareable`. Accepted Lens and World records
keep non-redistribution marks in existing license columns and origin JSON; updates
preserve earlier restrictions. World names and relationships retain their own rights
even when public evidence is added. A profile inherited when creating a Loupist retains
its rights after the import is removed. Exports exclude them and relationships supported only
by them, with a separate `leftOutLicense` count ("Left out: n items whose license
doesn't allow sharing"). The renderer receives these policies as typed data from Rust.
The Share sheet offers only `allowedLicenses` (the chosen license's name links to its
legal-code URL from Rust, then one plain line about what anyone may do), starts at `defaultLicense` and asks Rust again whenever a
section or left-out item changes; one allowed license is shown fixed with the reason. A
`licenseConflict` replaces the choice with the clashing items, each with Leave it out,
and keeps every output off until Rust allows a license again.

Retained import copies under `.loupist/packs/<import>/<version>` are derived update
baselines. A removed import loses its copies; an applied newer version prunes older
copies after commit, retaining the version still referenced by an active import.
Accepted text and origin rights remain in SQLite independently of those files.
Per-result shared-text provenance reads query only the delivered record IDs; a
single result may contain records from several packs.

A failed GitHub publish never deletes a repository. Device sign-ins request only
`public_repo gist`, without repository-deletion authority. If this attempt created
a repository, the error includes its name and GitHub URL so the person can inspect
or remove it there. A successful upload followed by a local record error is
reported as uploaded, rather than claiming nothing was published.

A failed creation is not reused by name: new repositories are automatically
initialized with a README, which counts as content even if no pack was published.
The app has no durable receipt proving ownership of an uninitialized failed
creation, so existing repositories remain refused, including empty ones. Retry
with a new name or remove the failed repository on GitHub first; the error does not
promise that an existing repository will be reused.

## Code

- Sharing (packs, GitHub, import, deep links): `src-tauri/src/sharing/`, `src-tauri/src/knowledge/shared_pack.rs`, `src/components/sharing/`, `src/contracts/sharing.ts`, `src/bridge/mock/sharing.ts`
