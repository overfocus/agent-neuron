# agent-neuron engine, second implementation (Rust, M1)

This crate is the Rust engine of [`agent-neuron`](../), milestone M1: the M0
sans-IO conformance core plus the process layer — real `run`, the `check`
command, the trace file and the live stream. It is written from
[SPEC.md](../docs/SPEC.md), [CONFORMANCE.md](../docs/CONFORMANCE.md),
[spec/network.schema.json](../spec/network.schema.json), the fixtures
themselves and [TRACE.md](../docs/TRACE.md)
([conformance-python.md](../docs/conformance-python.md) is an engine's
documentation, not engine source). The TypeScript and Python engines were used
only as black boxes: `node dist/engine/cli.js conform <dir>` and probe fixtures
of my own, written into a scratch directory outside the repository.

## M0 scope (the conformance core, closed)

- `neuron-engine conform <dir> [--only <text>]`, the command-line contract of
  CONFORMANCE 8: one line per fixture, `pass`/`fail` (first difference, two
  more indented lines)/`pending`, a last summary line, exit 0/1/2.
- The engine core of SPEC 4-11 on a virtual clock with scripted programs:
  routing, threshold and staleness, `maxHops`, level 0 and level 1 (with all of
  SPEC 3.2 and 6.1), composed neurons (sealing, scopes per call, 10.5 pools),
  human neurons (the scripted person of CONFORMANCE 5.5), budgets (10), effects
  (11), observers (9), the live stream (9.1) and `finished`.
- `check`-strength validation of network files (the errors of 2.7, needed for
  the refusal fixtures); a file failing them, or a network with an `http`
  neuron, refuses to run.

## M1 — the process layer

