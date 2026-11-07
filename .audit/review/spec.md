# Review task: Lens eval and method-learning branch (read-only)

Repository: /Users/junhoyoon/codex_pjt/loupist, branch `claude/lens-eval`, commit bafcb76.
Review the diff `git diff 3914a31..bafcb76` (`git log --oneline 3914a31..bafcb76` lists the commits).
Do NOT edit any repository file, commit, push, or run paid model calls. Write your report only to
`.audit/review/<your-agent-name>.md` (codex.md or grok.md) in this repository.

## The user's goal

Loupist is a local-first financial research desktop app. Its "Lens" is a per-Loupist wiki of
pages (subject, idea, method, perspective, principle) that the agent files through `lens_file`
during research turns, and that background "method learning" distills into reusable methods.
The goal: most users feel Lens pages get created and updated well, without over-fitting:
(1) product: a method or page generalized from one or two narrow research turns, or a stale page
that biases later research; (2) development: tuning to our own eval (Goodhart). Read
docs/knowledge/lens.md and apps/loupist/evals/README.md (sections "Lens lifecycle" and
"Method learning from research") first.

## What the branch does

- Eval: the headless runner records each turn's canonical Lens pages; new grader assertions
  `lens_pages`, `lens_unchanged`, `lens_read`, `lens_not_read`; suites
  `evals/agent/suites/lens-lifecycle-v1.json` and `evals/agent/heldout/lens-lifecycle-heldout-v1.json`;
  a `codex` judge backend; `lens-pages` review renderer; a rewritten `save_precision` judge criterion.
- Product: method learning starts from turn-filed Lens sections citing preserved original Source
  spans across >=2 turns, >=2 Chats, >=2 Sources (knowledge/learning.rs `evidence`, `recurs`),
  instead of 5 World claims; turn-created methods restricted to user teaching (SYSTEM.md, lens Skill,
  tool catalog); citation refusals list this turn's citation URLs (tool_executor/evidence.rs).
- Live eval: src-tauri/src/eval_harness/research.rs (two-Chat arm and one-Chat control).

## What to report

Findings ranked most severe first. For each: file:line, the concrete failure scenario (inputs or
state -> wrong result), and a suggested fix. Cover:
1. Correctness bugs in learning.rs (watermark `through_event` as lens-log rowid, settle -> enqueue
   recursion, the gate at enqueue/claim/publish, span eligibility join, rate limits, rollback).
2. Ways the eval can pass while the product is bad, or fail while it is good (Goodhart, grader
   leniency, regex assertions, judge criterion), and whether the held-out set is independent.
3. Over-generalization paths still open (e.g. evidence from the same company in two Chats,
   a method page filed by a turn, refine of a method from new evidence).
4. Anything in the prompt changes that could hurt users (agents refusing to save legitimate methods).
Say "no finding" for an area you checked and found clean. Do not pad. End with a one-line verdict.
