<div align="center">

<img src="apps/loupist/assets/loupist-icon.png" alt="Loupist" width="108" />

# Loupist

[![CI](https://github.com/loupistai/loupist/actions/workflows/ci.yml/badge.svg)](https://github.com/loupistai/loupist/actions/workflows/ci.yml)

**Watch. Remember. Explain.**

A Loupist is a research colleague for what you invest in: it watches the assets you follow, remembers how you think, and explains what changed with the evidence attached. It runs on your Mac and never trades.

[Install](installation.md) • [Development](apps/loupist/README.md) • [How it learns](#how-a-loupist-remembers) • [Security](#security-model)

</div>

---

A Loupist carries research across conversations. It reads primary filings, runs the valuation math itself, challenges its prior views, and turns each piece of research into knowledge it uses again — in later companies, later markets, and later reviews.

Each Loupist works in its own local folder, created by the app or opened from one you already have. Reports, code and data stay in that folder; `.loupist/` keeps its Chats and Memory with retained source bytes and immutable revisions. Close the app and copy the whole folder, including `.loupist/`, to preserve or move a Loupist. Lens methods remain Markdown. The app runs on your machine with a supported provider API key or account sign-in. There is no Loupist account or sync.

## Teach a principle, see it applied

**Day 1** — *"Check whether capacity costs are temporary before trusting margin recovery."* Save a principle and apply it to dated company reports. The Map connects retained Sources to entities, claims, principles and methods. With World processing and method learning enabled, repeated evidence can produce a reusable method; the review can also defer when the evidence is insufficient.

**Later** — *"Compare these companies’ margin recoveries."* In a new Chat, the Loupist can apply the saved principle and relevant learned method. Inspect a revised method’s dated Then / Now summaries and evidence in the Map.

Follow companies or import holdings as research context, receive dated briefings in the Thread, inspect their runs in Schedule, and keep your own judgments and later checks in a Journal.

## What makes it a Loupist

- **It carries lessons forward.** Ordinary research can retain Sources and refine research methods. A later relevant turn reads those revisions; historical inputs remain available in private research records.
- **Primary filings, two markets, one conversation.** OpenBB brings SEC EDGAR, FRED, keyless market data, and configured data vendors into the same chat as native OpenDART, World Bank, KRX, and the Korea Investment read catalog. Credentials stay on the device.
- **Its own math.** Pi's native file tools let the Loupist read and edit files in that folder. `run_python` runs numerical code in a pinned offline Python runtime with no shell and no network. Research scripts and results stay in that folder, and charts can preserve exact data revisions.
- **Records you own.** A Loupist folder preserves its Chats, retained originals and referenced tool activity. Open a copied folder to restore that Loupist. Each Loupist owns its sources, World, Lens, notes and private judgments. Tool results appear in Chat progress; files, charts, documents and saved Memory open beside the conversation. Removing a Loupist unregisters it; its folder, Chats and Memory remain available to reopen.

## How a Loupist remembers

The main destinations are **Chat · Map · Lens · Schedule**, with Settings beside them. The Map Library has Sources and Notes tabs, including Journal entries under Notes; Lens holds methods and principles. Briefings and scheduled prompts run while the app is running. Memory is the internal name for the records retained and reused by each Loupist. A quality-compounder Loupist and a deep-value Loupist keep their own sources, knowledge, and perspective.

| Kind | What it holds |
| --- | --- |
| Source | Retained material, its origin, and the exact passages used |
| World | Entities, events, and relationships supported by retained Sources |
| Lens | Reusable research methods and your principles, with scope, limits and revision history |
| Note / Journal | Chat working notes and your judgments, dated checks and later outcomes, with their exact inputs |

The composer has one **Private chat** switch. With it off, Chat can read saved context, retain Sources and revise Lens; turning it on skips Memory reads and saves together. Chat history still persists. New Loupists default to automatic World processing with a recommended connected model, editable in Settings. Existing Loupist folders retain their settings. New retained sources enter World processing when a model is configured and processing is enabled. A later relevant turn must use the learned state; storing a record alone does not establish learning.

> [!NOTE]
> This is a bounded claim, not a black-box one. A Loupist turns source-grounded experience into reusable knowledge that changes how it handles a later relevant case. It is not model-weight training, a promise of returns, or a single performance score.

## Install

The first release targets macOS 13 or newer on Apple Silicon only. Windows, Intel Macs, and Linux are not supported in this release. Signed installers will be published through [GitHub Releases](https://github.com/loupistai/loupist/releases); until an installer is available, use the [development setup](apps/loupist/README.md). See [installation.md](installation.md) for installation instructions. Releases require macOS signing and notarization. The app accepts only updates signed by the Loupist updater key.

## Security model

- React submits typed intent and has no raw filesystem, process, credential, or Memory-write authority.
- Rust owns Loupist isolation, persistence, and Memory writes.
- Native file tools are restricted to admitted workspace, scratch and attachment paths; private app data and credentials are refused. `run_python` is the only agent code execution tool. It has no shell, network or host environment, and it stays allowed after third-party text. See [Native tool permissions](docs/runtime/permissions.md).
- Model, market-data and Python workers run behind Rust authority; World extraction runs inside the app. The host owns their process lifetimes and allowed operations.
- Memory is the Loupist's durable learned state. Bundled Skills define product methods; workspace instructions and Skill entrypoints enter through a bounded, hash-checked snapshot.

## Development

Use Node.js 26.13+ LTS (26.x; see `.nvmrc`), Python 3.12 with `uv`, and Rust (CI pins 1.97.1).
The clean-checkout verifier installs locked dependencies and runs the web, Rust
and Python checks:

```sh
scripts/verify.sh
```

With dependencies installed, `npm run check --prefix apps/loupist` runs the
same checks without reinstalling. See [desktop development](apps/loupist/README.md),
[native UI testing](apps/loupist/e2e/README.md) and
[release acceptance](LOUPIST.md) for the relevant workflow.

> [!IMPORTANT]
> Loupist develops a user's investment-research Loupist from source-grounded experience. It is an AI research companion that can share its view when asked; it is not a licensed adviser and never executes trades.

Loupist is licensed under AGPL-3.0-only. See [Licensing](LICENSING.md) for app code, contributions, shared packs, provider data and Gallery terms.
