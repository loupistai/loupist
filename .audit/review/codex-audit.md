# Audit of the Lens evaluation record

Reviewed `claude/lens-eval` at `20a5664`, using the last 25 commits after the rebase, `.audit/lens-eval/evaluation.md`, `.audit/lens-eval/decisions.tsv`, the recorded outputs, and the current implementation. Only this report was written. No paid calls, builds, native interaction, commits, or test-suite reruns.

The four published results tables are arithmetically correct. Several explanations and the claim that all review issues were fixed need correction. Earlier items 1, 3, 7 and 8 remain at least partly open; the most serious remaining path admits contested evidence into learning.

## Unaddressed correctness findings

1. **P1 — Contested evidence still passes the new current-evidence check (earlier item 1).** `apps/loupist/src-tauri/src/knowledge/learning.rs:366` checks that the current section's digest equals the historical put's `after_digest`, but never tests `SectionState`. `apps/loupist/src-tauri/src/knowledge/lens_page.rs:941` explicitly excludes state from that digest. Claim invalidation changes only the section state (`lens_page.rs:370`, persisted by `lens.rs:1045`). Consequently, a turn section citing both a retained span and a World claim remains eligible after that claim falls and the section becomes contested. The span survives the filter at `learning.rs:376`; the claim and contested status are not included in the observation. Publication reuses this same insufficient filter (`learning.rs:835`). With a second eligible Chat/page, the disputed interpretation can still become an uncontested learned method. The new tests cover undo, forget and Source removal, not this state transition (`knowledge/learning/tests.rs:893`). Reject contested sections at admission and publication, and cover contestation before claim and during inference. This is a source-level finding, not a newly executed Rust reproduction.

2. **P2 — Observations omitted from an actual inference are still consumed (earlier item 3).** The diversity-first input and non-consuming pre-inference deferral are real fixes. However, `learning.rs:526` still stops when the 12,000-character input fills, while `claim` retains the full-window `end` (`learning.rs:674`) and `watermarks` advances to that review's entire `through_event` once `started_at_ms` exists (`learning.rs:134`). Subsequent `evidence` reads only rows above that boundary (`learning.rs:319`). Thus rows omitted from a successfully started review are never considered later. The new crowding test explicitly establishes that fewer than 26 observations fit, then stops before checking their later eligibility (`learning/tests.rs:959`). Keep an unprocessed set/cursor for omitted observations, or explicitly record and accept this lossy sampling policy. The claim that truncation never shows “less breadth than the evidence has” in `docs/knowledge/lens.md#method-learning` is also too strong: the core preserves a gate-passing pair, not every case or counterexample in the window.

3. **P2 — Different pages are not necessarily independent companies/cases (earlier item 7).** `learning.rs:258` requires two page IDs and independent Chat Source sets; it does not compare the pages' subject identity. A subject page and an idea page about the same company's same ramp, filed in different Chats against different reports, pass. These pages can coexist: `lens.rs:310` deduplicates by subject anchor only for `PageKind::Subject`, while an idea with a different title remains separate. `Observation` has no subject-identity field (`learning.rs:216`), and subject anchors are read only after publication (`learning.rs:947`). The new “one_company” test puts both filings on the same subject page (`learning/tests.rs:1022`), so it does not test this counterexample. Method pages and nested Source sets are now excluded, which addresses those subcases. Case/scope validation and the same-company subject-plus-idea control remain open. Turn-created or substantially widened methods also remain prompt-governed: `commands/tool_executor/lens_file.rs:201` does not verify an explicit user adoption request. The ordinary-turn fixtures do not establish a hard enforcement boundary for refinement.

4. **P2 — Existing non-user-origin safeguards remain invisible to a refine (earlier item 8).** `learning.rs:483` supplies only the first 300 applicability characters; it does not send existing steps, checks or limits. Digests and user-written keys are skipped in serialization (`learning.rs:194`). `Output::ops` can still replace all four non-user-origin sections (`learning.rs:601`). The added test preserves `Origin::User` sections (`learning/tests.rs:1133`); it does not protect limits written by Loupist or prior learning, including safeguards the user adopted through Chat. This is correctly acknowledged in `evaluation.md:105` and `decisions.tsv:26`, but cannot also be counted as fully fixed. Supply existing refine text and preserve unrelated safeguards, or prohibit a refine when that text cannot be shown.

