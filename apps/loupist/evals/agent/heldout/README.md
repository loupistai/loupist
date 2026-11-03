# Held-out agent scenarios

These scenarios are for final adoption checks only.
Never show them to a prompt or harness optimizer.
Never use them for Lens authoring or training.
Run them through `agent/cli.mjs` only with `--allow-heldout`.
Every scenario uses `split: "heldout"`; `--split heldout` selects that split.
Replay fixtures remain in `agent/suites/replay/` and the CLI resolves them for runs.
