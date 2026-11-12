# Contributing to agent-neuron

agent-neuron is an experiment: a universal wrapper (a "neuron") for any
program, and a network file format that wires neurons together. This file
says how to build it, how to test it, and the rules that keep the
specification and the code in step. Read the [README](README.md) first for
what the project is.

The repository holds a contract (the [specification](docs/SPEC.md), the schemas
and the fixtures) and one engine that runs it, in Rust. Until
[decision 0023](docs/decisions/0023-one-engine.md) it also held a TypeScript
library, a TypeScript reference engine and a Python engine; the commit before
that decision's build has them.

## Set-up

- A Rust toolchain (`cargo` on your PATH, for example from rustup). The engine
  uses the standard library only, so a build fetches nothing.
- Node 22 or later (`.nvmrc`) for `npm run` and for the demos, whose neuron
  programs and launchers are small Node scripts. There is nothing to install:
  `package.json` has no dependencies.
- Python 3 is optional: two variants of the alert storm and some of the
  engine's tests use it.

```sh
npm run check   # fmt, clippy, the engine's tests, a release build, the fixtures and the probes
```

`npm run check` is the gate: run it before you open a pull request.

## Commands

```sh
npm run check          # the whole gate, in this order:
npm run engine:fmt     #   cargo fmt --check (max_width 140, rust-engine/rustfmt.toml)
npm run engine:clippy  #   cargo clippy --release --all-targets -- -D warnings
npm run engine:test    #   cargo test --release, one test at a time
npm run conform        #   builds, then neuron-engine conform spec/fixtures and conform spec/probes
npm run engine:build   # cargo build --release → rust-engine/target/release/neuron-engine
npm run demo:network   # builds, then runs the alert storm from its network file (about 7 s); `-- <network file>` for another
npm run demo:human     # builds, then runs a network with a human neuron: open the address it prints on stderr and answer
npm run demo:team      # the team-decisions demo (builds a debug engine itself); `-- --headless` and `-- --headless --tie` need no browser
npm run demo:p2p       # builds, then four peer engines and a driver on this machine
```

The engine's own commands, with `neuron-engine` standing for
`rust-engine/target/release/neuron-engine`:

```sh
neuron-engine check <file>                 # validates a network file (schema and SPEC 2.7 findings, with line and column)
neuron-engine run <file> --inputs <jsonl | -> [--trace out.json] [--stream path] [--result path | -]
                                           # runs it; prints the answers of neurons with no outgoing links
neuron-engine conform spec/fixtures [--only <text>]   # pass/fail/pending per fixture, non-zero exit on a failure (docs/CONFORMANCE.md)
neuron-engine mcp                          # the MCP server on stdio
neuron-engine peer --listen <host:port> --token <hex>   # a peer engine
```

Every flag is in [rust-engine/README.md](rust-engine/README.md).

## Layout

