# Architecture

This page owns execution authority, storage, onboarding, the shell, Chat and
schedules. Record semantics belong to [Knowledge and Memory](knowledge-model.md); dataset and chart behavior
belongs to [Financial analysis](analysis.md).

## Platform boundary

The release target is macOS 13+ on Apple Silicon. Keep bridge contracts and durable
records independent of the OS; paths, process recovery, credential storage and
packaging own platform-specific behavior. [CI and releases](ci-cd.md) owns support
and qualification procedures.

## Trust

React submits typed intent. Rust owns credentials, SQLite transactions, broker permissions and process lifetime. Native tools run with the user's host permissions; the workspace is not a filesystem sandbox. Credentials, custom MCP and where bytes live: [Storage and authority](runtime/storage-authority.md#authority-and-credentials). The untrusted-text tier, web-site approval and holdings redaction: [Untrusted text](runtime/untrusted-text.md). GitHub, the Gallery and import links: [Publishing](knowledge/publishing.md).

## Where data lives

Each Loupist is a local folder with its own database. Scoped configuration does not leak across Loupists. [Storage and authority](runtime/storage-authority.md#where-data-lives).

### Scoped configuration

Scoped keys are per Loupist or global, checked in Rust. [Storage and authority](runtime/storage-authority.md#scoped-configuration).

## Chat

A Chat turn admits work in Rust and then runs Pi against the selected workspace. [Chat turns](runtime/chat-turns.md#chat).

### Local workspace execution

Native file and Bash tools use host permissions and an allowlisted environment without app credentials. [Chat turns](runtime/chat-turns.md#local-workspace-execution).

### Shared providers and World processing

Provider accounts and Auto are host state. World jobs share that boundary and do not see the renderer. [Model providers](runtime/providers.md#shared-providers-and-world-processing).

### Turn authority and finalization

Finalization is serialized in Rust. A cancelled reply does not roll back files the tools already wrote. [Chat turns](runtime/chat-turns.md#turn-authority-and-finalization).

### Asset pages

The host refreshes a bounded set of reads onto the asset page. Record shape and the thesis are [Asset pages](knowledge/asset-pages.md#asset-pages).

### Onboarding

Creation and import end on the new Loupist's Chat. [Onboarding](runtime/onboarding.md#onboarding).

### View state

Shell tasks: [Sidebar](ui/sidebar.md) (navigation, Needs attention, command palette, export), [Schedule](ui/schedule.md), [Home Thread](ui/home-thread.md), [Workspace panel](ui/workspace-panel.md), [Chat composer](ui/chat-composer.md), [Evidence and the browser](ui/evidence-browser.md), [Answer failures](ui/chat-answer.md). The Map and its Lens control: [Map and Lens](ui/map-lens.md).

## Runs

At most four model workers run, and a Loupist target is not run twice at once. [Scheduled runs](runtime/scheduled-runs.md#runs).

### Scheduled Chats

A saved task reuses ordinary Chat admission, with its own Memory flags and an allowlist when nobody is watching. [Scheduled runs](runtime/scheduled-runs.md#scheduled-chats).

### Results before and after

The results task prepares captures around report dates and does not post a delivery for a successful before-capture. [Scheduled runs](runtime/scheduled-runs.md#results-before-and-after). The comparison itself is [Results before and after](analysis/results.md).

### Local watchers

Watcher checks run on a budget and post into the home Thread. [Watchers](knowledge/watchers.md#local-watchers).
