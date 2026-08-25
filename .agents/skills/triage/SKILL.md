---
name: triage
description: Triages Tau user issue reports from the local feedback inbox (issues.jsonl) — lists and groups them, checks whether any are already fixed, investigates and fixes selected reports, and moves confirmed resolutions to done.jsonl. Use when the user asks to go through reported issues, triage feedback, list open reports, investigate or fix a reported issue, or mark a report as resolved.
---

# Issue report triage

Tau users submit issue reports from the sidebar. Reports are appended to a local
inbox that nothing reads programmatically; working through it is a manual
convention, and this skill is that workflow. `docs/observability.md` is the
authoritative description of the files.

## Files

Default to the production profile; use `tau-dev/` only when asked about
development builds.

- Inbox: `~/Library/Application Support/tau/feedback/issues.jsonl`
- Resolved: `~/Library/Application Support/tau/feedback/done.jsonl`

Each inbox line is `{"timeUnixNano": "<ns string>", "description": "...",
"sessionId": "..."?}`. `sessionId` is present only when the reporter chose to
include it. Timestamps are nanosecond strings — convert to readable dates when
presenting. An empty `issues.jsonl` means no outstanding reports.

## Triage workflow

1. Read every line of the inbox.
2. Group related or similar reports into categories: same feature, same
   symptom, or likely same root cause. A report can be its own group.
3. For each report, judge whether it may already be fixed: check `git log`
   since the report's timestamp and `done.jsonl` notes for overlapping work.
   Most reports are not fixed yet — flag "possibly fixed by `<commit>`" only
   with a concrete candidate commit.
4. Present the grouped list in chat: category, then per-report date, a
   one-line summary (quote verbatim where the wording matters), whether a
   session ID is attached, and status (open / possibly fixed / needs
   reproduction).

Deliver the report in the conversation; do not write triage notes to disk.

## Using instrumentation

Tau has extensive local instrumentation: content-free OpenTelemetry JSONL under
`~/Library/Application Support/tau/telemetry/` — `traces.jsonl`, `logs.jsonl`,
and `metrics.jsonl` plus rotated segments, retained for seven days and 256 MiB.
Records carry operation names, lifecycle state, timings, outcomes, counts, and
session/runtime identifiers — never prompts, transcripts, paths, payloads, or
raw errors.

Use it to reproduce an issue or collect more context: filter records by the
report's `sessionId` and search the minutes around its `timeUnixNano` for
failed spans, error-level logs, or unusual timings. For deterministic UI
reproduction afterwards, use the scenario tooling (`bun run repro -- --list`,
`docs/reproduction-scenarios.md`).

## Investigating and fixing a report

When the user picks reports to investigate or fix, prefer delegating each one
to a subagent rather than working through it in the main conversation: hand it
the report's description, its session ID, any telemetry findings, and relevant
code pointers, and have it come back with a root cause or a verified fix.
Independent reports can be delegated in parallel. Keep the inbox overview,
cross-report grouping, and resolution bookkeeping in the main conversation so
the triage picture stays in one place.

## Resolving a report

Only with the user's explicit confirmation of the outcome:

1. Append the report to `done.jsonl`, keeping its original fields and adding:
   - `resolvedAt`: current time as a unix-nanosecond string
   - `resolution`: `fixed`, `deferred`, or `not-an-issue`
   - `commit`: the fixing commit hash (only for `fixed` with a known commit)
   - `note`: one short sentence on the outcome
2. Remove exactly that line from `issues.jsonl`, leaving every other line
   untouched. Tau only ever appends to the inbox, so re-read the file
   immediately before rewriting it.

## Non-goals

- Do not start investigating or fixing during the inbox pass; the pass ends at
  the grouped list, and the user picks which reports to work on.
- Never move or edit inbox entries without the user naming the report and its
  resolution.
- Do not modify telemetry files; they are read-only evidence.
- Do not build automation that reads these files at runtime — the split is a
  human convention (see `docs/observability.md`).
