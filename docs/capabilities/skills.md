# Skills

Skills are procedures the model can load. They do not grant Memory permission or replace Lens records.

## Skills management

Fixed app procedures are `world`, `lens`, `journal` and `visualize`. Optional
workflows include research, financial analysis, file authoring and Skill
creation/installation. Four retail workflows, on by default, answer everyday questions
with every figure from `calculate`: `dividend-income` (history, payout, cuts, growth,
dated yield and record dates; Korea through `dart_dividends`), `fund-compare` (cost,
size, age, `holdings_overlap` and returns over one shared window; KRX ETFs by price
only), `why-it-moved` (the measured move beside the market, reported reasons, the
Loupist's labelled reading and what isn't explained) and `goal-math` (`goal_grid` over
several rates or `dca_history`, using only the numbers the user gives). The `filings`
Skill's `references/korea-statements.md` sets statement basis, periods and the Korean
disclosure and flow reads for KRX companies. Skill Creator and Skill Installer start
off in every scope until the user turns them on. The included catalog in
`src-tauri/src/agent_harness.rs` is authoritative. Optional/custom Skills use scoped TOML enablement; see
[Scoped configuration](../runtime/storage-authority.md#scoped-configuration).

Settings → Skills offers search/filter, instruction/file details, toggles, reset,
owned-skill create/edit/delete and import. Personal Skills live at global
`skills/<id>/`; Loupist Skills at `<workspace>/.loupist/skills/<id>/`. Each has `SKILL.md`
with frontmatter and optional supporting files. Included and other-scope Skills
are read-only here. Duplicate names expose precedence rather than silently merging.
A failed initial catalog read replaces the loading message with the error; Reload
retries the read and restores the list without leaving Settings. The collection
filters are a keyboard-selectable radio group rather than tabs with missing panels.

Imports accept a local folder, single-skill ZIP or exact public GitHub folder URL.
They preserve resources and reject symbolic links, unsafe paths, included names
and existing destinations. Packages are bounded to 512 files and 16 MiB, with
folder depth at most eight. Public GitHub archives are also bounded; import does
not use credentials or execute installation hooks. Use a local checkout for private
material. Draft with Loupist opens an unsent scoped Skill Creator request in a new Chat.

Changes are read again at Chat admission and apply on the next turn. Unreadable or
malformed Skills are disabled and excluded from that turn. Flat `skills/<id>.md`
files use their filename as the fallback identifier and have no package display metadata.
Skills cannot grant connection access or Memory permissions. Their bodies/references load for the
requested operation; they do not install upstream plugins or office runtimes.
[Bundled provenance](../../apps/loupist/agent/skills/NOTICE.md) owns licenses and
adaptations. [Prompt and judgment](../agent/prompt.md) owns prompt/cache behavior.
