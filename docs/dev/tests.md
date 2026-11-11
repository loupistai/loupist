# Tests

Run the narrowest behavior test. Search by the observable behavior or registered command.

## Tests

Follow the [testing policy](../../AGENTS.md#tests). Select the smallest affected behavior
case; `scripts/verify.sh --affected` is the handoff gate, not the edit loop. Run broad Rust
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

Rust types are the source of truth for the bridge. Every type a command takes or
returns, every stream event and every emitted event payload derives `ts_rs::TS`
under `cfg(test)` (ts-rs is a dev-dependency, so release builds are unchanged), and
`apps/loupist/src/contracts/generated/` holds their TypeScript, one module per Rust
module plus `bridge_commands.ts` (`BridgeCommands`: each command's `request` argument
and response). The `bridge_contract` test in `apps/loupist/src-tauri/src/bridge_contract.rs`
regenerates them and fails on any difference; regenerate after a wire change with

```sh
LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract
```

Never edit the generated files. A new wire type needs `#[cfg_attr(test, derive(ts_rs::TS))]`;
a field behind `skip_serializing_if` needs `#[cfg_attr(test, ts(optional))]` (or
`ts(optional = nullable)` when it is not an `Option`), and a type Rust only
deserializes uses `ts(optional_fields = nullable)`, since a missing `Option` is
accepted. ts-rs cannot see `serialize_with`, `from`/`into` or an untagged variant of
a tagged enum; give those a `ts(type = ...)`. Two Rust types may not share a
TypeScript name (add `ts(rename = ...)`). The `commands!` table there lists each
registered command's request and response (every command in `generate_handler!`), and
`command_signatures` only compiles while every row matches the command.

Hand contracts in `apps/loupist/src/contracts/` alias their generated type when the
shapes match (`export type X = Wire.X`). A hand type that narrows Rust (literal unions
for a Rust `String`) or adds renderer-only fields stays hand-written and is checked in
`apps/loupist/src/contracts/wire.test.ts`, which `tsc` checks with the app. Every hand
type there is checked: a field Rust always sends is required in TypeScript, as
`T | null` when Rust sends `null`; only a field behind `skip_serializing_if` is
optional. `Compatible` checks a narrowing field by field; `SameShape` checks the
fields and which are required when a nested type narrows further (a JSON value read
as a known shape); `Accepts` checks requests; `Covers` checks renderer models built
from a wire type; `Narrows` checks a discriminated narrowing of a Rust type serialized
by hand. The
bridge object is property-typed, so each request type in `src/contracts/` must be
assignable to the generated request (`apps/loupist/src/bridge/tauriBridge.ts`).

New wire types use `#[serde(rename_all = "camelCase")]`. Existing snake_case fields
stay: stored rows and `json_extract` paths read their serde names. Event names live
in `apps/loupist/src-tauri/src/events.rs` and `src/bridge/events.ts`;
`src/bridge/events.test.ts` checks that every `emit` names a registry entry and the
renderer subscribes to each one. `CommandError.code` is `CommandErrorCode`
(`src-tauri/src/app/error_code.rs`), generated as a union; read it with
`commandErrorCode` from `src/lib/errors.ts`, throw it from mocks with
`src/bridge/mock/commandError.ts`, and list a new code there before returning it
(`listed_command_error_codes` checks the literals). An unlisted string code still
crosses unchanged. The renderer ignores a stream event type it does not know (with a
development warning) instead of failing the reply.

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
