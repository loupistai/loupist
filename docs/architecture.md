# Architecture

This page owns execution authority, storage, onboarding, the shell, Chat and
schedules. Record semantics belong to [Knowledge and Memory](knowledge-model.md); dataset and chart behavior
belongs to [Financial analysis](analysis.md).

## Platform boundary

The release target is macOS 13+ on Apple Silicon. Linux is a development and
native-test host (WebKitGTK), not a release target. Keep bridge contracts and durable
records independent of the OS; paths, process recovery, credential storage and
packaging own platform-specific behavior. [CI and releases](ci-cd.md) owns support
and qualification procedures.

## Trust

React submits typed intent. Rust owns credentials, SQLite transactions, broker permissions and process lifetime. Native tools run as the user, confined without asking: file tools work inside the Loupist folder, Bash runs inside an OS sandbox that goes offline once the reply has read third-party text, and Rust decides each call from the target's path class and the reply's exposure: [Native tool permissions](runtime/permissions.md). Credentials, custom MCP and where bytes live: [Storage and authority](runtime/storage-authority.md#authority-and-credentials). The untrusted-text tier, the known-site rule and holdings redaction: [Untrusted text](runtime/untrusted-text.md). GitHub, the Gallery and import links: [Publishing](knowledge/publishing.md).

## Find a topic

- [Where data lives](runtime/storage-authority.md#where-data-lives)
- [Scoped configuration](runtime/storage-authority.md#scoped-configuration)
- [Chat](runtime/chat-turns.md#chat)
- [Local workspace execution](runtime/chat-turns.md#local-workspace-execution)
- [Shared providers and World processing](runtime/providers.md#shared-providers-and-world-processing)
- [Turn authority and finalization](runtime/chat-turns.md#turn-authority-and-finalization)
- [Asset pages](knowledge/asset-pages.md#asset-pages)
- [Onboarding](runtime/onboarding.md#onboarding)
- [View state](ui/)
- [Runs](runtime/scheduled-runs.md#runs)
- [Scheduled Chats](runtime/scheduled-runs.md#scheduled-chats)
- [Results before and after](runtime/scheduled-runs.md#results-before-and-after)
- [Local watchers](knowledge/watchers.md#local-watchers)
