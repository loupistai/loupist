# What a filing changed

Use this when the question is what a filing changed against the one before it: revised guidance, a segment
that moved, a change of management, a contract that ended, a risk that appeared or was reworded.

## Pair the filings

Compare a filing with the prior filing of the same company and kind: an annual report with the previous
annual report, a quarter with the previous quarter, and with the same quarter a year earlier when the
business is seasonal. When both are saved Sources, start from `world_changes` with `between: {to: <the
later Source>}` before fetching anything again. The host finds the earlier filing of the same declared
company and form (pass `from` when it asks, or to choose another) and lists what the later filing ended,
added, restated and contradicts, and what it does not repeat. A filing that is not saved yet is fetched and
saved first; World reads it after the reply, so until then compare the two documents themselves.

## Compare facts on a like basis

Guidance, segment figures, management and directors, major contracts, customers and suppliers are compared
as stated facts: the same metric for the same period on the same basis (reported or adjusted, consolidated
or separate), forecast against forecast and actual against actual. Revised guidance for one period is a
change; guidance for a new period is a new fact. A restated figure replaces the earlier figure for that
period, so say which filing restated it. An actual does not replace the forecast made before it: report
both and the gap.

`not_restated` lists what the earlier filing said that the later one does not repeat. That is not an
ending. Say that it was not repeated, look where the later filing would state it, and keep it apart from
what the later filing ended.

## Compare narrative as text

Risk factors, the business description, legal proceedings and accounting policies change in wording, not
in figures. Read the same section in both retained Sources and compare the text; for long sections, diff
them in `run_python` (`difflib`) and read the changed passages yourself. Report what is new, what was
removed and what was reworded in a way that changes its meaning, quoting the changed wording briefly with
each filing's date. Reordered boilerplate is not a change.

## Report and file

Lead with the changes that bear on the question, each with what the earlier filing said, what the later
one says and both dates. Keep an end the filing states apart from one taken from a later statement, which
is approximate: no later than that filing's date. With saving on, file one dated section on the subject's
page, changes since the prior report, whose basis cites the spans of both filings.