- `neuron-engine run <file> --inputs <jsonl> [--trace <out.json>]
  [--stream <path>] [--ping <ms>]` (SPEC 9.1's command form): the producer's
  injections at their `at` ms from time zero (the instant the last serve
  program said ready, CONFORMANCE 4.2's real analogue), the answers of the
  neurons with no outgoing links on stdout, one per line. Exit 1 on a run the
  engine refused; the streams and traces are written even in that case only
  when the run had events to report.
- The trace file (`--trace <out.json>`), format v1 per
  [TRACE.md](../docs/TRACE.md): the header (`format`, `version`, `library`,
  `engine` = `agent-neuron-rust`, the `demo` name, `neurons`, `links`) and the
  events, one value per line inside. The `run` self-checks after writing: the
  events read back from the file are the run's, and (with a stream) the
  stream's lines are the same events in the same order — P2's gate.
- The live stream (`--stream <path>`): a file or a pipe fed one event per line
  as they happen, never holding the run up. A pipe with no reader is off,
  with one message on stderr (9.1), and the run is untouched by it.
- `neuron-engine check <file>` (SPEC 2.7): the closed schema's errors
  (duplicate ids, a link naming an unknown id with the nearest-id suggestion,
  a duplicated link, a link into a source, the composed-file rules —
  missing/invalid file, entry/exit pairing and existence, a source inside, an
  exit with outgoing links, effect inside a composed file), a cycle in which
  no neuron has `maxHops`, the budget rules (whole numbers 0..2^53-1, a cost
  unit the file's budget lacks, an empty `budget` or `cost`, a budget
  demanding a cost on every non-source, composed included), and the warnings
  (a cost above the file's ceiling, a composed file whose neurons cost a unit
  the declaring cost does not name — 10.5 —, an executable not findable on
  `PATH`, an `http` neuron: a warning here, a refusal to run). Exit 1 on any
  error, 0 with none, 2 for usage and for a file that cannot be read. Every
  composed file is checked, relative to the file that declares the neuron
  (2.4); a tree that loops back on itself is visited once.
- The real host (`host.rs`), std library plus `kill`, `pipe`, `read`,
  `signal` and `write` (extern "C", POSIX): one process per call in its own
  process group (`process_group(0)`), one JSON value to stdin then closed
  (never a blocking step: the write happens in its own thread), the 1 MiB cap
  on a level 0 output and on a level 1 line, one long-lived process per serve
  neuron with slots, the `re` counter, queues, pings and the restart backoff
  of SPEC 3.2 (`f` counts: not ready, a life under 1000 ms after ready, or
  killed as hung; a pong or a life of at least 1000 ms that does not end in a
  hang clears it; 5 is failure). SIGTERM to a group, SIGKILL 2 s later, with
  the `term`/`kill` notes of 6.3; the notes of a cancelled level 0 call land
  at its timeout; the engine tears itself (SIGINT/SIGTERM to the CLI) down
  through a self-pipe that kills every registered group.

### How to run everything

```sh
cargo fmt --check                                   # max_width 140 (rustfmt.toml)
cargo clippy --all-targets -- -D warnings           # zero
cargo test                                          # 39: real 27, check 11, probe 1
cargo run --manifest-path rust-engine/Cargo.toml -- conform spec/fixtures   # 201, 0, 0
cargo run --manifest-path rust-engine/Cargo.toml -- conform py/probes       # 10, 0, 0
target/debug/neuron-engine run spec/examples/alerts.network.json --inputs spec/examples/alerts.inputs.jsonl --trace alert.json
target/debug/neuron-engine run spec/examples/alerts-python-serve.network.json --inputs spec/examples/alerts.inputs.jsonl
target/debug/neuron-engine run spec/examples/alerts-budget.network.json --inputs spec/examples/alerts.inputs.jsonl
target/debug/neuron-engine check <file>
```

Results when M1 closed, re-run the commands above to check: `spec/fixtures`
201 passed, 0 failed, 0 pending (the M0 numbers, identical over 5 consecutive
runs, still good); `py/probes` 10 passed, 0 failed; `cargo test` 27 real + 11
check + 1 probe, 0 failed; `cargo fmt --check` clean at `max_width = 140`;
`cargo clippy --all-targets -- -D warnings` clean. The three alert smoke runs
work end to end with written traces and self-checks, and their event counts on
this machine are machine-dependent: the storm's programs are real processes,
and a scheduler that coalesces small timer sleeps (this machine: a 10 ms sleep
at ~45 ms) stretches process starts, so the count of `note term` findings
moves by a handful of events per run (313-319 for the plain alerts network
with the same 40 traces, no errors and no timeouts in any run). The
conformance suites are the deterministic gate; the smoke runs are the
end-to-end one.

## What is deliberately NOT in M1

- The `http://` and human *page* surfaces (M2): an `http` neuron refuses to
  run; a `human` neuron runs against the scripted person of CONFORMANCE 5.5
  only, so `run` refuses a file with one ("no desk for a person").
- The ids-in-trace header of the trace file format, and any trace the `conform`
  command writes (the trace is the `run` command's surface).
- Windows: the process layer is POSIX. The engine's platform list in
  `signal_name` covers darwin and linux.

## Design in one page

`main.rs` (the CLI: discovery, argument, output) and `runner.rs` (the runner of
CONFORMANCE 3-6: the live-stream consumer, the two-run observer rule, message
prefixes, harness and refusal handling) drive `engine.rs`. `engine.rs` is the
whole simulation: one `BinaryHeap` of wake-ups ordered by `(t, set-order)`, so
SPEC 6.2 rule 1 and the fixed points of CONFORMANCE 4.1 fall out of the arm
order; a rule-7 reply wake-up is simply armed before its call's timeout. Every
difference between the virtual host and the real one sits in one `is_real()`
branch. Signals, works (held inputs, queued and running calls), level-0
groups, level-1 processes with their slots and queues, the staleness scopes
(the outer instance is one scope; a composed neuron's inside is a scope per
call) and the per-trace budget pools live in plain vectors and maps. Events
are built as `json::Value`s whose ids are opaque strings; the runner
normalises both event lists by first appearance (CONFORMANCE 3.2) before
comparing. The inside of a composed neuron is simulated honestly but its
events go to a discard buffer (sealing, SPEC 2.6). The real host's loop pumps
a queue of host events stamped when the reader read them, so an event's time
is never later than its own timeout. No external crates: `json.rs` is a
std-only JSON parser/writer with `deep_eq` (`covers` for observer rules),
which is what a `serde`/`serde_json` pair would have done, at about 500 lines
over what `serde_json` costs — this is the honest price of a dependency-free
engine, not golf. `net.rs`'s validators produce positioned findings (a small
scanner walks the raw text's array spans and finds each rule's anchor key), so
`check` reports `file:line:column` where it can, and `load` flattens the same
rules to the strings the runner carries. A rule about a whole file whose key
is absent sits at `1:1`; the columns count characters.

Places where the fixtures pin behaviour beyond the literal text, and how this
engine reads it (all checked black-box against the TypeScript engine or taken
from fixtures):

- The restart delay counts from the failure's detection, and a due restart is
  skipped silently when a kill grace is the only thing keeping the run open
  (`failures/l1-process-exits-and-leaves-a-survivor`; a probe against the
  TypeScript engine agreed that the delay does not wait for the grace).
- A pong deadline keeps the run open only while other work does: the run-over
  rule does not count it (probe: the TypeScript engine ends the run with a
  pending pong deadline; `CONFORMANCE` 4.1's list of things that keep a run
  open does not include it).
- A cut level 1 call's `commit` lands when its slot frees (probe against the
  TypeScript engine: commit at the reply that frees the slot, not at the cut).
- A cancelled level 1 call's slot and a kill grace keep the run open; restart
  delays and idle pings do not (`runend/` fixtures).
- The `budget:` message is `budget: needs <cost> <unit>, <pool> left`
  (CONFORMANCE 3.4's fixtures pin it); with several short units the first in
  the neuron's cost order is named (no fixture pins the case).
- The trace's `finished` checks run after the stale drops a new signal caused
  and before its own delivery effects, and again after a reply step's queue
  drain (`budget/a-cut-call-commits-its-reservation`,
  `budget/a-queued-level-1-call-checks-when-its-slot-frees`, a probe of mine).
- A level 0 cancelled call's group (and the kill grace) keep the run open and
  delay the call's `commit` and the trace's `finished` to the kill
  (`budget/a-cut-call-that-ignores-term-commits-when-killed`).

And the real host's own choices (M1; the SPEC text each stands on is named):

- A call settles 100 ms after its process exits, whatever the exit was
  (decision 0001, Appendix C): the outcome is read whole — an over-long
  output's kill can be seen at all, a survivor's last bytes land in the head.
  The level 1 down-notice rides the same settle, so an exit's last lines are
  read before the crash is reported.
- A program whose group ignores SIGTERM keeps the run open past its term
  (6.2 rule 6): the engine probes the group with `kill(-pgid, 0)` — the
  POSIX way to ask whether a member is still there — and the kill comes
  2000 ms after the term (6.3). The probe is honest and imperfect the same
  way: a member the engine has not reaped yet, or a reparented zombie for a
  moment, reads as a live member, because for an engine whose monitors reap
  what they see that is what "a member" is.
- A command's first argument containing `/` is resolved by the engine against
  the neuron's working directory before the exec, because on darwin the
  spawn's own chdir happens around it (races the exec); a bare name goes to
  `PATH` (2.3). The run command resolves the declaring file's directory
  against the caller's cwd when the file is named bare (2.4).
- The end-of-run teardown polls: stdin is closed everywhere, then the
  engine waits up to 2 s (a serve program's shutdown window, 3.2), SIGTERMs
  what still lives, waits 2 s more, SIGKILLs, and returns when the groups
  are gone or after a bounded last wait. A group that is gone is not waited
  for; a leftover the engine never reported on is stopped silently
  (decision 0001), without the notes 6.3 gives a cancelled call.
- The events of the real host are stamped when the reader read them, so the
  engine's virtual clock and the wall clock cannot disagree about an event's
  order at its own timeout instant (6.2 rule 7).
- `check` positions a rule on the quoted key it names, found in the raw
  text by span; a rule about the whole file whose key is absent (a required
  `neurons`, the cycle, the un-costed budget demand) sits at `1:1`, and the
  position of a warning about a composed file's costs is the declaring
  neuron's `cost` (10.5 names the declaring cost as the thing lacking).
- A missing composed file is an error of the declaring file, at the
  `network` key that names it; the composed file's own errors, when it can
  be read, are its own (each at its own file's positions).

## Size

Lines as committed (`wc -l`), formatted at `max_width = 140`:

| File | Lines |
|---|---|
| `src/json.rs` | 415 |
| `src/net.rs` | 1083 |
| `src/check.rs` | 252 |
| `src/fixture.rs` | 400 |
| `src/engine.rs` | 3784 |
| `src/host.rs` | 902 |
| `src/run.rs` | 98 |
| `src/trace.rs` | 204 |
| `src/runner.rs` | 539 |
| `src/main.rs` | 246 |
| `src/lib.rs` | 11 |
| source total | 7934 |
| `tests/real.rs` | 784 |
| `tests/check.rs` | 493 |

For scale: the TypeScript reference engine (`src/engine/`) is 1353 by
`wc -l` (decision 0005's table). This crate is larger for the same honest
reasons as at M0 — the std-only JSON layer and the conformance runner — plus
the M1 process layer (host, run, trace, check) that the reference engine
gets "for free" from Node. None of this is golf.