| Area | Where | Tests |
|---|---|---|
| The specification, version 0.1 ([docs/SPEC.md](docs/SPEC.md)), its decisions (`docs/decisions/`) and the closed schemas (`spec/network.schema.json`, `spec/neuron.schema.json`) | `docs/`, `spec/` | the fixtures; `rust-engine/tests/check.rs` (`check` over the schema's rules and the examples) |
| The conformance fixtures, SPEC section 8 ([docs/CONFORMANCE.md](docs/CONFORMANCE.md)): one directory each, network file, scripted inputs and replies, expected events | `spec/fixtures/` | `neuron-engine conform spec/fixtures` |
| The probes: ten more fixtures, written while the Python engine was built from the text, kept apart from the contract | `spec/probes/` | `neuron-engine conform spec/probes` |
| The engine: `engine.rs` (the simulation: routing, threshold, staleness, levels 0 and 1, composed neurons, budgets, effects), `runner.rs` and `fixture.rs` (the conformance runner), `host.rs` and `run.rs` (real processes, `run`), `net.rs` and `check.rs` (loading and validating), `settings.rs`, `trace.rs`, `result.rs`, `http.rs`, `desk.rs` (the page for `human` neurons), `mcp.rs`, `peers.rs`, `confine.rs` and `proxy.rs`, `json.rs`, `main.rs` (the CLI) | `rust-engine/src/` | `rust-engine/tests/` (real processes, ports and pages; helper programs in `tests/progs/`) |
| Example networks and their neuron programs: the alert storm and its variants, `diagnosis` (composed), `notify` (an effect) | `spec/examples/`, with the storm's shell detectors and data in `examples/alerts/` | run by hand, and by `npm run demo:network` |
| The demos (list in [docs/DEMOS.md](docs/DEMOS.md)) | `examples/network.mjs`, `examples/human/`, `examples/team/` with `examples/team-demo.mjs`, `examples/p2p/` | none: run by hand |
| The trace format ([docs/TRACE.md](docs/TRACE.md)): the replay module and five recordings of the deleted library's demos | `examples/trace/replay.mjs`, `traces/` | none |

## Principles

- **The neuron interface is most valuable the simpler it is.** Every new
  capability must keep a neuron's interface small: prefer an option on what
  exists over a new method, a field over a new type, and a default that does
  nothing over a setting everyone must learn. A feature that makes the
  simple case harder to write is the wrong shape.
- **The network file says how neurons connect, never how one decides.** JSON
  holds literals and references only; if a key has to read a payload to mean
  anything, it does not belong in the file (gates, filters, transforms and
  prompt templates are programs, and a filter is a small neuron). Values are
  literals: no `${}`, no conditions, no expressions. The schema
  (`spec/network.schema.json`) is closed. (One exception, decision 0012: the
  ground's deposit rule names the payload fields it records — the engine's own
  bookkeeping, as `replaces` keys the producer's inputs.) **Field budget**: a network file has
  10 top-level fields (`budget` is the ninth, `ground` the tenth — decision 0012), a neuron has `id`, `description`, one kind field and at
  most 11 options (`threshold`, `within`, `maxHops`, `timeout`, `env`, `cwd`, `cost`, `effect`, `may`, `maxInflight`, `each`; `effect` was
  the eighth and the last planned, decision 0016 reopened the budget for `may`, the ninth, decision 0021 spent one more on `maxInflight`, the tenth, and decision 0022 extended it for `each`, the eleventh), a link has `from` and `to`. A neuron has one kind field of seven: `neuron`, the seventh, names the neuron's own file (`spec/neuron.schema.json`, decision 0019), which holds what is true of the program wherever it runs, while the entry holds what is true only of this network. Test every proposed field with one question:
  *does it say how neurons connect, or how one decides?* Only the first kind
  gets in, and only with a decision record in `docs/decisions/` that says what
  it costs against the budget. Descriptive fields are a second, closed class:
  for people and tools, acted on by no engine. There are two, `description` and
  `at` (decision 0011); another needs its own record. A change to the file, the protocol or the
  routing is a change to `docs/SPEC.md`, the schema and the fixtures together.
- Every feature that adds or changes visible behaviour adds or updates a demo
  and its row in `docs/DEMOS.md`.
- The engine takes no dependency on one model provider: an agent inside a
  neuron is whatever program the network file names. It takes no crate either
  (standard library only), so a build never needs the network.
- Never put a real name, email or anything personal in a request to an
  outside service, a fixture or a log.

## Traces and privacy

Traces ([docs/TRACE.md](docs/TRACE.md)) record real runs, and the ones in
`traces/` are committed, so they are public. A committed trace must carry no
paths, emails, hostnames or user names, and fixtures and examples use made-up
agents and data. The test that checked the committed traces for these went
with the TypeScript library (decision 0023), so check a trace by reading it
before you commit one.

## The specification and the fixtures

The fixtures in `spec/fixtures/` are the contract. Any other engine is judged
by them.

- A change to the network file, the protocol or the routing is a change to
  [docs/SPEC.md](docs/SPEC.md), the schema (`spec/network.schema.json`) and the
  fixtures, in one commit.
- A fixture changes only in the same commit as the SPEC text it pins. Never edit
  an `expect` to make the engine pass.
- Fixtures are engine-independent: no real time, no ties the spec does not
  decide (a ping due at the instant of an injection), invented data.
- **One engine checks the fixtures now.** Until decision 0023 three engines had
  to agree with each hand-written `expect`, and a mutant test (a hundred
  deliberately broken engines that each had to fail a fixture) measured whether
  the fixtures pin the rules. Both are gone, so a fixture and the engine can be
  wrong together and nothing here notices: write an `expect` by hand from the
  SPEC text, and review it as such.
- A new engine rule gets a fixture.
- A new field in the network file needs a decision record in `docs/decisions/`
  that says what it costs against the field budget.
- Run one fixture with `neuron-engine conform spec/fixtures --only <text>`.
  Format and command-line contract: [docs/CONFORMANCE.md](docs/CONFORMANCE.md).
- `docs/SPEC.md`, `docs/CONFORMANCE.md`, `docs/TRACE.md` and
  `spec/network.schema.json` are compiled into the engine (the MCP server
  serves them as `spec://` resources), so editing one changes the binary and
  `rust-engine/tests/mcp.rs` compares them.

## Things that are easy to get wrong

- Run cargo for the engine's tests one test at a time. `rust-engine/.cargo/config.toml`
  sets `RUST_TEST_THREADS=1`, but cargo reads that file only when it is started
  inside `rust-engine/`; `npm run engine:test` sets the variable itself for that
  reason. The tests start real processes and listen on real ports, with timing
  margins: on a loaded machine one can fail, so run a failing test again alone
  before you believe it.
- The engine is POSIX only (process groups): macOS or Linux. Confinement
  (`run --confine`) is macOS only. `https` is refused (no TLS in a std-only crate).
- A program gets only `PATH` and the names in its `env`, so a Python neuron
  needs `python3` on `PATH`.
- `engine run` reads inputs with `at` relative to the start of the run, and the
  clock starts when every `serve` neuron is ready.
- `spec/examples/alerts.network.json` names its shell detectors and their data
  in `examples/alerts/` by relative path: move one and move the other.
- The alert storm runs real processes on the real clock, so its counts (flags,
  calls, cut) move with machine load; only the seven diagnoses are stable.
- `examples/team-demo.mjs` builds and runs the debug binary
  (`rust-engine/target/debug/neuron-engine`) and rebuilds it when a source file
  is newer; the other demos use the release binary.
- `--stream -` is not standard output: it writes a file named `-`.
- A trace ends with a `finished` event per trace (SPEC 7.2); fixtures compare it only with `"finished": true`, as notes
  need `"notes": true`, so a fixture about something else does not change when the trace gains an event.
- The conformance runner checks the live stream (SPEC 9.1) against the events of every run it makes, so an engine
  change that reorders, drops or hands over its events fails every fixture. A neuron with `effect` is never cut by
  staleness and fails as `unknown`, not `error` (SPEC 11); a composed neuron states `effect` exactly when its file has an effect neuron (11.5).
- Other projects may copy `traces/` and `examples/trace/replay.mjs`, so a breaking format change bumps `version`
  (rules in `docs/TRACE.md`).

## Known limits

Open problems are GitHub issues.
