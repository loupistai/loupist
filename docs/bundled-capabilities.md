# Bundled capabilities

This page owns Connections, credential setup, Skill management and the bounds and
scans of [shared Loupist packs](capabilities/packs.md#shared-loupist-packs). The official
catalog is `apps/loupist/marketplace/marketplace.json` plus `plugins/<name>/`.
Settings → Connections configures bundled services and the user's own
[custom connections](capabilities/connections.md#custom-connections); it does not install packages.
The Loupist settings sheet controls each Loupist's enabled bindings; open it from the
Loupist header or Settings → Loupists. [Architecture](architecture.md#trust)
owns runtime trust boundaries.

## Connections and credentials

Secrets stay in Rust. Grants are per Loupist. [Connections](capabilities/connections.md#connections-and-credentials).

## Agent-facing data services

Read-only bundled data tools and how a turn selects them. [Data services](capabilities/data-services.md#agent-facing-data-services).

### Provider data policy for packs

What fetched data a pack may carry. [Shared packs](capabilities/packs.md#provider-data-policy-for-packs).

### Public blockchains and DefiLlama

Address reads and TVL from fixed hosts. [Data services](capabilities/data-services.md#public-blockchains-and-defillama).

### Maintaining the Korea Investment read catalog

Account fields are reviewed before a release can expose them, and quantities are still redacted. [Data services](capabilities/data-services.md#maintaining-the-korea-investment-read-catalog).

## Skills management

Which Skills ship, and what enabling one does. [Skills](capabilities/skills.md#skills-management).

## Shared Loupist packs

File, text and license bounds for a pack. [Shared packs](capabilities/packs.md#shared-loupist-packs).

## Custom connections

A user-selected MCP server, its secret, and the approval hash. [Connections](capabilities/connections.md#custom-connections).
