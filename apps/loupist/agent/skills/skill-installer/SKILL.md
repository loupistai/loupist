---
name: skill-installer
description: Find, inspect, or install existing Skills from a requested GitHub folder, local folder, or ZIP. Use for adding Skills; authoring or revising one uses skill-creator.
---
# Skill Installer

Adapted from OpenAI's Skill Installer (Apache-2.0); Loupist destinations and installation workflow are modified.

Use the skill folders in the runtime context: this Loupist by default, personal when the user asks for availability across Loupists. Settings → Skills offers ZIP/folder imports and public GitHub folder URLs. Native file/Bash tools can perform the same requested task. Do not use `$CODEX_HOME`, install a whole repository, or modify bundled resources.

When the user asks what is available, inspect the named source and show relevant choices without installing them. OpenAI's public examples are at https://github.com/openai/skills and https://github.com/openai/plugins; follow the source's current guidance and each skill's own license. Do not assume every skill in a repository has the same license.

For an authorized installation, inspect the exact skill folder and its SKILL.md, references and scripts. Retain applicable license/notice files. Download or clone into a temporary directory, validate bounded relative paths and the required YAML name/description, then copy only the selected skill to its destination. Reject archive traversal, symbolic links and an existing destination. Do not execute downloaded setup scripts, hooks, package installers or extensions as part of adding instructions. Private repositories require the user's existing authorized Git access; never expose credentials or silently substitute another source.

Keep the requested scope and source. Confirm the resulting SKILL.md and relative resources exist, report the installed location, and say it is available on the next Chat turn. The Skills screen can reload external changes. Installing instructions does not enable a connection or expand Memory permissions.
