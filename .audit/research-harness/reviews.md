# Independent reviews

## Design

The independent gpt-6.1-terra judge chose candidate A. The selected design and
rejected alternatives remain in this directory. The product keeps one Pi turn
and one Rust Memory writer, with bounded learning from immutable experiences.

## Comments and naming

The gpt-6.1-terra comment review deleted five internal comments (ten lines) in
five changed files. No public API contract, external constraint, suppression or
HOLD required restoration. Two naming findings were resolved in `9f97ea1`:
`freeze_pending_review` identifies frozen admission, and
`same_research_without_receipt_ids` describes the idempotency comparison.
The fixes were direct renames; no new architecture or constraint encoding was
needed. No open comment findings remain.

## Correctness and privacy

An independent gpt-6.1-terra review of `9f97ea1` found one P2. A Source removed or
Lens paused/removed after its acknowledged read could still enter a new research
experience at completed-turn commit. Later filtering before learning did not
prevent the new retention.

`1b1b4b6` reuses the existing Memory delivery check inside the same transaction
before saving the experience and note. The new transaction test failed before
the fix and passed after it for Source removal, Lens pause and Lens removal.
The complete learning suite passed 24 tests. Logs are
`/tmp/loupist-research-control-race-before.log` and
`/tmp/loupist-research-learning-final.log`.

A fresh gpt-6.1-terra reviewer passed the two fixes at `9cc7748`, including the
common completion instructions motivated by the failed live attempt. It found
no concrete correctness, privacy, Journal-authority or saving-gate issue. It did
not run tests or infer reliable model behavior from prompt wording. The live
outcome and its limits are recorded separately in `evaluation.md`.

## Final trail review

An independent gpt-6-luna reviewer inspected the repository decisions, evidence
pointers, both evaluation traces and the final host receipts. Its verdict was
PASS for the supported claims, with native verification still open. macOS and
Linux receipts refer to the same product commit and show unchanged source during
their respective runs. The reviewer found no unresolved evidence pointers.

The reviewer confirmed the failed first attempt, later autonomous candidate
formation, exact relevant method use, correct Source-only baseline and the
single malformed source citation. The calculated 40% evidence anchor is valid.
The review does not establish an answer-quality or investment-return benefit.

Attention flags are the outstanding native window handoff, the single-sample
evaluation limit and the documented Linux parser/linker workarounds. Schema 6's
refusal of earlier folders remains a material compatibility boundary. See
[verification.md](verification.md) for the exact checks and remaining work.
