# agent-neuron engine, second implementation (Rust, M0)

This crate is the Rust engine of [`agent-neuron`](../), milestone M0: a sans-IO
core that runs every agent-neuron conformance fixture on the virtual host. It is
written from [SPEC.md](../docs/SPEC.md), [CONFORMANCE.md](../docs/CONFORMANCE.md),
[spec/network.schema.json](../spec/network.schema.json), the fixtures themselves
and [conformance-python.md](../docs/conformance-python.md) (an engine's
documentation, not engine source). The TypeScript and Python engines were used
only as black boxes: `node dist/engine/cli.js conform <dir>` with probe
fixtures of my own, written into a scratch directory outside the repository.

## M0 scope

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

## Go/no-go criteria (from the branch plan)

1. `cargo run --manifest-path rust-engine/Cargo.toml -- conform spec/fixtures`
   passes every non-pending fixture, and passes identically on 5 consecutive
   runs.
2. `cargo run --manifest-path rust-engine/Cargo.toml -- conform py/probes`
   passes all 10 probes (the probes are inputs, not copied fixtures).
3. `cargo fmt --check` clean (max_width 140) and
   `cargo clippy --all-targets -- -D warnings` clean.
4. No golf: readable code, real names, wc per file in the table below.
5. This README documents the scope, the criteria above, and what is not in M0.

Results when M0 closed, measured on this machine (re-run the commands above to
check): `spec/fixtures` 201 passed, 0 failed, 0 pending, identical over 5
consecutive runs; `py/probes` 10 passed, 0 failed; `cargo fmt --check` clean at
`max_width = 140`; `cargo clippy --all-targets -- -D warnings` clean.

## What is deliberately NOT in M0

- A real process layer: no pipes, `setsid`, signals, environments, byte
  framing. The 1 MiB and junk outcomes are scripted, as the fixtures do.
- A `check` command (validation runs internally, errors only — warnings would
  need line/column reporting that only makes sense with a file surface), a
  `run` command, a trace file (`TRACE.md`), `run --stream <path>`.
- The `http://` and human *page* surfaces: an `http` neuron refuses to run; a
  `human` neuron runs against the scripted person of CONFORMANCE 5.5 only.
- The `engine` trace header field, and the ids-in-trace header of the trace
  file format.

## Design in one page

`main.rs` (the CLI: discovery, argument, output) and `runner.rs` (the runner of
CONFORMANCE 3-6: the live-stream consumer, the two-run observer rule, message
prefixes, harness and refusal handling) drive `engine.rs`. `engine.rs` is the
whole simulation: one `BinaryHeap` of wake-ups ordered by `(t, set-order)`, so
SPEC 6.2 rule 1 and the fixed points of CONFORMANCE 4.1 fall out of the arm
order; a rule-7 reply wake-up is simply armed before its call's timeout.
Signals, works (held inputs, queued and running calls), level-0 groups, level-1
processes with their slots and queues, the staleness scopes (the outer
instance is one scope; a composed neuron's inside is a scope per call) and the
per-trace budget pools live in plain vectors and maps. Events are built as
`json::Value`s whose ids are opaque strings; the runner normalises both event
lists by first appearance (CONFORMANCE 3.2) before comparing. The inside of a
composed neuron is simulated honestly but its events go to a discard buffer
(sealing, SPEC 2.6). No external crates: `json.rs` is a std-only JSON
parser/writer with `deep_eq` (`covers` for observer rules), which is what a
`serde`/`serde_json` pair would have done, at about 500 lines over what
`serde_json` costs — this is the honest price of a dependency-free engine, not
golf.

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

## Size

Lines as committed (`wc -l`), formatted at `max_width = 140`:

| File | Lines |
|---|---|
| `src/json.rs` | 415 |
| `src/net.rs` | 633 |
| `src/fixture.rs` | 394 |
| `src/engine.rs` | 3068 |
| `src/runner.rs` | 527 |
| `src/main.rs` | 73 |
| `src/lib.rs` | 6 |
| total | 5116 |

For scale: the TypeScript reference engine (`src/engine/`) is 1353 by
`wc -l` (decision 0005's table). This crate is larger for two honest reasons:
the std-only JSON layer (`json.rs`, 415 lines — what `serde_json` would have
been, and the price of a dependency-free build), and a runner (`runner.rs`,
527 lines) that is part of the conformance contract a new engine must write.
The engine itself, without JSON, is 2578 lines; the 0005 experiment, golfed,
came to 1292 (about 2200-2500 at rustfmt's shape) with its process layer
unfinished. None of this is golf.

