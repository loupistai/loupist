<div align="center">

<img src="assets/loupist-icon.png" alt="Loupist" width="96" />

# Loupist Desktop

</div>

This directory contains the Loupist React and Tauri desktop product, its agent resources, read-only market-data worker, in-process World extraction and search, bounded workspace file tools and offline Python, native E2E harness, app-local locks, and release packaging.

## Development

For Linux cloud development, source tests and browser previews, follow
[Linux cloud development](../../docs/dev/checks.md#linux-cloud-development).

```sh
sh scripts/setup.sh
# On the supported Mac, stage the bundled runtimes before the first native build:
sh scripts/stage-macos-arm64.sh
npm run tauri dev
```

Development builds keep their app state in `~/.loupist/development/`; installed
builds use `~/.loupist/`. They do not open the installed app's database,
credentials, or process state. An unsupported database is refused and left
unchanged. Debug `tauri dev`
also binds loopback WebDriver so `e2e/up.sh` and `agent-driver.mjs` can attach
to the same window you are clicking. If Vite reports that port 1420 is already
in use, attach with `e2e/up.sh` or stop the older development process before
starting again.

Global defaults live in `config.toml` under the selected app root. Each Loupist can
override Chat defaults, optional Skills and Pi settings in its workspace
`.loupist/config.toml`. Settings → Chat defaults and Skills edit shared defaults;
the selected Loupist’s settings edit its overrides. Each Skill
inherits independently, and resetting a local value restores the global default.
Chat Memory defaults apply to new Chats. Canonical Chat and Memory remain in the
Loupist folder.

```toml
# Global or Loupist-local config.toml
[skills.valuation]
enabled = false

[chat]
use_memory = true
update_memory = true
```

Map (the internal World model), Lens, Journal and visualization are always available app workflows. Map building
settings, connection grants, scheduled tasks and execution state remain in
SQLite; credentials stay in the private credential stores.

Settings → Skills lists included and user-added workflows, with search and
shared enable/disable controls; Loupist settings manage local inheritance. Advanced holds Add skill, Create skill and
Skill Creator, which starts off. Skill Installer is not bundled; Settings imports Skills without a model.
Folder, single-skill ZIP and public GitHub folder imports preserve supporting files.
Personal skills use `<app-root>/skills/<id>/SKILL.md`; Loupist skills use
`<workspace>/.loupist/skills/<id>/SKILL.md`. Changes load on the next Chat turn.
See [Skills management](../../docs/capabilities/skills.md#skills-management) for ownership and limits.

Isolated suites (`test:native`, `test:persistence`, live native E2E) use their
own Vite port and a no-watch E2E profile, but share the one-window GUI lease with
`tauri dev`. Stop the shared development window before those runners; after they
finish, start `tauri dev` again only when you want the window back. See
[`e2e/README.md`](e2e/README.md).

Run app-local checks without reinstalling dependencies:

```sh
npm run check
```

`npm run verify` is the clean-checkout gate and installs every locked app
dependency before running the same checks.

The first release supports macOS 13 or newer on Apple Silicon only.
Windows is not supported in this release.
Release builds require staged sidecars and the credentials selected by
`release-profile.json` (self-signed Beta, Developer ID, or ad-hoc fallback); the
[release workflow](../../.github/workflows/release.yml) defines the required
configuration. Use the [release acceptance flow](../../LOUPIST.md) to verify
the app against its recorded signing mode. The Beta is not notarized; see
[installation](../../installation.md) for first-launch approval and verification.
