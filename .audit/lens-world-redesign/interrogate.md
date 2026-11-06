# Interrogate at da594a6

Intent: Memory compounds from ordinary work (keyed-section Lens pages through one writer, `lens_file` gated by acknowledged reads, broader World with identity, reversible vocabulary changes, Bot hand-off, discovery) while preserving Rust-owned authority, failed-turn atomicity, as-of rules, shared-content isolation, per-Loupist isolation, no paid retries and no credentials in Memory.

Adversaries: opus and fable (Claude subagents, own git worktrees), codex `gpt-6-astra @xhigh` and grok `grok-4.7 @xhigh` (Orca workers in Orca worktrees). All at `da594a6`. Every finding below was rerun by the orchestrator and failed as reported.

## Verified findings

| ID | Sev | Finding | Found by | Repro |
| --- | --- | --- | --- | --- |
| F1 | P1 | As-of turns see Lens text written after the cutoff: summary, later sections in search hits and snippets, context index summaries, backlinks, history, idea status. | codex, opus | `lw-adv-codex` `adv_cutoff_*`; `lw-adv-opus` as-of probe |
| F2 | P1 | A `lens_file` call that staged successfully can fail the whole reply at commit (`MemoryCommitFailed`): the text policy (unknown `world:` node, unknown `about` node, `world://`, World link on a method) runs only inside `file`. | opus | `lw-adv-opus` `adv_text_policy_runs_only_at_commit`, eval `ADV_BADLINK_EVAL` |
| F3 | P1 | Bot cards never render in the real conversation: the AI SDK message adapter (`src/chat/ai-sdk.ts`) drops `vocabulary`, `link` and `handoff`. | grok | `lw-adv-grok` `src/chat/bot-cards.repro.test.tsx` |
| F4 | P2 | Parallel `lens_file` calls in one step bypass the 8-page / 40-op turn budget (counted only from delivered filings). | opus | `lw-adv-opus` parallel probe |
| F5 | P2 | A `create` that resolves to an existing page (any title for the perspective; same title otherwise) overwrites its summary and idea status without an acknowledged read. | opus | `lw-adv-opus` create probe |
| F6 | P2 | Portable pages accept a reference-style `[a]: world:id` link; `text_forbidden` matches substrings while `links()` parses Markdown. | opus | `lw-adv-opus` reference-link case |
| F7 | P2 | Control characters reach titles, headings, Markdown and summaries (old writer refused them via `validate_text`/`validate_title`). | opus | `lw-adv-opus` control-character probe |
| F8 | P2 | Undo of one op restores the filing's initial body, erasing earlier ops of the same filing (log `before_json` is per filing, not per op). | codex | `lw-adv-codex` `adv_undo_must_restore_previous_op_in_same_filing` |
| F9 | P2 | Contest state is not derived at write time: a filing citing an already withdrawn or missing claim is current, and undo of a contested section clears the mark. | fable | `lw-adv-fable` `a_filing_citing_an_already_withdrawn_claim_is_not_contested`, `undo_after_contest_keeps_the_contest_mark` |
| F10 | P2 | Memory reset keeps a Loupist-written summary on a preserved user page. | codex | `lw-adv-codex` `adv_reset_must_remove_loupist_summary_from_kept_page` |
| F11 | P2 | Shared packs: a valid local page with a Korean heading (byte vs char limit) or a leading-hyphen key exports a pack the importer rejects; replacing a shared page with an empty summary keeps the old one. | codex | `lw-adv-codex` `adv_export_valid_local_page_must_round_trip`, `adv_shared_replacement_must_remove_old_summary` |
| F12 | P2 | Undoing an old vocabulary add removes the same-named kind a later change re-added (undoability matched by name only). | fable | `lw-adv-fable` `undoing_an_old_add_does_not_remove_a_later_re_added_kind` |
| F13 | P2 | Dismissing or opening yesterday's links consumes today's three-post Bot allowance. | fable | `lw-adv-fable` `dismissing_yesterdays_links_does_not_consume_todays_allowance` |
| F14 | P2 | A committed hand-off left `created` by a crash is never launched by the delivery pass (only by the next Bot turn), contrary to the design and docs. | fable | `lw-adv-fable` `a_created_hand_off_after_a_restart_is_launched_by_the_delivery_pass` |
| F15 | P2 | "Show this path on the map" draws nothing on a Map over 60 names when the path is outside the focal neighbourhood, and issues no World read. | grok | `lw-adv-grok` `src/components/map/link-path.repro.test.tsx` |

## Dropped

- fable: a second `research_open` in one turn replaces the first. Reproduces only by calling the executor directly; the broker's per-turn budget refuses the second call (`broker/tests.rs:622`). Hardened anyway (executor refuses when a hand-off is already staged).
- fable: detector bounds and hand-off delivery race probed with no failure.
- opus: unknown/foreign handles, `said` in unattended runs, credentials, Memory-off and stopped turns all held.
- codex: schema v5 refusal held.
