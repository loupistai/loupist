---
name: skill-creator
description: Create or revise a reusable Skill and its supporting files when the user asks for a Skill. Teaching a method without that request uses Lens.
---
# Skill Creator

Adapted from OpenAI's Skill Creator (Apache-2.0); Loupist paths, scope, validation and Memory boundaries are modified.

Create a focused Skill that another capable model can use. Keep only guidance that changes decisions: local conventions, domain procedures and real constraints. Preserve the user's task and authorization.

Write it with the file tools into a Skill folder named in the runtime context: this Loupist's folder by default, the personal folder when the user wants it across Loupists. Skill files cannot be written in a reply that has read web pages or other third-party text: write the Skill first, or ask the user to request it in their next message. Do not overwrite an existing Skill without reading it.

A Skill is a folder named with a lowercase hyphenated id holding `SKILL.md`: YAML `name` and `description`, then Markdown instructions. Put long conditional detail in linked `references/` files; keep the entrypoint within 64 KiB. The agent that uses the Skill has file tools, the app's tools and `run_python` (offline Python, no shell, no installs): do not write steps that need a terminal, a download or another program. A helper script is only text the model can paste into `run_python`.

Write a description that says when to use the Skill and when not to. Check the YAML and every linked file by reading them back. The Skill is available from the next message; report the saved path and what you checked.

Keep research conclusions in Memory. Teaching a method without asking for a Skill uses Lens. A Skill does not grant connections or Memory permissions.
