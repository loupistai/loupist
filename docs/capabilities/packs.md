# Shared packs

A shared pack carries Memory the author selected, within the scan and license bounds. Provider data follows the policy below. The share and add flows are [Sharing a pack](../knowledge/sharing.md) and [Pack add](../knowledge/pack-add.md).

## Shared Loupist packs

[Sharing](../knowledge/sharing.md#sharing) owns what a pack carries and how Add merges it.
These rules live in `src-tauri/src/sharing/` (`pack.rs`, `scan.rs`, `import.rs`,
`deep_link.rs`); the bounds, links, normalization and import scan apply to every pack
opened, and the export scan to every pack saved or published. The manifest accepts
CC BY, BY-SA, BY-NC and BY-NC-SA 4.0, CC0 1.0, and the non-redistributable
`LicenseRef-Loupist-Personal` (HTTPS `termsUrl` required). `credits` carry stored origin
attribution; Personal content imports but cannot be publicly shared. New exports use
the Rust-selected compatible license, defaulting to BY-NC-SA. See
[Licensing](../../LICENSING.md) for the legal boundaries.

- **Bounds:** the Skill import bounds (512 files, 16 MiB, depth at most eight) with no
  symbolic links, hidden files or executables; macOS `.DS_Store` and `__MACOSX` entries
  are skipped. A ZIP is read only up to 16 MiB, and one top folder is stripped. Only the
  format's paths are allowed: `loupist-pack.json`, `LOUPIST.md`, `README.md`, `LICENSE`,
  `preview.png`, `lens/perspective.md`, `lens/methods/<slug>.md`,
  `lens/principles/<slug>.md`, `world/schema.json` and `world/{entities,claims,sources}.jsonl`,
  or one flat `method-<slug>.md` or `principle-<slug>.md`. Any other path (a `skills/`
  folder, a settings or instruction file) or a manifest field outside the format rejects
  the pack `unsafe`. The manifest must list exactly the files present with their SHA-256
  and size, and its counts must match (`damaged`). Markdown files are at most 64 KiB, JSONL
  lines 8 KiB and `preview.png` 1 MiB with a PNG signature; a pack holds at most 200
  methods, 100 principles, 10,000 names, 8,000 relationships and 5,000 Sources, and a World
  extension of four types and six relations. Files are UTF-8, and record fields carry
  no control characters other than newline and tab.
- **Links:** only `https://github.com/{owner}/{repo}[/tree/{ref}/{path}]` and
  `https://gist.github.com/{user}/{id}`, without credentials, port, query or fragment
  (`link_invalid` otherwise; a missing or private one is `link_not_found`). A repository
  link resolves to one commit and streams that commit's archive from
  `codeload.github.com` within 16 MiB; a gist must belong to the user in the link and must
  not be truncated. No request follows a redirect, and API responses are capped at 4 MiB.
  Signed in, reads use the token and retry anonymously after a 401.
- **Normalization:** before the import checks, hidden characters (zero-width and joiners,
  bidi controls, soft hyphen, line and paragraph separators, Hangul fillers, variation
  selectors, tags) are removed, look-alikes (fullwidth, mathematical and circled letters,
  Cyrillic and Greek letters, angle-bracket and colon forms) fold to ASCII, text is
  lowercased and every whitespace run, line breaks included, becomes one space. JSON and
  YAML are decoded first, so an escape such as `\u003c` is checked as the character it
  spells. Text imitating the app's markers (`<<untrusted`, `<<shared`, their end markers,
  `safety:locked`), also through HTML entities or spacing, rejects the pack `spoof`.
- **Import scan:** every perspective, method and principle field and title, every name,
  relationship and Source string, and the pack's name, author, origin names and profile
  are scanned with English and Korean rules, reading hidden characters both as nothing
  and as a space, so a phrase split across lines or by a hidden character still matches.
  Flags are `instruction` (directives to the AI, role claims, claimed user
  authorization), `tool` (tool-catalog names, `bash`, `mcp__`, commands, home and system
  paths, instruction files, Skills, settings), `secret` (password, key, token, account and
  card words), `link` (send, post or upload phrasing, URL templates with `{`, Markdown
  images from http, `data:` URIs, base64 runs over 200 characters) and `hidden`
  (characters removed from what is added). Each flag names the lines it touches.
