# Lens follow-ups review

Scope: `git diff 43ac347..2bd6786` at `2bd6786`. Read-only. Upstream ripgrep 15.2.0 archives were hashed and listed. `python3 apps/loupist/scripts/test_stage_workspace_tools.py` passed, 9 tests. Rust tests were not run. No paid model calls.

## 1. An unanchored page still counts as a second company

`apps/loupist/src-tauri/src/knowledge/learning.rs:482-484`, with the gate at `315-316`.

Chat A files a span-cited section on a subject page anchored to one company. Chat B files the same check on an idea page with no `about` anchors, citing a different source. The idea page's case is `page:{record_id}`, which shares nothing with the subject's node or folded name. Enqueue, claim, and publish all accept the pair. `learning/tests.rs:1258-1263` asserts that this queues a review. The lens skill does not require `about` on an idea page. Two unanchored idea pages about one company do the same.

A second split is two spellings of one company. A resolved anchor stores both `node:{id}` and `name:{fold}`, and a pending anchor stores only its own fold (`learning.rs:476-480`). "Acme" resolved to a World node and "Acme Inc." still pending (fold `acme inc.`) share neither key, so they are two cases. The comment at `372-373` says either spelling matches. They match when the folds are equal, or when both rows already carry the same node. `resolve_anchors` (`lens.rs:1104-1108`) writes `node_id` only onto the row whose fold is an exact World hit.

Pages that share any anchor stay one case, including two different companies given the same folded name. That blocks a method.

Fix: count a case only from resolved World node ids. A page with no resolved node is not a second case. Remove the `page:{id}` fallback.

## 2. The said check accepts a flag, and summary and retire skip it

`apps/loupist/src-tauri/src/commands/tool_executor/lens_file.rs:263-272`. Each method `Put` must contain some `BasisRef::UserSaid`. Resolving `{"said": true}` (`629-647`) checks that this turn has a user message. It does not compare the section markdown with that message.

The user says "what about margins?" The agent creates a method from its own analysis and attaches `{"said": true}`. Staging succeeds. The basis stores this turn's message id.

These flows still work. A later turn whose user text is only "Keep it as a method" can cite `said` for a procedure stated earlier. HV-006 is that flow. A same-turn correction that includes `said` works. An edit made in the app does not go through `lens_file`. An unattended `put` is refused, because `said` is unavailable when `handles.unattended` is set.

`docs/agent/turn-tools.md:58-60` says an unattended run writes no method. `Summary` and `Retire` are outside the `Put` check. An unattended run that has exact-read the method can retire `limits` or replace the summary. The safeguard is removed.

A failed kind lookup fails open. `is_ok_and` at `258` treats an error from `lens_kind` as "not a method" and skips the check.

`agent/skills/lens/SKILL.md:42-48` also tells the agent to cite `{source}` for a method grounded in a newly saved file. That `put` is refused until the basis includes `said` as well.

Fix: require `said` on method `summary` and `retire`. Keep describing the flag as a citation of this turn's message, and require the section text to come from that message if that is the rule. Tell the skill to send `said` and `source` together when a file grounds the method.

## 3. A refine can still replace a method the user taught in chat

`learning.rs:714-728` writes the model's four sections and always writes a new summary. `981-998` skips keys in `user_written`, which is only `Origin::User` (`614-619`). A method saved from chat uses `FilingOrigin::Turn`, so a limit the user dictated is not in that set.

The user says "never use this during a commodity-price shock" and the agent stores that sentence on `limits`. Two later chats about other companies pass the gate. The review is shown the full method, because `current` is set. The model returns limitations that mention only the new companies. Publish replaces `limits`. The shock rule is gone. `a_refine_sees_the_whole_method_and_never_replaces_text_it_was_not_shown` checks that a method which did not fit cannot be targeted, and that a model which copies the limit keeps it. A model that drops a limit it was shown still publishes.

A method that does not fit in the 8,000-character method budget is listed with `current: None` (`622-624`) and cannot be refined. The model still sees the 300-character `applicability` and can `create` a second method that never received the hidden limits.

Fix: treat a turn section whose basis includes `said` as user-written. Reject a refine whose new `limits` or `checks` drops a sentence from the shown section.

## 4. HV-002 fails a page that records the withdrawn schedule

`apps/loupist/evals/agent/heldout/lens-behavior-heldout-v1.json:183` and `apps/loupist/evals/agent/grade.mjs:365-369`.

The user asks to delete the phase-2 date and keep the rest. The agent removes it as a current fact and writes "2026년 4분기 일정은 오보로 삭제했다". `section_not_matching` is `4분기|Q4|fourth quarter`. The grader fails when any summary or section matches. The probe rubric at `207-208` tells the judge to fail an answer that states the fourth-quarter date, including one that quotes it as the withdrawn misreport.

