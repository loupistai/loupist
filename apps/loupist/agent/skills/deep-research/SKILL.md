---
name: deep-research
description: Research one question in depth from a plan the user approves, keeping a checkpoint file, looking for counterevidence and ending in a cited report. Use when the user picks this Skill or asks for deep research; skip a question a few reads can answer.
---

# Deep research

This is for a question whose answer rests on many sources and takes a long
reply. Such work has to survive a stop and stand up to the strongest case
against it, so it runs from a written plan.

## Plan first

Before any web read, work out which unknowns could change the answer and plan
around them: the questions to settle, the sources that would settle each
(what this Loupist already holds, then primary sources), the outline of the
report, and what the work leaves out. Write the plan to
`outputs/<slug>/plan.md`, with a short slug for the question, and ask once
with `ask_user` whether to start or change it.

## The plan is the checkpoint

File changes survive a stopped reply; nothing else in progress does. After each
step, record in `plan.md` its status and what it found, with the title, URL
and date of each source. When a plan for the question already exists, resume
from it: skip the finished steps, and read again whatever the report will
cite, because a citation can only point at a read made in this reply.

## Evidence

For each conclusion taking shape, look for the strongest evidence against it
and for what would have to be true for it to be wrong, and report what that
search found. Keep the Sources the report rests on and file what it
established, as in any research.

## Deliver

Publish the report with `artifact_publish`: the conclusion first, then the
evidence for and against it, what remains open, and citations as in an answer.
The answer leads with the conclusion in a few sentences and points to the
report. If the work stops short of the plan, say which steps remain.
