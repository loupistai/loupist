# Saved Python scripts

Open **Library → Scripts** in a Loupist's Map to find its saved Python. A version
shows the summary, arguments, code, origin, the latest run and the tasks that use
it. Test runs that version with the supplied arguments and shows the output. It
uses the Loupist's enabled data reads without reaching workspace files or
attachments. Export opens the native save dialog for a `.py` copy.

The agent saves a script with `script_save` and finds it with `script_list` or
`script_read`. In a Chat or scheduled run, `run_python` takes a name for the
latest version, or `name@version` for an exact version. The script's argument
defaults apply unless the run supplies its own values.

A check pins `name`, `version` and `sha256`. Saving a new version of a live check proposes it; the approved hash keeps running until the user turns the new one on. Go back re-pins only a version that was turned on before. Deleting the task leaves the script in the library. Versions those pins name are not pruned.

## Versions and storage

The Loupist's database holds each version's code and SHA-256 digest. Editing or
restoring creates another version. A version's latest read and test metadata
can be refreshed as it runs. The library travels with the Loupist folder.
Scripts are excluded from shared packs.

A Loupist keeps up to 50 named scripts and 8 MiB of script data. Code is limited
to 32 KiB and summaries to 300 characters. The latest 20 versions of a script
are kept along with any older versions pinned by a task. A pin prevents
deletion. Undo on a saved version removes that version after checking its pins.

## Reads and replay

The bundled `loupist` module offers `call(tool, **arguments)`,
`dataset(dataset_id, revision, pointer=None)`, `followed()`, `price(asset_id,
days=0)`, `sources(query, limit=10)` and `source(id)`. `gather` takes a list
of `(tool, arguments)` pairs. The program can read its `args`, named `inputs`
and the host's `now` value.

Rust fetches named inputs, starts the sandbox and serves newly requested reads
after that worker exits. The next start sees the same values and failures for
all calls already served. A Python call has up to four starts and 32 distinct
reads. Only the final start can copy workspace changes back. Refused or failed
reads surface as `loupist.Refused` or `loupist.Unavailable` with a reason code.
The Python result can cite the reads staged within its own delivery. Those
references become available to other tools when the parent delivery is acknowledged.
The dataset helper gives the script the selected JSON value and gives the
parent result the host's dataset receipt as evidence.

The [Python exposure policy](../runtime/permissions.md#python-execution)
limits available methods. Custom data requires an unchanged Always approval in
a Chat. The host applies the same connection grants and as-of cutoff to direct
and Python reads. Third-party text raises the turn's trust tier before the host
serves any further reads, including after a worker start sees a tagged file. Python cannot
send messages, call a model, write Memory, or make a web request.

Activity keeps a step's read list. When retained replay data is available,
**Run again on the same data** uses the step's stored code, arguments, time
and ledger with no provider requests. It compares the output and reports any
difference. A replay has no live workspace inputs or writable output. Run logs
carry read and round counts without the code, arguments or results.
Before retaining replay data, the host screens the complete record for
credential-like content, including named inputs and read results that Python
never printed.
Retained replay records are private to the Activity step. Dataset tools cannot
read or export them, even with a pinned reference.
