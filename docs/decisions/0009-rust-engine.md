# Decision record 0009: the Rust engine, the second complete engine (2026-11-07)

**Status: decided and built.** Kevin chose the direction on 2026-11-07 and the whole engine was built
the same day: milestone M0 was committed at 14:57 (`c0cb3ae`), M3 closed at 16:49 (`652c525`), and the
branch merged to `main` at 17:35 (`39ae56f`). The desk's host surface merged at 18:11 (`5012ae5`) and
the trace header's `ids` field at 19:22 (`887300f`). Every gate below was rerun on merged `main` on
2026-11-07, most of them for this record.

## The decision

The engine of [SPEC](../SPEC.md) is built a second time, in Rust, and lives on `main` as the family's
second complete engine, under the standing rules [decision 0005](0005-rust-engine-experiment.md) set
down when it stopped the first attempt:

- **The fixtures are the contract** (SPEC section 8, [CONFORMANCE](../CONFORMANCE.md) section 8): a
  second engine starts from the fixtures and the text alone.
- **The other engines are black boxes**, run through their CLIs against probe fixtures of their own,
  written into a scratch directory outside the repository. Their source is never read.
- **A written formatting rule**, rustfmt at `max_width = 140` (`rust-engine/rustfmt.toml`), so a size
  criterion means something, with `cargo clippy --all-targets -- -D warnings` as its companion.
- **Standard library only.** `rust-engine/Cargo.lock` has one package and no dependencies, so the
  build fetches nothing.

**Why now.** 0005's own revisit trigger — "the engine must be embedded in another runtime that cannot
host Node" — arrived as the maintainer's voice tool (the pipeline of [decision 0008](0008-human-neurons-on-a-page.md)).
Its install story was the argument: it needed the engine beside it, and a clone-and-install of a Node
library is a worse install than one built binary. The tool's own engine route now documents the story
in one sentence: clone this public repo, run `cargo build --release` in its `rust-engine/`, and point
at the built folder (or put `neuron-engine` on `PATH`). The other two triggers of 0005 — a sealed
composed neuron as WebAssembly, and the scale of long-running networks — stay open.

**Scope as four milestone gates**, each closed before the next was started. "Complete" is the bar the
milestones were cut to:

## What "complete" means

