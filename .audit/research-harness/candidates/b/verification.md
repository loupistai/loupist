# Candidate B verification

These are proposed checks, not checks already run. This design pass changed no product code.

## Observable source checks

Drive the current broker and completed-turn transaction in the existing Rust tests.

- A completion cites acknowledged Source and Lens receipts, commits with the answer, and reads back with the exact original bytes and method revision. Unknown, other-turn, other-profile, search-only, and truncated-only references fail.
- A later working-note update deletes the old working note as today but leaves its independent completion and frozen inputs readable.
- A duplicate staging call and a recovered ambiguous commit produce one completion. A changed uncommitted completion replaces its draft without publishing an earlier version.
- Stopping after `source_save` and `note_save` but before completion commits neither report nor note nor new Source. A positive successful completion case proves the test can see each write. Native test file effects still exist after cancellation.
- Memory off and read-only cases refuse writes. A prior staged completion followed by a custom result with Don't save produces no report, Source, Lens change, or learning admission. Cross-Loupist IDs refuse.
- A dated read hides later reports, methods, evidence, and World extension details. A dated report does not trigger this release's autonomous learning. Current principles retain the documented exception.
- Committing one eligible report queues one review without requiring five new World claims. Its report and original-source basis appear in the admitted review input. A model's unprovided evidence key refuses publication. Two claims from one copied input do not satisfy the report route's two-originals gate.
- A due report waits for World drain and the existing cooldown, survives restart as pending, and is consumed once on settlement. An uncertain paid review is not automatically retried. Disabling learning before finish publishes nothing.
- A new candidate method appears in the later turn's discovery catalog, exact-read returns its current full document, and set-aside, pause, archive, and ownership rules apply. The method stays not evaluated after repeated deliveries or `applied` markings.
- Schema change refreshes warm context. A partial summary sends exact-read guidance. The complete read matches the extraction owner's type names and signatures. The test checks actual returned values rather than a second hand-written schema list.

Method publication tests should use the existing inference seam with a supplied structured reply. They prove the host contract, not semantic usefulness. Test Behavior, Not Implementation changes these tests from prompt substring assertions into read-back and refusal checks.

## Product loop demonstration

Use a deterministic native fixture with two external originals and a preseeded method that misses a cash-conversion condition. Ask an ordinary research question, inspect the final answer and committed report, settle a candidate review, and ask a second independent question. The second turn must discover and exact-read the changed method. Its answer must apply the condition to new evidence, not repeat the training company's conclusion.

Show the same question with no reusable lesson. It produces useful research without a method publication. Show one unsupported opportunity and one evidence-backed candidate so the evaluator can distinguish declining a weak claim from refusing all candidates. The result must retain the strongest contradictory source.

Observe the receipt and Lens reader in macOS WKWebView at full width and narrow split width. Check keyboard focus, English and Korean strings, reduced motion, overflow, Source return navigation, and any form legends through `assertFormLegendsFit`. A Chrome preview alone does not establish native layout.

## Learning and opportunity evaluation

Reuse the existing `evals/lens` comparison. Keep fresh profiles for A Memory off, B training Sources only, C those Sources plus a curated method, and D those Sources plus an unedited method created by the production conversation and review path. Freeze the new research reports in training evidence but do not import them into held-out profiles. D minus B tests autonomous method transfer; a saved method or a better answer in training does not.

Add development scenarios for a helpful correction, a misleading correction, an asset-domain mismatch, contradictory sources, and a false relation chain. Freeze their original evidence, questions, model, reasoning, and execution settings before comparing versions. Repeat from independent training conversations and use the existing blinded quality rubric. Report failures, refusal rates, inappropriate transfer, tool use, tokens, and incomplete outcomes. Do not tune on the held-out Lens suite.

An opportunity benchmark first tests factual research quality. Can the agent identify the exposure, distinguish issuer from instrument, preserve dates and attribution, explain a plausible mechanism, find counterevidence, and give a falsifier? "Non-obvious" requires a fixed comparator such as a simple sector screen and blind relevance judgments. A novel sounding answer is not sufficient.

Any future alpha test must separately freeze candidate selection rules, the availability cutoff, universe including delisted names, the instrument mapping, holding horizon, benchmark, and transaction-cost assumptions. Keep out-of-sample outcomes and multiple-testing control separate from method-learning development. Product UI must not claim alpha while this work remains unrun.

Compare the one-child alternative only after the serial research path exists. Hold total acquisition and inference budgets constant, randomize condition order, and record child failures and extra approval friction. Admit it only if the quality gain justifies its cost and authority complexity. The test is whether independent work changes the supported answer, not whether two transcripts contain more facts.

## Handoff gate

Run the narrow Rust, agent contract, and UI checks relevant to the chosen implementation. Keep source checks working on macOS and Linux. Run `scripts/verify.sh` before handoff and report actual commands and results. Native Keychain, WKWebView, OS input, and release acceptance remain macOS responsibilities. No source or browser-preview success substitutes for those observations.
