---
name: why-it-moved
description: Explain why a stock, fund or index moved on a day or over a period, keeping the measured move, reported reasons and inference apart. Use for questions like why something rose or fell; skip a plain price lookup.
---

# Why it moved

1. Measure the move from dated closes: the last close before the window and the
   last close in it, with `calculate` `percent_change` citing both. Say which
   dates it covers.
2. Measure the market and the sector over the same dates the same way (an index
   or a sector fund). When no connected source has them, say the comparison was
   not checked.
3. Look for reasons dated in the window or just before it: the company's
   filings first (OpenDART, SEC), then timestamped news from official or keyed
   providers, then `web_search` and `web_fetch`. For a Korean stock, add Korea
   Investment investor flows for those days when connected.
4. Answer in four short sections, titled in the user's language:
   - "Moved {x} while the market moved {y}"
   - "Reported reasons": only what a dated source says, cited
   - "{your Loupist's name}'s reading": your inference, labelled as inference
   - "What isn't explained"

Never invent a cause. Something published after the move did not cause it, and
a source that only restates the move is not a reason. When nothing explains the
move, say so plainly. Read [move attribution](references/move-attribution.md)
for timing, event types, flows and wording.