- **Export scan:** every string that would leave: the profile; Lens titles and fields;
  names, aliases, identifiers and summaries; propositions, attributions and qualifier
  values; Source titles, publishers, URLs and DOIs. `credential` (known key prefixes, JWT
  shapes, high-entropy runs of 32 or more characters, and exact matches of the GitHub
  token, every value in Pi's private `auth.json`, every custom-connection secret and every
  connector credential), `account` (the saved Korea Investment account number, Luhn-valid
  card numbers, IBANs, resident-ID shapes) and `holding` (a held symbol or name within 40
  characters of that holding's exact quantity, cost or value) block; `personal` (an email,
  phone number, the macOS full name or home path) warns. Secret values are compared in
  Rust only; a finding names its class and the item's label, cut before the first word
  that could carry the value.
- **Neutral title:** with Profile off, the manifest name and README title read "Shared
  Loupist" (Korean "공유된 루피스트"), the saved folder or ZIP is named from that title, and
  the repository description reads "A shared Loupist": the Loupist's name stays home.
- **Index entries** (`gallery.rs`, `loupist-index/1`, at most 5,000 entries): `repo` is
  `owner/name` with GitHub's login and repository-name rules, `path` at most six segments
  of letters, digits, `.`, `_` and `-`, `commit` 40 hex digits, `id` a UUID, `language` a
  2–8 character lowercase tag, `assetClasses` from the app's classes and `updatedAt`
  RFC 3339. Name (120 characters), focus and author (80) become one display line with
  hidden characters removed and whitespace folded; control characters reject the entry.
  An invalid entry is left out on its own, duplicates keep the first, and an entry the
  `blocked` list names is never shown. Choosing a listed pack opens the ordinary
  repository link at the listed commit, so every rule above still applies.

### Provider data policy for packs

`sharing/provider_policy.rs` is the single Rust export table. It covers every bundled
OpenBB provider and native finance/web source; access to a provider does not grant
redistribution rights. The same table applies to every pack.
Unknown providers are `restricted`. `public_record` is an application classification,
not a conclusion that every filing or statistical series is free of third-party rights.

| Classification | Providers and native aliases |
| --- | --- |
| `public_record` | SEC EDGAR (`sec`); OpenDART (`opendart`, `dart`, all native `dart_*` reads); FRED (`fred`); BLS (`bls`); BEA (`bea`); US Treasury (`us_treasury`); Federal Reserve (`federal_reserve`); ECB (`ecb`); IMF (`imf`); OECD (`oecd`); World Bank (`worldbank`, `world_bank`, `world-bank`, native `worldbank_series`); CFTC (`cftc`); `government_us`; US Congress (`congress_gov`); EIA (`eia`) |
| `restricted` | `fmp`, `intrinio`, `polygon`, `tiingo`, `alpha_vantage`, `benzinga`, `tradier`, `biztoc`, `cboe`, `nasdaq`, `tmx`, `deribit`, `finra`, KRX (`krx`), KIS (`kis`, `koreainvestment`), DefiLlama (`defillama`), `famafrench`, `econdb`, `tradingeconomics` |
| `restricted` (scraped) | `yfinance`, `seeking_alpha`, `wsj`, `finviz`, `stockgrid`, `multpl` |
| `restricted` (other/native) | `onchain`, `native-web`, `web-research`, `exa`, providerless `openbb`, every unknown provider |

No Korean public-statistics connector beyond OpenDART is bundled today. Any future
one requires an explicit reviewed table entry; its unknown identity stays restricted.
Native aliases and provider namespaces are resolved by this same Rust module.

Only public-record Sources and explicit user-authored statements are eligible;
imported pack references additionally require their stored redistribution rights.
User-owned uploads, account captures and custom-connection results are not authored
statements. Web-fetch pages are a reference-only exception: title, URL and other
metadata may leave, never the page text. No retained Source bytes or quotes leave.
A relationship whose every Source is private or restricted is excluded and counted;
non-redistributable pack material has its own `leftOutLicense` count. See
[Sharing](../knowledge/sharing.md#sharing) for item-license compatibility and Credits.
