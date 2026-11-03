# Home Thread

The persistent home Thread appears as Bot. Each Loupist has one Bot. Research sessions have separate titles and drafts. The app restores the selected conversation and its unsent draft on restart, including attached files, the exact inline references, the selected work, Skills and the as-of date. Scheduled results, crossed lines, watcher alerts and the morning message are host posts, not model calls.

Each Loupist has one persisted home Thread (`home-{loupist}`), created on workspace load
with the Loupist's Chat defaults; rename and delete refuse it. Opening a Loupist lands
there unless a saved chat or an unsent new chat was open. When a scheduled run
finishes (succeeded or failed) after the Thread exists, Rust posts one host message
into the Thread with the run's first paragraph and a `delivery` record (run, task,
cadence, status, run chat), built from the run and task only, never from holdings.
Posting takes the chat-mutation lock; while a reply runs in the Thread, delivery
waits for the next completion or workspace load. The result reads "From: {task} ·
{cadence} · Open" and Open goes to the run's Chat; showing results marks them read.
A crossed asset-page line posts one host message per move through the same path
once the committing reply finishes (a busy Thread gets it on the next completion or
workspace load). Its `alert` record (`HomeAlert`) holds the asset, move, name,
symbol and market, the user's claim, the observed value with its unit, when the line
was set, the fact date and the first source, never holdings. Undone crossings are never announced. It
reads "{name} crossed the line you set on {date}: … Thesis marked Broken." with
Review thesis and Undo (by move id, while it is the latest move); there is no OS
notification.
[Watcher](../knowledge/watchers.md#local-watchers) alerts the morning message did not take post through the same
path as one host message per check (`watch` record: up to five dated alerts with their
sources, then "+n more", built from stored alerts only); an alert on screen for a second
while the window is visible counts as seen. The [results task](../runtime/scheduled-runs.md#results-before-and-after)
posts one message (`earnings` record) when reported figures first arrive ("{name} reported
{period} results." with the two most material measures and See before and after) or when
a before-capture failed (Try again runs `earnings_prepare`); a successful before-capture
posts nothing, and its runs, which target the Thread itself, get no `delivery` message.
Neither carries holdings or sends an OS notification.

An empty home Thread opens on the Loupist's morning message; a Thread with history
shows it at the end when there is news since a counted last visit and the user has
not written this visit, and an empty topic Chat shows only its questions. The message opens with a "Coming up"
strip when anything is ahead: up to five events in the next 14 days from the read-only `upcoming_events` command
(no model call, no quantities) — the next unreported results date and the sooner dividend date per watched or held
asset, and open dated Journal checks — soonest first, each opening its asset page or Journal entry; a check shown there
is not repeated below, and the strip is hidden in as-of chats and topic Chats. Then comes a lead sentence, then what changed
on the Map since the user last looked, What you follow (up to three followed assets
with a material asset-page change since then, each with its read and most material
row, then "+n more", and up to three unseen watcher alerts the first check after launch
or of the day handed to it; not on a first visit), the latest briefing and pending dated
Journal checks, on the first visit of a Loupist made from the Crypto, Macro & rates or
Commodities template a "Works best with {connection} · Open Connections" line naming the
recommended connection it lacks (Tiingo, FRED or EIA; enabled and available for the
Loupist counts as having it),
and three locally generated questions that fill the composer without sending. A first
visit counts what there is to look at instead of naming a day; general questions fill
the list when the Loupist has little to point at. It is
computed in the renderer (no model call) and is not persisted. "Last seen" is a
per-device value per Loupist in `localStorage` (`loupist.loupistVisit.v1`), written
when the user leaves a Loupist after a visit of at least five minutes.
