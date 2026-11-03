---
name: skill-creator
description: Create or revise a reusable Skill and its supporting files when the user asks for a Skill. Teaching a method without that request uses Lens.
---
# Skill Creator

Adapted from OpenAI's Skill Creator (Apache-2.0); Loupist paths, scope, validation and Memory boundaries are modified.

Create a focused skill that another capable model can use. Keep only guidance that changes decisions: local conventions, domain-specific procedures, reliable helpers and actual constraints. Preserve the user's task, authorization and existing invocation policy.

Use the skill folders provided in the runtime context. Default to this Loupist's skill folder; use the personal folder when the user wants the workflow across Loupists. Never write into bundled app resources or Codex's own configuration. Do not overwrite an existing skill without reading it and respecting the user's requested changes.

A skill is a folder named with a lowercase hyphenated identifier, with a `SKILL.md` containing YAML `name` and `description`, followed by Markdown instructions. Put substantial conditional detail in linked `references/`; add `scripts/` or `assets/` only when they materially help. Supporting paths are relative to the skill folder. Keep the entrypoint within 64 KiB.

Write a description that makes the use case clear. Instructions should be concise and leave ordinary reasoning to the model. Check the YAML, linked files and any new script with a realistic example. Changes become available on the next turn; report the saved location and what was verified.

Keep research conclusions in Memory. Teaching without a request for a skill uses Lens; honor an explicit request for a reusable Skill. Installing a skill does not grant connections or Memory permissions.
