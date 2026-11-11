# Publishing

GitHub publishing, the community Gallery and `loupist://import` links are Rust flows. No command returns the GitHub token. What a pack may contain is [Sharing a pack](sharing.md). [This release](sharing.md#this-release) keeps those flows and leaves them out of the user app: a link is dismissed, and the Gallery, publish controls and pack review do not open.

GitHub publishing (`sharing/github.rs`) uses an OAuth App device flow with scopes
`public_repo gist`. The client ID, community index URL and Gallery listing terms URL are build-time variables
(`LOUPIST_GITHUB_CLIENT_ID`, `LOUPIST_COMMUNITY_INDEX_URL`,
`LOUPIST_GALLERY_TERMS_URL`, read with `option_env!`; terms must be HTTPS and are unset
by default):
without a client ID the share sheet hides publishing and gists, leaving folder, ZIP and
Copy as text; without an index URL there is no Gallery, listing, blocked-list check or Report. Release
macOS and Windows builds keep the token in the OS keyring (service
`com.monarchjuno.loupist.github-sharing`, account `{environment}:github-sharing`); debug,
test and WebDriver builds keep it in process memory. No command returns the token; the
renderer receives only the login, which global `catalog.sqlite3` stores. Rust builds and
scans the whole pack before creating anything on GitHub, then creates a public
repository with topic `loupist-pack` and one commit "Loupist pack v{n}", or a public or
secret gist for one item. Update refuses a recorded repository whose default branch
holds another pack ID or none (`repo_mismatch`). A 401 deletes the token and signs out
(`publish_denied`). GitHub, archive and index requests run in Rust without redirects. GitHub JSON replies stop at 4 MiB while streaming, including replies without a content length.

The community index (`sharing/gallery.rs`) is fetched over https, at most 4 MiB, kept
in app data (`gallery-cache/`) for offline use and refreshed after six hours or on
Refresh; a failed fetch keeps a fallback marked offline without renewing its freshness, so the next check retries; the fallback remains in memory so its cards and cached images can still open offline. An entry that does not validate is left out on its own, and anything else wrong
rejects the document. Entries on its `blocked` list never appear, and `pack_list` marks
added packs it names. Map images come only from `raw.githubusercontent.com` at the
listed commit: PNG, signature checked, at most 1 MiB each, 64 MiB kept. A Gallery
listing request requires configured terms and explicit acceptance of that exact URL.
`github_account` exposes `galleryTermsUrl` (or null). Rust reads the manifest at the
recorded published commit, refuses Personal-licensed content with
`license_not_shareable`, and records the accepted URL and UTC date with the share
identity and commit in the additive `share_listing_acceptances` table (no schema
version change). It then forks the index repository into the user's account, commits
one entry file there and opens a pull request whose body states that acceptance; it
writes nowhere else. Index entries under the Personal license are not displayed.
Missing acceptance is refused by Rust with `gallery_terms_required`. A fork's update check
(`sharing/update.rs`) reads the pack's own repository or gist the same way and reuses a
result for ten minutes.

`loupist://import?url=…` links arrive through `tauri-plugin-deep-link` (macOS and
Windows, not e2e builds; scheme in `tauri.conf.json`) and the single-instance plugin's
`deep-link` feature, which hands a second launch's link to the running app. Rust
(`sharing/deep_link.rs`) accepts only host `import` with one `url` parameter that passes
the [link allowlist](../capabilities/packs.md#shared-loupist-packs), holds at most one link
(a newer one replaces it) and emits `deep-link-import` (`ok` with the source label, or
`bad`). When pack sharing is shown, the renderer asks only after onboarding; nothing is fetched before Preview, and
nothing is added before the review's Add. This release dismisses the link instead ([This release](sharing.md#this-release)).

**Updates** (`sharing/update.rs`). A pack added from a link is checked for a newer version
when that section opens, at most once a day (Rust keeps the date; nothing checks in the
background), or on Check for updates; successful results are reused for ten minutes, while an offline result consumes neither that cache nor the daily check. A rate-limit refusal consumes the daily automatic check, while the manual button can retry. A pack added from a ZIP or folder takes a newer ZIP
or folder the user chooses. The newer version is staged through the same validation and
scans as a first import and compared with the retained copy of the version added (or,
when that copy is gone, with what the import recorded). Nothing changes until the user
applies a selection: new items arrive as pending suggestions; a changed item still
pending takes the newer text; a changed accepted item shows yours beside theirs and takes
the author's text only when chosen, and a chosen change with a scan finding waits as a
"Changed" suggestion to be accepted on its own. Whatever was not chosen in that version
is declined, and a version with nothing to offer moves the import forward silently. The
map gains the new relationships (reusing the pack's existing Sources) through the same
merge, and relationships the author withdrew are marked withdrawn; an update never
grants anything, and a blocked version cannot be applied.

Sharing again a Loupist that started from a pack writes the origin chain (each earlier
pack, author and version) into the new pack's manifest.

**Gallery** (`sharing/gallery.rs`). When the build names a community index
(`LOUPIST_COMMUNITY_INDEX_URL`) and the user app is showing pack sharing, New Loupist offers Browse the Gallery: listed packs with
name, author, focus, counts, license and an optional map image, sortable and searchable,
kept for offline use. The list scrolls in the page, including below 1060px, and focusing a card below the fold scrolls it into the page. Preview opens the ordinary review and Use adds it as above; the
renderer never sees a pack's location, which Rust resolves from the listing id. The Share
sheet offers List it in the Gallery only when the build names HTTPS listing terms: the user
ticks "I agree to the Gallery listing terms" (linked to that URL), and the request carries
acceptance of that exact URL ([Trust](../architecture.md#trust)). Rust then forks the index
repository and commits one entry file
(`entries/<owner>__<repo>.json` with only `{repo, path, commit}`) and opens a pull
request; the index's CI validates the pack at that commit and nobody reviews by hand
([Trust](../architecture.md#trust)). Without an index there is no Gallery, listing, Report
or blocked-list check.