These residuals contradict an unqualified reading of “all eight were fixed” (`evaluation.md:97`) and “fixes for all 8 review items” (`decisions.tsv:29`). Commit `0017ca4` contains useful targeted fixes and tests; it does not close every earlier finding. The claim that those tests were mutation-checked is historical evidence in the commit message, not something independently rerun by this audit.

## Incorrect or overstated empirical claims

5. **P2 — The claimed 2/6 context-collapse count combines different failure modes.** `evaluation.md:50` and `decisions.tsv:31` attribute two candidate failures to lost July segment mix/mills. In `apps/loupist/evals/output/lens-candidate-v1/judge2/judgments.jsonl:90`, LL-005 trial 2 explicitly **retains** the mix, debt and mill footprint; it fails because the facts/risks still show the old dividend after `skipped_stale` operations. The actual retention loss is final-candidate LL-005 trial 1 (`lens-candidate-final/judge2/judgments.jsonl:83`, also `grade/assertions.jsonl:103`). Report one retention loss in the final three trials and one different stale-update failure in the earlier three. Fisher's two-sided p=0.5 is arithmetically correct for the table 2/6 versus 0/3, but that table measures pooled update failures, not two observed context collapses. It also pools different candidate versions. Non-significance in this sample does not establish absence of regression or show that other prompt changes could not affect update behavior.

6. **P2 — “Run once per arm” is not the held-out attempt history.** `evaluation.md:26,57` and `decisions.tsv:37` omit the earlier partially completed attempts that `decisions.tsv:32,35` themselves record. The retained `traces.jsonl` files show:

   | Attempt directory prefix | Baseline completed / failed turns | Candidate completed / failed turns |
   | --- | ---: | ---: |
   | `lens-heldout-{baseline,candidate}` | 5 / 2 | 5 / 2 |
   | `lens-heldout-{baseline,candidate}-r2` | 1 / 2 | 11 / 2 |
   | `lens-heldout-{baseline,candidate}-r3` | 30 / 0 | 30 / 1 |

   All failures here are recorded as `provider_error`. The two `*-final/traces.jsonl` files contain exactly the completed turn identities from `*-r3`; they are not additional runs. The candidate r3 failure is LH-003 trial 1, followed by a completed replacement. Thus the accurate description is one final complete dataset per arm, after two abandoned partial attempts and one additional failed candidate unit. This does not demonstrate cherry-picking, but prior completed exposure matters to the claimed once-only holdout control and should be disclosed.

7. **P3 — The baseline citation-refusal numerator is stale.** `evaluation.md:93` says 81/652. In `lens-baseline-v1-merged/traces.jsonl`, excluding the later LL-009 addition yields exactly 652 calls; **85** of those calls are `calculate`/`grid_publish` errors whose message contains `not a citation_url from this turn` (78 calculate, 7 grid_publish). The earlier `lens-baseline-v1/traces.jsonl` has 607 calls and 81 calculate/grid errors, but only 78 match that citation-refusal predicate; the remaining three are other malformed arguments. Thus `decisions.tsv:14` also labels a broader error count as citation refusals, and its numerator should not be carried into the later denominator. “Repeated identically” should also not be treated as a count of duplicate calls: the final development report's separately measured repeated-failure total is 35 baseline and 37 candidate, with some argument evidence unavailable. The main 91/676 and 88/664 invalid-call table entries are correct.

8. **P3 — The two other recorded research attempts did not both fail with xAI provider errors.** `evaluation.md:85` says they did. `lens-research-v1-run1-socket-path.json` records `failureKind: protocol_error` in both first training turns; `decisions.tsv:20` identifies the socket-path cause. `lens-research-v1-run3-final.json` records `provider_error` in both arms. Neither attempt has a completed training turn. The four retained run1–run4 JSON files support one socket/protocol failure and one provider-error failure, alongside the two completed samples.

