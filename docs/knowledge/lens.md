# Lens learning

Lens holds perspectives, principles and methods. Learning proposes them; nothing becomes Memory until the host commits it. The Map control is [Map and Lens](../ui/map-lens.md).

## Lens

A Loupist has [user principles](#user-principles), a standing perspective and method
Lenses. Memory-enabled turns can receive a bounded standing perspective
automatically; oversized content requires an exact `lens_read`. Methods have six
fields: Markdown, applicability, assumptions, procedure, principles and
limitations. Discovery is not a read of those fields.

The **Lens** panel opens Research principles for the current Loupist alongside
the Chat. It lists user principles, the standing perspective and learned methods;
the World Library contains Sources and Notes only. Opening a method renders its
saved document, links and headings. In the side panel the document scrolls on its
own, and Outline and links stays at the top of that document. View Markdown reveals the saved body,
including `lens:` links, followed by its populated fields under section headings.
For a user principle these are the statement, why and example. Rust
versions these records in local Memory. `LensWorkspace` is a document reader, with applicability,
limitations, outline, links and backlinks. Learned methods on the Research principles
page retain their provenance panel alongside the saved document. The learned-method inspector shows the
learning rationale, cited World facts and a Then vs now comparison built from bounded
growth-event before/after summaries. Keep acknowledges the publication for the current
app session and Loupist. Archive confirms inline and asks Rust to archive the Lens;
the default perspective cannot be archived. `lens:recordId` links resolve within the
same Loupist and follow current heads. Discuss in Chat attaches the saved Lens;
proposals can route back to their originating Chat or be dismissed. Saved basis shows
the current change rationale and evidence. There is no historical Lens browser.

A principle or method can be paused independently of learning. Rust stores paused
ids in the existing per-Loupist JSON settings, excludes them from automatic context,
search and selected references, and refuses exact model reads until resumed. The user
can still inspect a paused item and resume it. Paused principles also leave autonomous
method-review input. The standing perspective cannot be paused.

Method and perspective creation and revision are requested through Chat, or come
from accepting a [shared Loupist's](sharing.md#sharing) suggestion. Those
document readers do not expose create, save, rename or archive controls.
`lens_update` can warn about related current methods without granting read
authority or blocking a save. A stale-revision update becomes a read-only
proposal rather than replacing newer content.

### Method learning

Method learning defaults on. It also requires World processing and Memory saving
to stay enabled, and the Loupist must not be paused. After at least five added or
disputed claims since the previous review, Rust records one pending review. The
review stays pending until that Loupist has no queued or running World jobs. It is
admitted at most once every six hours and four times in a rolling day, using
the selected World model on the same isolated inference path as World processing.
Pending work survives restart, includes claims committed while the batch drains,
and wakes at the next permitted time even if no later World publication arrives.
A paid or uncertain attempt is not retried automatically. A review left running
at startup is marked a failed uncertain attempt.

Publication requires at least two cited claims from that review's input. The
input includes every active principle statement. If those statements do not fit,
or they change before publication, the review defers and publishes no method.
Deferral and failure write a review record and no growth event. Learned means a
retained method, not model-weight training or a measured quality gain. A saved
method alone does not demonstrate improved later work; use
[evaluation](../../apps/loupist/evals/README.md) to test transfer. Loupist settings
shows Learn methods from new information and the ten most recent reviews. A
successful review announces the method and links to its document in Lens. When
that Loupist's Map is visible, the World entities it applies to glow for four
seconds, suppressed under reduced motion. An
unacknowledged learned publication stays marked New for seven days.

### User principles

User principles are Lens records with `lensKind: principle`, a statement, optional
why and example, and user origin (`settings`, `feedback` or `chat`), or `shared` for one
accepted from a [shared Loupist](sharing.md#sharing). The Lens panel shows the statement, its origin and application history. Asking the Bot for a change uses the admitted Memory tools. A principle linked from a World entity opens in Lens. An origin Chat
link opens that conversation without changing its draft. Saved answer corrections
can become principles. Feedback and the Memory row offer Undo, which archives, and
View, which selects the principle in Lens. Direct creation accepts a
settings or verified answer-feedback origin. Active statements are unique after
normalization. Listing titles are limited to 80 characters; delivery receipts keep
the full statement.

Memory-enabled turns receive active, unpaused principles in their own block,
newest first, at most 30 rules and 6,000 characters including the overflow notice.
They outrank the standing perspective. Memory-off turns omit them. Only statements
actually attached to that bounded block create context receipts and usage deliveries.
Delivered context counts as acknowledgement for replacement and does not by
itself create Map `applies_to` edges. An exact Lens read returns `lens_url` to open the rule or method from an answer. A final-answer `lens:` link to a current
same-Loupist principle or method records applications to World entities delivered
in that turn. `principle_save` stages a Chat-origin change only with Memory read
and write permission. Replacement requires the admitted or explicitly read
current revision. Successful turn finalization commits the change with the reply.
Archived principles stay readable and leave automatic context.
