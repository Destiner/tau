# Reproduction and regression authoring

Use deterministic native/controller fakes for product regressions, fake stdio for
process behavior and explicit installed-Pi checks only for contract drift. There is
no browser scenario runner or Playwright suite. See [CI/testing](ci-testing.md) and
[development storage](development-storage.md).

## Choose the owning layer

- `crates/tau-core/`: file-backed profiles/registries, process JSONL, event tagging,
  SSH command policy and cleanup. Use temporary profiles and fake commands/stdio.
- `crates/tau-session/`: RPC correlation, generations/identity, replacement,
  materialization, queues, extension work and recovery. Use deterministic exchanges
  with controlled ordering and time.
- `gpui/`: actual retained controls, focus, drafts, selection, transcript geometry,
  interaction and paint. Use production-root tests and editor/Metal integrations;
  controller-only assertions do not prove visible behavior.
- Bun tests cover native command planning/packaging and other retained tooling without
  launching the product or contacting providers.

Minimize a report into invented data and a causal event sequence. Keep exact owners,
request/response correlation and explicit gates; reject unexpected work. Demonstrate
failure before the fix and success afterward without widening deadlines, adding
arbitrary retries or repairing activation in the harness. Preserve durable regressions,
not migration-only comparison captures. Never copy private prompts, transcripts,
credentials or raw errors into fixtures or telemetry.

## Manual reproduction

For a concrete issue, use a fresh explicit isolated profile and disposable project:

```sh
bun run dev -- --isolated /absolute/profile --project /absolute/project
```

This is not a filesystem/tool sandbox: Pi retains credentials, extensions and real
project access. Do not run against production or authenticate SSH as an incidental
check. File-preview scope is text/Markdown/code and admitted images; PDF/audio/video
acceptance is not claimed. See [APP.md](../gpui/APP.md) for current interaction,
workflow, cleanup and deferred acceptance boundaries.

## Real Pi compatibility canary

```sh
bun run test:pi-contract
```

This is an explicit check of installed Pi, excluded from default tests. It requires
Pi on `PATH` or `TAU_PI_PATH` and fails if missing/incompatible. The canary uses
temporary working/config/session directories, offline mode, disabled tools and
extension/resource loading, and no provider credentials.

Only read-only RPCs are sent: `get_available_models`, `get_commands`, `get_state`,
`get_available_thinking_levels`, `get_messages` and `get_entries`. There are no prompts, model calls,
SSH or external services. Assertions cover JSONL/process usability, correlated IDs
and consumed fields, not catalogue names/order or environment-specific paths. Read
`scripts/pi-contract.ts` for exact isolation and checks before running it.

Native OS/installed-Pi/model acceptance is separately opted in via selected
`native-os-tests` targets; it is not implied by a deterministic test pass. The last
automated busy-close failure remains negative evidence; user manual workflow and SSH
acceptance and explicitly deferred campaigns are documented in APP, not an instruction
to rerun them during cleanup.
