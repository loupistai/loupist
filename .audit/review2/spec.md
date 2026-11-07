# Review task: Lens follow-ups branch (read-only)

Repository /Users/junhoyoon/codex_pjt/loupist, branch `claude/lens-followups`, commit 2bd6786, based on main 43ac347 (PR #48).
Review `git diff 43ac347..2bd6786` (`git log --oneline 43ac347..2bd6786`). Do NOT edit repository files, commit, push or make paid model calls.
Write your report only to `.audit/review2/<codex|grok>.md`, then send worker_done.

Goal: most Loupist users should feel Lens pages and methods are created and updated well, without over-generalizing or erasing safeguards. Earlier reviews and an audit are in .audit/review/ (codex.md, grok.md, codex-audit.md); this branch closes their open items:
- ripgrep 15.2.0 staged beside fd (scripts/stage_workspace_tools.py, check-sidecars.py, stage-pi-windows-x64.ps1).
- citation_url schema patterns anchored (agent/tool-catalog.json) after finding xAI enforces `pattern` as a whole-value match.
- knowledge/learning.rs: case identity from subject anchors; a started review consumes only shown rows; a refine only for methods shown in full; prompt agent/prompts/method-learning.md.
- commands/tool_executor/lens_file.rs: a turn's put on a method page needs a `said` basis.
- evals/agent/heldout/lens-behavior-heldout-v1.json: a new behavioral holdout (check its rubrics are answerable from what each probe provides, and that no scenario leaks its grader expectation into the prompt).

Report findings ranked most severe first: file:line, concrete failure scenario, suggested fix. Cover correctness of the learning watermark/shown-rows logic (crash, rollback, concurrent filings, legacy rows), whether case identity can still be gamed or wrongly merge different companies, whether the said rule blocks legitimate user flows (user teaches a method across two turns; user edits a method; unattended runs), packaging/staging risks for rg on macOS and Windows, and holdout validity. Say "no finding" for areas you checked clean. End with a one-line verdict.
