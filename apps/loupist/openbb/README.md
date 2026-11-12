# Loupist OpenBB runtime

This project freezes OpenBB Platform and the official OpenBB MCP server into a
private, read-only sidecar. It is not a public CLI. Rust owns process creation,
credentials, capability checks, result limits, and shutdown.

The frozen OpenBB distribution and its pinned provider packages are trusted
product code. Exact-host checks cover the audited Python HTTP stacks and catch
undeclared provider egress, but they are not an OS network sandbox against a
compromised native dependency. HOME/cache redirection, private bootstrap,
process-tree cleanup, MCP filtering, and result bounds remain enforced at the
host boundary.

## Wire contract

The process reads one newline-terminated Loupist bootstrap frame before the MCP
client sends `initialize`:

```json
{"type":"loupist.bootstrap","protocol_version":1,"run_id":"run-1","scratch_dir":"/private/run/directory","credentials":{},"settings":{"allowed_categories":["equity"],"enabled_provider_ids":["yfinance"],"allowed_network_hosts":["consent.yahoo.com","fc.yahoo.com","finance.yahoo.com","guce.yahoo.com","markets.businessinsider.com","query1.finance.yahoo.com","query2.finance.yahoo.com"],"provider_config":{}}}
```

The frame is at most 65,536 bytes. `scratch_dir` must already exist, be an
absolute non-symlink directory, and be private to the current user. Credential
keys are OpenBB credential names authorized by the enabled provider entries in
`runtime-manifest.json`. After the newline, stdin and stdout speak the official
MCP stdio protocol.

DNS, connection and timeout exceptions are classified before OpenBB's REST
handlers mask them. The MCP error result carries only the fixed structured
marker `{"loupist_error":{"kind":"offline"}}`; Rust converts it to the same
`offline` broker failure as native data reads, without forwarding exception
text, URLs or credentials. HTTP status errors, invalid arguments and rejected
network policy remain distinct from offline reads.

The wrapper points OpenBB's home, settings, caches, temporary files, Matplotlib,
Numba, and Python bytecode at the run scratch directory before importing any
OpenBB module. Credentials are installed into OpenBB's process-local user
settings service without writing `user_settings.json`; they are never accepted
in command-line arguments or environment variables. Provider configuration is
strictly mapped by the manifest. In particular, the SEC contact email replaces
all bundled SEC request identities, and Tradier's account type is limited to
`sandbox` or `live`.

Provider authority is also enforced for Tools that do not expose a top-level
`provider` input. The runtime manifest explicitly classifies the audited local
analytics Tools and maps direct provider Tools to their one implicit provider.
Unknown providerless routes are omitted. For example, the `imf_utils` surface is
not discoverable unless the Loupist has enabled the `imf` provider.

The source manifest names the executable `loupist-openbb`. Windows is not
supported in this release.

## Development

```sh
uv sync --locked --python 3.12
uv run --locked pytest
uv run --locked ruff check .
uv run --locked python build_sidecar.py --check
```

`build_sidecar.py` runs `openbb-build` and creates a PyInstaller one-directory
bundle named `loupist-openbb`. Platform staging scripts live in the parent
`scripts/` directory.

The sidecar depends on base `openbb` plus only the extensions
`runtime-manifest.json` can reach: the package of every listed provider and the
`econometrics`, `quantitative` and `technical` routers named in
`allowed_categories`. It does not use `openbb[all]`, which adds
`openbb-charting` and Plotly; no Loupist Tool asks OpenBB for a chart.
`openbb-platform-api` stays because base `openbb` requires it. Adding a provider
or category means pinning its package in `pyproject.toml`, running `uv lock`,
and updating the manifest (including `uv_lock_sha256`). `tests/test_manifest.py`
fails when installed provider packages and the manifest differ, and
`tests/test_server_policy.py` starts the real server and fails unless every
allowed category and every provider is reachable through a Tool.

The bundle also contains `THIRD_PARTY_LICENSES/python-distributions.json` and
its referenced compliance archive. The builder resolves the locked runtime
dependency closure, copies every distribution's full metadata plus
package-level license/NOTICE files, adds the CPython and PyInstaller license
payloads, and verifies every archived byte by SHA-256.
