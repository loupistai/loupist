<div align="center">

<img src="assets/loupist-icon.png" alt="Loupist" width="96" />

# Loupist Desktop

</div>

This directory contains the Loupist React and Tauri desktop product, its agent resources, read-only market-data worker, in-process World extraction and search, native workspace execution, native E2E harness, app-local locks, and release packaging.

## Development

```sh
npm ci
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
`.loupist/config.toml`; Settings → Chat preferences and Skills edit the same files. Each Skill
inherits independently, and resetting a local value restores the global default.
Chat Memory defaults apply to new Chats. Canonical Chat and Memory remain in the
Loupist folder.

```toml
# Global or Loupist-local config.toml
[skills.valuation]
enabled = false

[chat]
use_memory = true
update_memory = false
```

World, Lens, Journal and Visualize are fixed app Skills. World processing
settings, connection grants, Scheduled tasks and execution state remain in
SQLite; credentials stay in the private credential stores.

Settings → Skills lists included and user-added workflows, with search,
enable/disable, scope and inheritance controls. Create a skill directly or use
Skill Creator to draft one in Chat; Skill Installer helps add requested workflows.
Folder, single-skill ZIP and public GitHub folder imports preserve supporting files.
Personal skills use `<app-root>/skills/<id>/SKILL.md`; Loupist skills use
`<workspace>/.loupist/skills/<id>/SKILL.md`. Changes load on the next Chat turn.
See [Skills management](../../docs/bundled-capabilities.md) for ownership and limits.

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
Windows runtime, staging, and signing implementations remain available for future
support. The manual [Windows compatibility workflow](../../.github/workflows/windows-compatibility.yml)
runs native and package checks independently of release CI. Its optional signed
build requires a matching version tag and Windows credentials, and uploads
workflow artifacts only; it does not publish a release or updater feed.
Release builds require staged sidecars and platform signing credentials; the
[release workflow](../../.github/workflows/release.yml) defines the required
configuration. Use the [release acceptance flow](../../LOUPIST.md) to verify
the signed app.