Fix: fail the schedule only when it is stated as current. Allow it in a sentence that marks it withdrawn or a misreport. Use that same distinction in the probe criterion.

## 5. HV-005's probe requires a date the train grade never stored

`lens-behavior-heldout-v1.json:420-438` and `449-465`.

The train attachment contains "2026년 3월 10일" and the 1,300원 planning assumption. The train assertions require a subject page with a span and no method. They do not require the date or the figure on the page. The probe is another session. `turn_prompt` (`eval_harness/mod.rs:593-621`) sends the user question, which contains neither 1,300원 nor March 10. The rubric requires both. A train turn can pass its deterministic grade and leave the probe with nowhere to read the day.

Fix: require the train page to contain the dated assumption. Keep the probe rubric.

## 6. HV-006's read check depends on a title the create check accepts any string for

`lens-behavior-heldout-v1.json:519-565`.

The user asks to keep the comparison. The create assertion accepts any method title (`title_matches: "."`). The probe's `lens_read` passes only when the title matches `working capital|distributor|cash conversion|payable|days`. A method titled "Brenmark and Tollan quarter check" fails retrieval. The apply rubric can pass from the probe question alone, which gives inventory days 50 to 68 and payable days 60 to 45.

Fix: name the title the probe will read, or match `kind: method` with `min: 1`. Grade the explanation against the saved method's steps.

## Checked, no finding

Watermark and shown rows. `Consumed` is every lens-log row through a started review's `through_event`, plus the `shown` rowids above it (`learning.rs:133-176`, `631-652`, `784-786`). Each started review copies the previous `shown` set forward. New evidence rowids sit above the previous log head, so a later cutoff stays at or above the previous watermark. Recovery marks a running review failed and leaves that watermark (`816-829`): rows above it are eligible again, and rows the attempt showed are not retried. Publication rolls back inside `learning_publication` (`894-913`) without moving the watermark. A second review cannot start while one is pending, queued, or running (`113-118`). A legacy started review stores the old `Input` object in `input_json`; parsing that as `Consumed` yields an empty `shown` set and consumes the whole `through_event`, which is the old full-window rule. Rows with a non-empty `world_job_id` stay out of the watermark. A defer before inference leaves `started_at_ms` null, so the evidence stays eligible; `seen` waits for another Lens log row before requeue (`122-124`).

A full window sets `through` to the log head (`632`). A put that was ineligible at that moment (paused source, contested section) is under that head and absent from `shown`. Unpausing it creates no new log row, so that put is not offered again. That matches the previous started-review rule. A new filing of the section is a new row and is eligible.

Anchored case identity. The same node, the same folded name, a subject plus an idea that shares the anchor, and a chat whose sources are a subset of another's do not queue. Method pages are not evidence.

Said on the flows that should succeed. A later "keep this" turn can cite `said`. A same-turn edit can too. Unattended method puts are refused. Learning publication does not pass through this check.

Packaging. The pins at `stage_workspace_tools.py:75-82` match the GitHub 15.2.0 archives (`3750b2e9…` for `aarch64-apple-darwin`, `71b2fef8…` for `x86_64-pc-windows-msvc`). Both archives contain the executable, `LICENSE-MIT`, and `UNLICENSE` under `ripgrep-15.2.0-<target>/`. Each is about 1.8MB, under the 16MB read cap. `stage-pi-macos-arm64.sh:111-128` signs every Mach-O in the runtime, then `--record-signature` stores the signed digest. `stage-pi-windows-x64.ps1:65-68` signs `rg.exe` and then records the digest. The staging unit tests passed.

Citation patterns. The host form is `#evidence-` plus `ev_` plus 32 lowercase hex digits (`commands/tool_executor/evidence.rs:10-16`). The three catalog patterns are `^#evidence-ev_[0-9a-f]{32}$`. The extension test requires every catalog `pattern` to be anchored at both ends.

Holdout leakage. `turn_prompt` sends the user text. `expect`, rubrics, and scenario titles stay out of the prompt. HV-001's user correction states 2.8 and 2.3 because that is the correction. HV-004 puts both restart claims in the attachments, and the probe question does not repeat them.

HV-003 is answerable from the attachment (2.40, 1.10, and 3.05) and the seeded limit, which says to use distributable cash flow for a master limited partnership. The user question does not state that conclusion. A model that already knows MLP accounting can meet the rubric without reading the seed. The page assertion still requires the limit text to remain.

Verdict: Request changes. Anchored same-company pages, ripgrep staging, citation patterns, and the shown-row watermark hold, while an unanchored page, a said flag that skips summary and retire, and a refine that can drop a chat-taught limit remain open.