| | TypeScript reference (`src/engine/`) | Python (`py/agent_neuron/`) | Rust (this record) |
|---|---|---|---|
| Built from | the text and the fixtures | the text and the fixtures | the text and the fixtures |
| Conformance | 201 fixtures | 201 fixtures, and it wrote the 10 probes in `py/probes` | 201 fixtures + the 10 probes, identical over 5 consecutive runs |
| Real processes | yes (SPEC 3, 6) | none — scripted programs on the virtual clock only | yes |
| `http` neurons | refused to run (SPEC Appendix C's reference wording) | refused | yes, and `https` refused honestly at plant (no TLS in a std-only crate) |
| `human` neurons | the page of Appendix D (decision 0008) | refused | yes, the desk of Appendix D with 0008's security rules |
| MCP | none | none | `neuron-engine mcp`, a server (M3) |

The Python engine is a protocol test (it asks whether the specification is a protocol and not just the
TypeScript code, per [README](../../README.md)), not a complete engine: it has no process layer at
all. The Rust engine is the second engine that runs a real network end to end — and the first that
runs `http`.

## What was built

| Milestone | Scope as closed | Commits | Gate at close |
|---|---|---|---|
| M0 | The sans-IO conformance core on the virtual host: `conform` per CONFORMANCE 8, `check`-strength validation, the scripted person (CONFORMANCE 5.5) | `c0cb3ae`, hygiene in `455196f` | 201/0/0 over 5 consecutive runs; 10/0/0 on `py/probes` |
| M1 | The real process layer: `run` (`--inputs`, `--trace`, `--stream`), `check` with positions, the POSIX host (process groups, pings, restarts, SIGTERM then SIGKILL with the 6.3 notes) | `77d1c9f`, `cafe620`, `9ec1757`, `09fa78f` | 27 real + 11 check tests; the three alert smokes end to end |
| M2 | Real `http` neurons and the desk: a std-only HTTP/1.1 client, the staleness/timeout cancel of a socket, the page with 0008's rules | `732fb84` | 11 http + 11 desk tests; both conformance suites unchanged |
| M3 | The MCP server: stdio JSON-RPC on the hand-rolled JSON layer, five tools, four embedded `spec://` resources, `run://` jobs and subscriptions | `652c525`, `9b5dcb7` | 14 mcp tests (15 after the trace work below) |

The whole milestone branch merged to `main` as `39ae56f` — 47 files, 15,185 lines with tests — and the
design, gates and size tables live in [rust-engine/README.md](../../rust-engine/README.md), which is
this crate's record of record. Two things landed on `main` after the merge:

- **The desk's host surface** (`5012ae5`): `GET /state`'s item carries the ask whole (`payload`, and
  `t`, the run's millisecond at the ask), and every accepted answer lands as one
  `desk answer: {item, kind, text}` line on the run's stderr at the moment of its POST — what a host
  recording a run needs, because the desk closes with the run (a `/state` poll cannot win the last
  answer, whose settle ends the run). The page's own Appendix D fields are unchanged; these ride
  beside them. Approve's rule is one shared function, read the same by the desk and by the engine's
  settle.
- **The trace header's `ids`** (`7b0b19f`, merged `887300f`): `{"signal": "run-numbered",
  "trace": "run-numbered"}`, written on every trace this engine writes by the one trace builder both
  trace routes share, read back whole by `run`'s self-check and carried by `read_trace` — while
  [TRACE.md](../TRACE.md)'s rule stands unchanged: ids are opaque strings, compared, never parsed. A
  reader without the field still reads the file.

## How it was built

Delegated, as an engine-sized build is here: an agent built it on `feat/rust-engine` against a brief
that listed the operative documents in order, the black-box bans, the commit discipline and the finish
criteria; the merge and feature commits carry the builder's co-author trailer (`Co-Authored-By: Hermes
(agent)`), and the lead reran the gates on the merged tree before and after each `main` merge — the
branch state verified last is never the state that ships. Ship discipline per
[docs/agents/feature-loop.md](../agents/feature-loop.md): the merges to `main` carry the maintainer's
go.

## Where the first attempt's misses stand now

Decision 0005 refused the first Rust engine on six criteria; the causes, against this engine:

| 0005's cause | Now |
|---|---|
| Criterion 1 — conform identical over 5 runs | met: 201/0/0 and 10/0/0, identical over 5 consecutive runs (rerun for this record) |
| Criterion 2 — the alert storm with real processes | met: `tests/real.rs` 27; the three alert smokes ran exit 0 today with written, self-checked traces |
| Criterion 3 — a third-party serve program works unchanged | met: `spec/examples/neurons/hypothesis_serve.py` served through `alerts-python-serve` today (exit 0) |
| Criterion 4 — level 1 p99 under 5 ms | not re-measured for this record (the experiment measured 1 ms; nothing in the design regressed it) |
| Criterion 5 — process hygiene (SPEC 3.1, 6.3) | met: SIGTERM to the group, SIGKILL 2 s later if anything remains, with the `term`/`kill` notes of 6.3 |
| Criterion 6 — size, in spirit (not only `wc -l`) | met: `max_width = 140` enforced by rustfmt, `cargo fmt --check` clean, clippy `-D warnings` clean, named code throughout |
| The old bugs (the `kill(-0)` window, the 64 KiB stderr stop, `check` panics, unbounded channels) | moot: the experiment's code is gone from every ref (`git cat-file -t e5a44d3` fails on this clone); this engine started from the text again |

Size today (`wc -l`): sources 11,246 + tests 3,377 = 14,623, against the TypeScript reference engine's
1,353 as committed (0005's table). The excess is the honest price of the standing rules: a std-only
JSON layer (`json.rs`, 415), the conformance runner, the process layer, and a std-only MCP server
(`mcp.rs`, 1,335 — the whole client side of the protocol an SDK would give a Node or Python engine).
None of it is golf; that was the rule.

## The gates on merged `main` (2026-11-07)

| Gate | Result | Run by |
|---|---|---|
| `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` | clean | this record's author, today |
| `cargo test` | 78 passed, 0 failed — unit 2, main 0, `check` 11, `desk` 12, `http` 11, `mcp` 15, `real` 27 | this record's author, today |
| `neuron-engine conform spec/fixtures` | 201 passed, 0 failed, 0 pending, identical over 5 consecutive runs | this record's author, today |
| `neuron-engine conform py/probes` | 10 passed, 0 failed, 0 pending, all 5 rounds | this record's author, today |
| The three alert smokes (`alerts`, `alerts-python-serve`, `alerts-budget`) | exit 0, traces written and self-checked (344/346/614 lines; event counts move a few with the machine's load, as the README says) | this record's author, today |
| Vitest (the TypeScript side) | 256 passed, 16 files — the lead's gate on merged `main` today; rerun for this record: 256/256 twice, with one timing flake (255/256, clean on the rerun) between them | this record's author and the lead, today |
| `cargo build` | clean, fetching nothing (`Cargo.lock`: one package) | this record's author, today |

The standing gate recipe is [rust-engine/README.md](../../rust-engine/README.md) ("How to run
everything"); nothing in this record changes it.

## What the direction bought

- **The engine is a server**: `neuron-engine mcp` speaks JSON-RPC 2.0 over stdio with five tools —
  `check_network` (the `check` command's whole surface), `run_network` and `start_run`/`poll_run` (a
  run as a job, with live `run://{id}` resources and per-second updated notifications), `read_trace`
  — and four `spec://` resources (`SPEC.md`, `CONFORMANCE.md`, `TRACE.md`, `network.schema.json`)
  embedded at build, byte for byte against the repo's text (equality is a test).
- **Deliberately unexposed**, enforced rather than documented: `conform` and the fixtures (a
  conformance runner is not a client surface); a network with `human` neurons (refused at plant — the
  desk is the person's surface, and an MCP result never holds that page); `https://` (refused, the
  wording naming the cause: no TLS in a std-only crate).
- **The voice tool runs the binary**, not a Node package (its main `39e7dd8`, the same day): it
  resolves the built binary, spawns `neuron-engine run <network> --inputs --trace`, streams the
  engine's stderr, and drives the desk over HTTP — `person(item, answer)` decides, and the recording
  rides the desk's `desk answer:` stderr lines. The TypeScript engine stays the reference for
  conformance.

## Deliberately out, and open

- **Not this engine's**: Windows (POSIX only; the platform list covers darwin and linux), `https`
  (reopens when a dependency policy ever exists), npm distribution (the dependency is a built binary;
  a packaging question, not a blocker).
- **The protocol's open questions wait for the next SPEC pass**: durable holds, who may answer,
  notifications, run quiescence — collected as issue #30 on the public repo
  (`github.com/overfocus/agent-neuron`).

## Revisit this record when

- a sealed composed neuron should run as WebAssembly — 0005's standing trigger, with the sans-IO core
  as the natural unit;
- a host needs Windows, or a dependency policy makes in-crate TLS conceivable rather than refused;
- the scale of a long-running network matters — the one 0005 trigger still untested;
- or a third engine starts — it starts from these fixtures too, and from this README's gate recipe.
