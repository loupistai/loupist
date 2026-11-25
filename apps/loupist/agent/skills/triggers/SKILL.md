---
name: triggers
description: "Write, test and repair a check the app runs on a schedule without a model: a price band, a volume spike, a level crossed, a new item in a list, a macro release, or any condition data can decide."
---

# Checks the app runs by itself

A check is a short Python script. The app runs it on a schedule in a sandbox with no network and no files,
and starts you only when it says the condition is met. Nobody is there when it runs, so it has to be right
about the data it gets, honest about what it could not read, and quiet about what it already reported.

## The contract

- Read through loupist only: loupist.followed(), loupist.price(asset_id, days=n), loupist.call(tool, ...),
  loupist.gather([...]). Parameters come from loupist.args, the time from loupist.now.
- End with exactly one loupist.result(met=..., reading=..., as_of=..., evidence=...). reading is one line a
  person can read on a card (at most 160 characters). as_of is the date of the newest data the verdict rests
  on. evidence is a small dict of the numbers behind it.
- loupist.state is a dict the app keeps between runs. Use it so that the same event is met once: store what
  you reported (a date, an id) and compare. Keep it small.
- A read can fail. loupist.Unavailable and loupist.Refused carry a code. Skip what could not be read, name
  it in evidence, and never treat an unread asset as one where nothing happened. If nothing could be read,
  let the exception stand: the app records that the data was unavailable.
- Old data is not news. If the newest date has not moved since the last run, the condition is not met again.
- The check may call only the tools its dry run used. Call each tool you will ever need in the normal path.

## Method

1. Make each read once yourself and look at its shape and its dates.
2. Start from a template when one fits (price-band, volume-spike, level-cross, new-item); otherwise write
   the smallest script that decides the condition.
3. Save it with script_save and propose it with trigger_save. Read the dry run it returns: errors, days
   that could not be replayed, and how often it would have fired. A check that fires most days is noise; one
   that never fires when the user named a recent event is wrong.
4. Tell the user what it checks, what it reads, how often it runs and when it would have fired.

When a check stops working, read its last evaluations with schedule_read, fix the script as a new version
and propose the update. The approved version keeps running until the user turns the new one on.
