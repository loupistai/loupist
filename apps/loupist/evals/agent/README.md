# Agent release evaluation

The [agent eval guide](../README.md#agent-evals) owns trace and judge commands.
[Release v018](suites/release-v018.json) contains ten fictional units. Every unit
states its fixture as runner input, so no profile is prepared by hand; the staged
units' seeds are also kept, by reference to shared Sources and scripts, in the
[setup fixture](fixtures/release-v018.json).

The agent runner stages every unit from its suite entry: replayed research and
saved Sources, scripted World publications, Lens pages that cite their claims,
followed assets linked to their World company, Learning programs with an
earlier occurrence and their scheduled occurrences through the production
scheduler, the user's yes on a card (`approvals`), and seeded or saved checks
with their occurrences and a dry run at each occurrence's clock. For the
guidance check, preserve the
saved proposal and the dry run's observed tools so the judge can verify its
World read. Include the Korean session's observed progress notes in the judge
input.

Judge completed traces with the standard judge. Each unit needs its rubric
verdict and any deterministic checks in its expectations. Inspect tool refusals
against the advertised tool contract as part of release review.