9. **P3 — The rebase description is over-narrow.** `decisions.tsv:33` says main touched “only connection setup.” The last-25 log includes `c89d0ed` (#47, workspace tab row/tooltip) and `be57fec` (#46, panels/navigation/greeting), as well as `86f36d1` (#45, connections). That wording is false, although these UI changes do not by themselves invalidate a headless comparison. Pin the measured commits rather than describing the development candidate simply as “the final branch”: development traces record clean `b1e69f3`, held-out candidate traces record clean `33992fb`, and baseline traces record clean `929ef67`. Both candidate lifecycle datasets record the same agent snapshot hash `4ed6a3ba…`; the Rust learning fixes correspond to rebased `0017ca4`.

## Recomputed results and supported claims

I separately counted all 396 deterministic assertion rows and 371 judgment rows, grouped by metric, and compared them with each report's `metrics.<name>.checks`. All denominators and numerators agree; no row in those counts has an unknown pass value. Coverage is complete for 60 development turns per arm and 30 held-out turns per arm.

| Metric | Dev baseline | Dev candidate | Held-out baseline | Held-out candidate |
| --- | ---: | ---: | ---: | ---: |
| save_recall | 33/33 | 33/33 | 18/18 | 18/18 |
| save_precision | 141/143 | 144/145 | 71/71 | 72/72 |
| lens_update | 39/42 | 37/42 | 17/18 | 17/18 |
| carry_forward | 18/18 | 18/18 | 4/6 | 3/6 |
| negative_transfer | 6/6 | 6/6 | 3/3 | 3/3 |
| retrieval | 9/12 | 9/12 | 6/6 | 6/6 |
| Invalid tool calls | 91/676 | 88/664 | 50/322 | 39/299 |

Sources: `apps/loupist/evals/output/{lens-baseline-v1-merged,lens-candidate-final,lens-heldout-baseline-final,lens-heldout-candidate-final}/{grade/assertions.jsonl,judge2/judgments.jsonl,report/report.json}`. I also recomputed tool validity directly from each `traces.jsonl` using `tool-evidence.mjs`; every field matches the report.

The omitted development `efficiency` metric is 0/3 in both arms; `task_success` is 3/3 in both. Invalid-call rates remain 13.46%→13.25% in development and 15.53%→13.04% held out, above the report's 5% goal in every arm. The tables should not be described as an overall passing quality gate.

Other supported observations:

- LL-008's baseline creates an additional, empty-basis method in trials 0 and 2; candidate creates none in all three. LL-006 creates no method in either arm. LL-008's three retrieval failures per arm are reads of the seed method; the separate negative-transfer judgments pass.
- LL-003 has three `corrects-premise` failures per arm. All six correct the end date but decline the requested 2027 exposure sizing. The final candidate's remaining save-precision failure is an improperly categorized Corvane idea page (`lens-candidate-final/judge2/judgments.jsonl:71`), not another unsolicited method.
- LH-004 supplies no FFO, maturity or occupancy data, while its rubric demands a company-specific assessment. The missing-data explanation for those carry-forward failures is supported by the fixture and judgment reasons. A revised rubric needs a fresh evaluation; the existing 4/6 and 3/6 remain the observed scores. Both held-out update misses are LH-002 trial 2's refusal to assert that the remaining 14 ships are specifically container ships; they are not failures to update the fleet count.
- The initial baseline's retained `grade/assertions.jsonl` is 91/99, as the historical decision entry says. Its original judge file fails all 27 Lens commits while passing 27 Source and 25 note commits; the old subject-page criterion problem is real.
- The final verification evidence is split across two receipts for the same clean `33992fb` source tree: `output/checks/20261107T193536Z-2b573361/receipt.json` has passing Python/Rust and failed web Node-version setup; `20261107T194140Z-31512657/receipt.json` has a passing web rerun. Together they support `decisions.tsv:34`. `evals/output/verify-final.log` alone ends in failure and is not a standalone green gate receipt.

### Research sample reconciliation

I recomputed every `measures` field from `arms.*.reviews`, `arms.*.methods` and the probes in both named JSON files; all fields match. Both runs have one created two-Chat review, one learned method, two distinct Source IDs on each of its four learned sections, two completed training turns per arm, no one-Chat review, P1 exact-reading the method, P2 not reading it, and no probe Memory commits, Lens operations or new reviews. The quoted titles and limits are present.

| Observation | `lens-research-v1-run2.json` | `lens-research-v1-run4-final.json` |
| --- | --- | --- |
| Recorded code state | `730fd42`, dirty | `33992fb`, clean |
| Review status / method created | created / true | created / true |
| Minimum distinct Sources per section | 2 | 2 |
| P1 exact read / applied flag | true / **false** | true / true |
| P2 method read | false | false |
| Probes wrote nothing / one-Chat reviews | true / 0 | true / 0 |

Run2's P1 has null `lens_read.status`, so its applied flag is false. Nevertheless, its answer explicitly names the method and uses the upper-bound reasoning; the narrower narrative at `evaluation.md:78` is supported by the answer. Do not turn that narrative into a claim that both recorded applied flags are true. Run4's applied flag is recorded as true. Neither flag establishes an independent causal quality improvement: the one-Chat arm is a gate control and has no matched third-company probe without a learned method.

## Earlier review reconciliation

| Earlier `.audit/review/codex.md` item | Current assessment and evidence |
| --- | --- |
| 1: withdrawn/corrected/hidden evidence | Partly addressed by `0017ca4` current-digest resolution and publication re-read; contested-state case remains (finding 1 above). |
| 2: legacy watermark namespace | Addressed: `learning.rs:134` excludes nonempty `world_job_id`; recovery settles legacy pending/queued rows; targeted test at `learning/tests.rs:930`. |
| 3: truncation consumes unseen evidence | Partly addressed: core selection and pre-inference deferral fixed; omitted observations in a started review still consumed (finding 2). |
| 4: recovery strands post-watermark filings | Addressed: recovery re-enqueues affected Loupists at `learning.rs:705`; restart test at `learning/tests.rs:996`. |
| 5: judge cannot see canonical page | Addressed by `20e01f0`: `judge.mjs:152` sends stored sections, per-section basis/state and operations, and cache identity includes the input (`:185`). |
| 6: regex accepts wrong current values | Substantially addressed by stored-page rubrics (`20e01f0`) and anchored figures (`a2b3e30`). Regex alone is still weak; the judgment now supplies the semantic check. LL-008 retrieval reads are separately reported. |
| 7: case independence / turn bypass | Partly addressed: excludes method evidence and nested Source sets, requires two pages. Same-company multi-page cases and prompt-only turn refinements remain (finding 3). |
| 8: unseen mature safeguards on refine | Open for non-user-origin sections; user-origin protection is only a partial fix (finding 4). |
| 9: refuses explicitly requested methods | Prompt conflict addressed by `8711ce6`; `20e01f0` adds LL-009/LH-004 positive cases. Those cases teach a checklist explicitly; they do not independently test adopting a checklist the assistant previously developed across multiple companies. |
| Live-eval quality limitation | Documentation corrected by `a2b3e30`; two saved samples show actual creation/use. No matched no-method quality comparison, repeated final-code trials, or learned-method refinement evaluation. |
| Behavioral holdout independence | Still open and accurately acknowledged in `evaluation.md:108`; sector/template variation is not an independent behavioral holdout. Attempt history needs the correction above. |

The current outcomes support narrow fixture-level improvements and a descriptive successful learning example. They do not establish that most users will experience better pages, that two reports establish a broadly valid method, or that stale/narrow learning paths are closed. Retained raw outputs are ignored artifacts; a reviewer needs access to them to reproduce these numbers. Historical model-judgment and mutation-test assertions are not substitutes for a frozen behavioral holdout covering contestation, same-company multi-page research, omitted evidence, and mature-method refinement.

Verdict: request corrections to the evaluation narrative and keep the residual learning findings open before claiming that all review issues were fixed or that the branch establishes general quality gains.
