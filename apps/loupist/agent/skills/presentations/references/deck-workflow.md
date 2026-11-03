# Deck workflow

Original Loupist guidance grounded in python-pptx's slide/layout model.
See [sources](../../NOTICE.md).

## Inspect and plan

Inspect slide size, masters, actual layout names, placeholders, theme fonts,
slide order, speaker notes, charts, and editable objects. Layout indices are
not universal across templates. Select layouts from the supplied deck rather
than assuming index one is always a title-and-content slide.

Extract content from text boxes and tables; inspect chart data and embedded
media separately. A text dump is not a complete reading of a graphical slide.
Preserve notes and unrelated content when modifying an existing deck.

## Build and verify

Choose a narrative that answers the user's question. Keep source labels and
units next to material financial data. Use editable shapes, text, tables, or
charts where the requested deliverable needs later editing; use images when
appropriate to the actual content and available tools.

Reopen the saved deck to check slide count, content, and package integrity.
Render slides through an available office engine and inspect every authored or
changed slide for overlap, off-slide objects, clipping, missing glyphs, and
unreadable charts. Do not treat valid slide XML as proof of a good visual.

Check the library's preservation support before round-tripping specialized
features. If rendering or an unsupported feature cannot be verified, state
that limit and deliver the saved file without claiming a successful preview.
