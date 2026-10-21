---
name: triage
description: Triages Tau user issue reports from the local feedback inbox (issues.jsonl) — groups them, checks for existing fixes, provides standalone agent prompts for each report, investigates and fixes selected reports, and moves confirmed resolutions to done.jsonl. Use when the user asks to go through reported issues, triage feedback, list open reports, investigate or fix a reported issue, or mark a report as resolved.
---

# Issue report triage

Tau users submit issue reports from the sidebar. Reports are appended to a local
inbox that nothing reads programmatically; working through it is a manual
convention, and this skill is that workflow. `docs/observability.md` is the
authoritative description of the files.

## Files

Default to the production profile. Development builds now use a per-process
temporary profile, printed at startup; see `docs/development-storage.md`. Ask for
that run's directory when triaging development feedback, and copy reports before
the app exits. The old persistent `tau-dev/` directory is legacy data only.

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
4. Present a concise grouped overview in chat: category, then per-report
   number, date, a one-line summary (quote verbatim where the wording matters),
   whether a session ID is attached, and status (open / possibly fixed / needs
   reproduction). Keep report numbers stable within the conversation, including
   after cancellations or resolutions; do not renumber the remaining reports.
5. Follow the overview with a standalone, copy-paste agent prompt for each
   outstanding report, using the format below. Include these by default, not
   only when the user asks for handoff prompts. Updated lists after cancellations
   or resolutions should also include prompts for the remaining reports, unless
   the user explicitly requests only a short overview. If the inbox is empty,
   say so without generating prompts.

Deliver the overview and prompts in the conversation; do not write triage notes
or prompt files to disk.

## Standalone agent prompts

Use a heading such as `### #5 — Allow queueing while compacting`, followed by one
fenced `text` block containing the entire prompt. The receiving agent has no
conversation context, so each block must be independently usable:

- Identify the actual repository working directory and describe Tau as a native
  Tauri desktop UI for Pi, with a Vue frontend and Rust backend. Tell the agent
  to read `AGENTS.md`, `docs/quality.md`, and task-relevant repository docs.
- Include the report's description, readable date with timezone, original
  `timeUnixNano`, and exact session ID when present; otherwise state that no
  session ID is attached. Resolve references such as “continuing” using related
  reports so the prompt makes sense alone.
- State what is known from triage, including concrete candidate commits or
  relevant resolved work. Distinguish confirmed facts, user observations, and
  hypotheses. Do not imply a candidate fix or shared root cause is verified.
- Give a focused task and acceptance criteria grounded in the report. For a
  clear request, ask for implementation; for an unexplained failure, ask for
  evidence-backed investigation and a fix only if a defect is established.
  Do not invent requirements or prescribe an unverified implementation.
- Include relevant constraints, likely edge cases, and known code pointers
  without investigating solely to flesh out the prompt. For runtime issues,
  point to read-only telemetry in the applicable profile and warn that historical
  records may have expired. Preserve Pi ownership and compatibility, session
  isolation, drafts, and telemetry privacy where relevant.
- Ask for deterministic regression coverage, relevant repository checks, and a
  build for changes, followed by a summary of findings, changes, and validation.
  If evidence is insufficient, ask for the limitation and smallest next step
  rather than a speculative fix. For unsettled UI design, request design review
  before implementation; do not reopen a settled design direction.
- Explicitly prohibit modifying `feedback/issues.jsonl` or
  `feedback/done.jsonl`: the main agent owns resolution bookkeeping.

Keep each prompt proportionate to the report. Repeat essential context rather
than referring to “the prompt above.” Related reports may be grouped in the
summary, but each prompt should still stand alone; flag overlapping work so
agents are not blindly assigned competing fixes.

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
`docs/reproduction.md`).

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

- Do not start investigating, fixing, or delegating execution during the inbox
  pass; the pass ends at the grouped overview and standalone prompts, and the
  user picks which reports to work on.
- Never move or edit inbox entries without the user naming the report and its
  resolution.
- Do not modify telemetry files; they are read-only evidence.
- Do not build automation that reads these files at runtime — the split is a
  human convention (see `docs/observability.md`).
