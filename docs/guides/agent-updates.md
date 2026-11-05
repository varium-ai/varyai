# Agent updates

The Tools/Agent page starts one background task per selected CLI when **Update
all** is pressed. Independent native updaters can run together. Commands that may
write shared package-manager state, including installer fallbacks and uncertain
targets, wait for a shared writer slot. A slow native updater does not hold that
slot or delay other tools.

Each running tool shows its own updating indicator. The update notice lists the
active tools and counts completed tasks, including failures. Each task checks the
installed version after its update command exits; a successful exit without a
verified version increase is reported as unverified. One tool's failure does not
stop the other tasks, and raw command output is not shown in the update notice.

For a single tool, the notice names the outcome directly, such as
`Pi updated: 1.4.0 → 1.5.0`, without a batch count. Multiple verified updates show
the number of updated tools followed by their version changes. Mixed batches
count updated, failed, unverified and not-started outcomes separately, omitting
zero counts. Unverified results are not presented as successful updates or
command failures.

Switching workspace tabs leaves tasks running. Returning to Tools/Agent restores
their progress and results, including tasks that finished while the page was
closed. This state lasts for the current renderer session; it is not a durable
job history across application restarts. Duplicate starts are blocked in the UI,
and the backend separately reserves each tool to prevent overlapping operations
from another window. Shared package-manager writers wait asynchronously instead
of failing because an unrelated update is active.

Installation probing and confirmation still happen before updates start. Multiple
installations or an uncertain target require confirmation because a command may
update only one installation. Connection and model writes are guarded only for
the tools being checked, installed or updated. Other tools keep their connection
switches, model pickers, visibility settings, recovery and restart actions
available. A tool's guard is released after its own version check finishes,
without waiting for the rest of the batch. Cancelling confirmation or failing
preflight releases the selected tools too. Workspace navigation and read-only
rescans stay available. Updates do not start an Agent or replace the running
Vary AI application.

Regression tests use deferred synthetic update commands and temporary fixtures;
they do not run real installers or modify installed tools.
