# Native storage microbenchmark (2026-09-27)

Run: `scripts/bench-native-storage.sh` (repeated after the repair).
Fixture: v1 opt-in disposable native profile; 7 local + 1 simulated remote
project; 589 JSONL files, 321,388,544 bytes (306.5 MiB), 120 short files;
434 registered local / 423 archived, 155 unregistered; 2,389 remote metadata
rows / 2,388 archived. Deterministic invented content; no production transcripts
or SSH host. Pi installed: 0.87.1. Working HEAD at measurement start:
`687da969e16a63dfe9aa19536433cc61a1c1c199` (concurrent uncommitted
changes present). Historical baseline source: `09195aadb6204c8605aed71dc490aa041765069d`.

| Native operation, warm OS cache                                            |   n |       Median | p95 (nearest rank) |                  Range |
| -------------------------------------------------------------------------- | --: | -----------: | -----------------: | ---------------------: |
| Baseline-equivalent local discovery (`list_sessions_in` across 7 projects) |  30 | 1,106.821 ms |       1,163.337 ms | 1,099.843–1,194.048 ms |
| Current-style project-registry JSON write (including 2,389 remote rows)    |  30 |    17.819 ms |          18.275 ms |       17.651–19.525 ms |

The discovery phase was present in the historical full-snapshot mutation path,
confirmed with `git show`; the metadata write represents the scan-free path.
These are **different phases**, not an A/B of the same command or two app
binaries. The 1,089.002 ms median difference (~98.4% relative to the
discovery median) is only a phase contrast; it is **not** a measured user-visible
improvement. The test does not measure input-to-paint, transcript content
readiness, Pi bootstrap, persistence settle through IPC, native app interactions,
or real SSH latency. No observations of those milestones were collected here
(n=0 each); no native UI behavioral verification was performed. Browser tests
must not be substituted for native timing. Re-run the script after changes to
compare identical fixture and phase definitions; do not add phase percentiles
together.
