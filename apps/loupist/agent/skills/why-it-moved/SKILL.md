---
name: why-it-moved
description: Explain why a stock, fund or index moved on a day or over a period, keeping the measured move, reported reasons and inference apart. Use in a research session, or when the user picks this Skill, for questions like why something rose or fell; in the Bot such a question is a quick ask answered without it. Skip a plain price lookup.
---

# Why it moved

This is the full attribution, for a research session or a user who picked this
Skill. In the Bot, without that choice, a question about a move is a quick ask:
answer it as one and leave this procedure; the worked-out reason goes to
`research_open`.

1. Measure the move from dated closes: the last close before the window and the
   last close in it, with `calculate` `percent_change` citing both. Say which
   dates it covers.
2. Measure the market and the sector over the same dates the same way (an index
   or a sector fund). When no connected source has them, say the comparison was
   not checked.
3. Look for reasons dated in the window or just before it: the company's
   filings first, from its market's filing register where a connection reads
   it, then timestamped news from official or keyed providers, then
   `web_search` and `web_fetch`. Add investor flows for those days when a
   connected source reports them.
4. Cite dated sources for reported reasons, label your own inference as
   inference and say what remains unexplained.

Never invent a cause. Something published after the move did not cause it, and
a source that only restates the move is not a reason. When nothing explains the
move, say so plainly. Read [move attribution](references/move-attribution.md)
for timing, event types, flows and wording.
