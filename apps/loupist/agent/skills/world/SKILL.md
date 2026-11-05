---
name: world
description: Use when investigating what this Loupist already knows about entities, events, and their relationships, or when organizing source-grounded knowledge in World.
---

# World

World is the Loupist's accumulated, source-grounded view of entities, events,
relationships, dates, and attributed claims. A relation may be a forecast,
hypothesis, or disputed statement. Preserve its status and source. The turn's
Memory settings govern reading and saving; selecting this Skill does not change
them.

Find relevant material with `world_search`, then exact-read it before citing it.
World stores names and facts in English and keeps original-language names as aliases, so search in English.
When Memory reads are on, World hints in the turn are discovery only. Search
hits and graph connections are discovery. Follow source spans behind important
factual claims and keep dated priors distinct from fresh evidence. Prefer a
relation name already used in this World when it fits, and keep a more specific
name when it does not.

The turn's `worldVocabulary` carries the same definitions and relationship
signatures used by extraction. It is vocabulary, not evidence or permission to
act. The bundled core describes the current app. If a historical extension is
unavailable at the cutoff, do not apply today's custom meanings to that answer.
An Organization node may name an issuer or project without resolving a particular
share class, listing, bond, token, or contract. Verify that distinction in originals
before drawing instrument-specific conclusions.

A World read's `sourceTargets` supplies `readArgs` for each retained original.
Use those arguments with `world_read`. A complete retained Source is the original
evidence for that capture; reading its file or URL again just to confirm it adds
no evidence. Fetch again for missing coverage or a required newer version.

To add grounded material, save a Source. World processing extracts entities and
relationships from saved Sources. A research focus and anchor entities in World
processing settings steer that extraction without rejecting other names.

Compare claims on the same entity, period, and definition. Keep conflicting
accounts and their original attribution visible. Distinguish later corrections
from facts that were available at the research cutoff. A forecast or hypothesis
must keep that status beside observed facts.

Use `source_save` for an acquired original and `note_save` for a user-stated
fact or interpretation. Source retention and World processing finish separately.
Read `world_job` for committed processing status;
`waiting_for_turn_commit` means finish the reply, not poll.

Explain findings with attribution, dates, uncertainty, and readable source titles.
