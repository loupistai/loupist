# Loupist licensing

**pre-release; counsel review pending.** This explains the project's current choices.
The linked license and agreement texts govern; this page is not a replacement for them.

## App code

Loupist's app code remains open source under **AGPL-3.0-only**; see [LICENSE](LICENSE).
Commercial use is permitted under that license. Distribution carries source and notice
obligations. Section 13 also requires a modified version that supports remote network
interaction to prominently offer those users its corresponding source, at no charge.
Running a local desktop app does not by itself require publishing private research,
user data or packs. [AGPL legal text](https://www.gnu.org/licenses/agpl-3.0.html).

## Contributions and the Project Steward

The [Contributor License Agreement](CLA.md) leaves contributors owning their work and
grants the Project Steward additional rights, including commercial use, sublicensing,
and relicensing contributions under open-source, source-available, commercial or
proprietary terms. It does not withdraw the public license granted for a released
version. The CLA's legal identity and acceptance process still need counsel review.
Publishing a pack is separate from contributing it to the app repository: neither a
pack upload nor a Gallery listing alone accepts the code CLA.

## Names and logos

The code license does not grant permission to use Loupist's name, logos or other marks
to imply affiliation or endorsement. See [TRADEMARKS.md](TRADEMARKS.md).

## Shared packs

World and Lens packs can be published openly on GitHub. Their content has its own
license, separate from the app code. Rust defaults a new share to **CC-BY-NC-SA-4.0**.
CC BY 4.0, CC BY-SA 4.0, CC0 1.0 and CC BY-NC 4.0 remain available where compatible.

NC limits licensed uses to purposes not primarily directed toward commercial advantage
or payment. It is not a blanket permission to use content inside a commercial product.
An NC pack is publicly available, but is not unrestricted open content or an open-source
software license. The rights holder can separately authorize commercial use; publishing
under CC does not revoke existing compliant recipients' rights.
[CC BY-NC-SA legal text](https://creativecommons.org/licenses/by-nc-sa/4.0/legalcode).

Loupist applies this conservative pack-combination policy to included material:

| Included item's license | Offered pack licenses |
| --- | --- |
| CC0 or CC BY | Any of the five public pack licenses |
| CC BY-SA | CC BY-SA only |
| CC BY-NC | CC BY-NC or CC BY-NC-SA |
| CC BY-NC-SA | CC BY-NC-SA only |
| Personal | Item excluded from every later share |

Rust intersects these choices across the selection. An empty intersection refuses
export with `license_conflict` and names affected items. Original rights remain in
force: selecting a pack license does not erase upstream attribution. The manifest and
README carry Credits from stored origins, including pack name, author, license, link
and version when supplied. NC READMEs state the non-commercial term.

## Non-redistributable packs

`LicenseRef-Loupist-Personal` identifies non-redistributable content with separately
supplied terms. It is not an open license. A manifest using it must provide
an HTTPS `termsUrl` with the applicable terms. Import accepts it and exposes its terms
and non-redistributable status for review. Accepted material keeps that restriction
through updates and is excluded and counted in later exports. GitHub publishing and
Gallery listing refuse Personal packs with `license_not_shareable`.
Adding public evidence does not remove a stored restriction on a World name or
relationship. Removing an import does not free retained Lens or profile content.

## Provider data

A pack license cannot grant rights the sharer does not hold in third-party data. The
Rust export table in
[provider_policy.rs](apps/loupist/src-tauri/src/sharing/provider_policy.rs) applies to
all packs. Government, regulator and
official statistics providers are `public_record`; commercial, exchange, scraped and
unknown providers are `restricted`. Public-record classification is an application
export policy, not a claim that every underlying document is copyright-free.

Only public-record Sources, explicit user-authored statements and eligible imported
pack references may leave. Account captures, uploads and custom-connection results do
not become user-authored merely because the user saved them. Web pages travel only as
references. No retained source text is exported. A relationship supported solely by
restricted or private Sources is excluded and counted. Non-redistributable imported
items have a separate exclusion count. The connector documentation maintains the full
table and native aliases.

## Gallery listing

Gallery listing is unavailable unless the build supplies a valid HTTPS
`LOUPIST_GALLERY_TERMS_URL`. A request must explicitly accept that exact URL. Rust dates
and records acceptance with the share identity and published commit, and includes the
URL and UTC acceptance date in the listing pull request. Listing terms do not replace a
pack's content license or grant additional redistribution rights.

## Third-party notices

Dependencies and bundled assets retain their own licenses. Read [NOTICE](NOTICE) and
[the app's third-party notices](apps/loupist/THIRD_PARTY_NOTICES.md). This page does not
relicense those materials or provider data.
