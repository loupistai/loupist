# Tests

Run the narrowest behavior test. `scripts/verify.sh` is the handoff gate, not the edit loop. Search by the observable behavior or registered command.

## Tests

Follow the [testing policy](../../AGENTS.md#tests). Select the smallest affected behavior
case; `scripts/verify.sh` is the handoff gate, not the edit loop. Run broad Rust
checks before a watched native session, since test-created files can trigger rebuilds.

```sh
# Renderer, from apps/loupist:
npm test -- <test-file>
# Rust, from repository root:
cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml <test-name> --locked
# Pi extension/tool contracts:
node --test apps/loupist/agent/*.test.mjs
# Private workers:
uv run --project apps/loupist/openbb --locked pytest apps/loupist/openbb/tests
```

### Find a test

Look up the behavior or command in the implementation and its adjacent tests.
For example, from the repository root:

```sh
rg -l 'number_checks|NumberChip' apps/loupist/src apps/loupist/src-tauri/src
rg --files apps/loupist/src | rg '(NumberChip|chat-evidence).*test'
npm --prefix apps/loupist test -- src/components/chat/NumberChip.test.tsx
```

Use [Which tool](../../apps/loupist/e2e/README.md#which-tool) for window behavior.
The [eval guide](../../apps/loupist/evals/README.md) owns credential-free
Pi contracts and separately authorized live calls.

### World extraction fixtures

World extraction golden tests replay frozen prompt and response-schema fixtures
in `src-tauri/src/knowledge/world_extract/golden/stages.json`. Intentional prompt
changes require reviewing and updating those fixtures with the Rust tests.
The in-process job tests exercise extraction, atomic publication/indexing,
uncertain-call quarantine, local recovery and hybrid search with deterministic
model replies and real local embeddings.

### Bridge contracts

`apps/loupist/src/bridge/tauriBridge.test.ts` checks that the bridge sends only
commands registered in `generate_handler!` in `apps/loupist/src-tauri/src/lib.rs`
and that every registered command has a bridge caller. It runs with the renderer
tests in `scripts/verify.sh`; add the bridge method in the same change as a new
command. `apps/loupist/src/contracts/assetRequests.example.json` lists the asset
requests the renderer sends; `apps/loupist/src/components/assets/AssetPage.test.tsx`
and the Rust `commands::assets` test both check it, since those Rust requests deny
unknown fields. `apps/loupist/src/contracts/financeRequests.example.json` does the same
for the earnings, watch and follow requests, checked by
`apps/loupist/src/components/briefings/Watching.test.tsx` and the Rust `commands::watch`
test.

## Code

- Evaluations: `src-tauri/src/eval_harness/`, [eval guide](../../apps/loupist/evals/README.md), [World evals](../../apps/loupist/evals/world/README.md)
