# agent-neuron

An experimental agent graph. An **agent-neuron** is a universal wrapper, or
container, that a single agent runs in. Neurons connect to each other
many-to-many: any neuron can send to and receive from any number of others,
so agents form a graph rather than a tree or a pipeline. A TypeScript (ESM, strict) library on
Node 22, tested with Vitest. There is no website and no deploy yet, only a local page that draws a demo's
network (`npm run view`). The code
lives at `github.com/overfocus/agent-neuron`, private for now.

This is a standalone library: it shares no files with any other repository.

## Commands

Node 22 (`.nvmrc`). Run `nvm use` first.

```sh
npm run typecheck   # tsc --noEmit over src/ and test/
npm test            # vitest run (test/**/*.test.ts)
npm run build       # tsc -p tsconfig.build.json → dist/
npm run rust:test   # cargo test over the rust/ workspace: catalogue, music (needs a Rust toolchain)
npm run rust:build  # cargo build --release over rust/ → rust/target/release/{catalogue,music}
npm run check       # typecheck, rust test, rust build, test, build: the gate without agent-kit
npm run demo:rust   # builds both, then runs examples/rust.mjs (tracks neuron in Rust)
npm run demo:composed # builds both, then the composed `music` neuron: keystrokes in, answers out, then its inner trace
npm run demo:loop   # builds, then runs examples/loop.mjs (a loop cut by maxHops)
npm run demo:suggest   # builds, then runs examples/suggest.mjs (the typing demo plus a threshold neuron)
npm run example     # builds, then runs examples/first.mjs (pipe a reply into stdin: echo approve | npm run example)
npm run demo:typing # builds, then runs examples/typing.mjs (send without waiting, dropping stale work)
npm run demo:alerts # builds, then runs the alert storm triage demo (about 6 s)
npm run demo:notify # builds, then runs the side-effecting notify demo (an effect not cut by a newer input, a crash recorded as unknown; about 3 s)
npm run demo:network # builds, then runs the same storm from its network file with the engine (about 7 s); `-- <network file>` for another
npm run engine -- check <file>   # builds, then validates a network file (schema and SPEC 2.7 findings, with line and column)
npm run engine -- run <file> --inputs <jsonl> [--trace out.json] [--inner out.json] [--stream path]  # runs it; prints the answers of neurons with no outgoing links; --stream writes each event as a JSON line as it happens (SPEC 9.1)
npm run engine -- conform spec/fixtures [--only <text>]  # builds, then the conformance fixtures: pass/fail/pending per fixture, non-zero exit on a failure (docs/CONFORMANCE.md)
npm run view        # builds, then serves the network view on a free 127.0.0.1 port; prints the URL and pid (stop it with kill <pid>)
npm run record -- <alerts|typing|suggest> [--threshold n] [--out file]  # builds, runs the demo, writes traces/<demo>.json (docs/TRACE.md)
npm run check:browser -- --base <url> --routes / --checks examples/view/checks.json  # needs the view running
npm run check:layout -- --base <url> --file examples/view/layout.json  # phone and desktop layout, same server
npm run verify      # the gate: typecheck, rust test, rust build, test, build, conform; writes .verify/receipt.json
npm run verify:receipt  # exit 0 only for a PASSED full verify of this clean tree
npm run ship -- --approved "<go-ahead>"  # after Kevin's go-ahead (/loop:ship)
npm run wt -- new <slug>  # a worktree on feat/<slug>; `list`, `gc [--apply]`
npm run ships -- summary  # what shipped this week
npm run issue -- add --title "..." --kind bug|debt|idea --source human --body-file <f>  # `list` shows the open ones
```

`verify`, `verify:receipt`, `ship`, `wt`, `ships` and `issue` are the agent-kit
loop plugin's scripts, run through `scripts/kit.mjs` (a copy of the kit's
template; keep it equal) and set by `agent-kit.config.json`. `check:browser` and
`check:layout` drive the installed Chrome through `playwright-core` against a
running `npm run view`; `verify` does not run them (they need a server), so run
them when `examples/view/` changes (`docs/agents/browser.md`).

There is no linter yet; `tsc` in strict mode is the static check.

## Agents working here

The loop comes from the agent-kit `loop` plugin (`/loop:feature`,
`/loop:ship`, the `loop:*` agents). `docs/agents/` holds this repo's copy of
the loop docs: `feature-loop.md`, `parallel.md`, `browser.md`. A project agent
in `.claude/agents/<name>.md` overrides the plugin's agent of the same role.

The loop's scripts and agents come from agent-kit, which is private: outside
Kevin's machine `npm run verify` cannot find it, and `npm run check` (the same
steps, without the receipt) is the gate anyone can run. It needs a Rust
toolchain (`cargo` on PATH, typically rustup's `~/.cargo/bin`), and so does
`npm test`, because `test/rust.test.ts` and `test/composed.test.ts` run the release binaries.

## Written to be public

This repo may become public as an example of the neuron concept. Treat every
tracked file and every commit message as public from the start:

- No personal paths, private project or folder names, emails, client or
  customer names, keys, or notes about Kevin's other work. Machine-specific
  settings go in gitignored files (`.claude/settings.local.json`,
  `.env.local`); estate-wide rules stay in `~/.claude/CLAUDE.md`, not here.
- Fixtures and examples use made-up agents and data.
- The same goes for GitHub issues, branch names and pull requests: they
  become public with the repo. Describe Kevin's other sites by what they are
  ("a music site"), never by name, even when an example is inspired by one.
- `issues.denylist` in `agent-kit.config.json` is tracked, so it cannot hold
  the names it would guard; keep those names out by hand and check issue text
  before filing.
- If something private is committed, stop and tell Kevin before pushing: it
  has to come out of history, not just out of the next commit.

## Deploying

Nothing deploys yet. Treat `main` as the release line anyway: never push or
merge to `main` without Kevin's go-ahead for that change; the global
`guard-main` hook asks before any push to it. `feat/` and `fix/` branches
may be pushed to `origin` as a backup: there are no preview deployments.
TODO(Kevin): whether this becomes an npm package or a deployed service.

## Areas

| Area | Code | Tests |
|---|---|---|
| Core (neuron, network, signals, adapters in `src/adapters/`) | `src/` | `test/` |
| The demos (`npm run example`, `demo:typing`, `demo:loop`, `demo:suggest`, `demo:alerts`, `demo:notify`; they build first. `demo:rust` and `demo:composed` live here too and need the Rust build; list in `docs/DEMOS.md`) | `examples/` | none: run by hand, except `demo:alerts`: `test/alerts.test.ts` |
| The network view (`npm run view`): a Node server, one page, no framework | `examples/view/` | `test/view.test.ts` (server); `examples/view/checks.json` and `layout.json` and `layout-suggest.json` (browser, by hand) |
| The specification, version 0.1 (network file, neuron contract, routing; [docs/SPEC.md](docs/SPEC.md)), its decisions (`docs/decisions/`), the closed schema and the alert storm as network files (`spec/`) | `docs/SPEC.md`, `spec/` | `test/spec.test.ts` (schema, examples, broken copies, the example programs) |
| The reference engine for network files (`npm run engine`, subpath `agent-neuron/engine`): `check.ts` (validator and `check`), `engine.ts` (routing, threshold, staleness, level 0 processes, level 1 `Server`, composed neurons, budgets, trace), `cli.ts`, `virtual.ts` (the virtual clock and scripted programs, a `Host` the engine takes instead of real time and processes), `conform.ts` (the fixture runner). Separate from the library: `src/index.ts` does not export it | `src/engine/` | `test/engine.test.ts` (the storm in JavaScript, Python and Python level 1, small networks of real processes, a scriptable level 1 neuron in `test/support/serve.mjs`), `test/spec.test.ts` (`check`) |
| Trace recorder and replay (`npm run record`): `examples/trace/` (tap, scenarios, record, replay) and the recorded files in `traces/` | `examples/trace/`, `traces/` | `test/recording.test.ts` (validity, replay, privacy of every committed trace) |
| The conformance fixtures, SPEC section 8 ([docs/CONFORMANCE.md](docs/CONFORMANCE.md)): one directory each, network file, scripted inputs and replies, expected events; `budget/` pins the v0.1 budget and the `finished` event, `effect/` the `effect` option and the `unknown` outcome, `stream/` the live stream | `spec/fixtures/` | `test/conformance.test.ts` (all pass, five identical runs, and 81 deliberately broken engines that must each fail one) |
| Rust neurons (std-only): `catalogue` (library + binary; `npm run demo:rust`) and `music`, a composed neuron with its own inner network (`npm run demo:composed`) | `rust/` | `cargo test` in each crate; `test/rust.test.ts` and `test/composed.test.ts` drive the built binaries |

## Everywhere

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
  (`spec/network.schema.json`) is closed. **Field budget**: a network file has
  9 top-level fields (`budget` is the ninth), a neuron has `id`, `description`, one kind field and at
  most 8 options (`threshold`, `within`, `maxHops`, `timeout`, `env`, `cwd`, `cost`, `effect`; `effect` is
  the eighth and the last planned), a link has `from` and `to`. Test every proposed field with one question:
  *does it say how neurons connect, or how one decides?* Only the first kind
  gets in, and only with a decision record in `docs/decisions/` that says what
  it costs against the budget. A change to the file, the protocol or the
  routing is a change to `docs/SPEC.md`, the schema and the fixtures (once they exist) together.
- Every feature that adds or changes visible behaviour adds or updates a demo
  and its row in `docs/DEMOS.md`.
- Library code has no side effects on import, and the core takes no
  dependency on one model provider: an agent inside a neuron is whatever the
  caller plugs in.
- Never put Kevin's name, email or anything personal in a request to an
  outside service, a fixture or a log.

## Things that are easy to get wrong

- Worktrees live under `.claude/worktrees/` inside the repo. `vitest.config.ts` limits
  tests to this checkout's `test/`; without it, `npm test` in one checkout runs every
  worktree's tests too.
- ESM with `NodeNext` resolution: relative imports need the `.js` extension
  (`import { x } from './x.js'`), even from `.ts` files.
- A neuron with a `threshold` holds inputs inside `Network` (`hold` in `src/network.ts`),
  not in `pending`, so `idle()` does not wait for them and their `unref`'d timers
  never keep Node alive: a script that ends right after `idle()` never sees the
  `'expired'` drop (the suggest demo sleeps past `within` to show it). `send` and `query` skip the threshold.
- A threshold neuron's running call listens to the abort of every input it acted on (`AbortSignal.any` in `run`), but reports one `onDrop` 'replaced', for the crossing input, which may not be the replaced one. `onDrop` carries no phase: `tap` (`examples/trace/tap.mjs`) adds `phase` to replaced drops by replaying the declared neurons (its second argument; without it no phase is written), so a listener that needs `phase` must go through `tap(…, neurons)`. The committed traces predate `phase` and were not re-recorded (their numbers did not change).
- `examples/alerts-demo.mjs` takes the library as an argument (`runAlerts(lib)`), so the demo passes `dist/` and `test/alerts.test.ts` passes `src/`, with no build needed first. Its test asserts invariants only: the exact numbers (alerts for one service 150 ms apart or 900+ ms apart; a shell detector must finish in about 150 ms) hold only on an idle machine. `examples/alerts-demo.d.mts` types it for the test.
- Replay and `tap` do not model `send` into a threshold neuron (the library does not report `via`), so `phase` can mislabel a held input there.
- The page's "waiting" count follows `emit`'s threshold semantics only: it does not model `send` (which skips the threshold) or an input a neuron's `maxHops` refuses.
- The view's server listens on 127.0.0.1 only, on a free port (never a fixed
  one), and answers only that host name and refuses cross-site requests to `/events` (`Sec-Fetch-Site`, `Origin`). Stop it by the pid it prints, never
  with `killall node`. Closing the page does not stop its run. The page makes
  no outside requests (a CSP of `default-src 'self'; img-src data:` enforces it). The server
  imports `dist/` only when typing or suggest runs (so `test/view.test.ts`
  needs no build); the neurons and their thresholds (so which ones show "waiting") are declared in
  `examples/trace/scenarios.mjs`, and the page counts with `examples/trace/replay.mjs`, served as `/replay.js`. A demo hooks the view through `listeners` and `log`
  options on `runTyping` and `runAlerts`, called before the demo's own
  handlers (a listener that throws skips the demo's own handler for that event, so the view's only serialise and write); terminal output is unchanged and `src/` knows nothing of the view.
- The engine is POSIX only (process groups). `engine run` prints answers from a `result` callback of `runNetwork`, not from the trace, because the trace cuts payloads over 2000 characters. It keeps a map of unfinished work per (sender, key) that is not pruned until the key is replaced, like the library's known limit. When a call ends the engine kills the program's process group; a program that daemonises on purpose cannot survive its call.
- The engine (`src/engine/`) runs real processes on the real clock, so its tests use `sh` and `cat` and assert invariants, never exact timings. It runs `source`, `run`, `serve` and `network` neurons (http, human: `check` warns, `run` refuses); its gaps and the choices it made where the spec is silent are in SPEC Appendix C. A `serve` neuron starts with the engine (a neuron that cannot reach `ready` makes `runNetwork` throw) and stops when the run is over, so a test that ends a run waits for its processes to go; level 1 tests record what the neuron saw in a log file rather than assert timings, and a hung-call test takes about 2.5 s because it waits out the SIGKILL grace. A serve process is noticed as down when it exits (after a 100 ms grace) or closes stdout, so a helper that inherits its stdout does not hide a crash. `engine run` reads inputs with `at` relative to the start of the run. A program gets only `PATH` and its `env` names, so a Python neuron needs `python3` on `PATH` (the Python variant test skips with a warning without it). `spec/examples/alerts.inputs.jsonl` is `examples/alerts/alerts.json` with each alert keyed by its service; `test/engine.test.ts` checks they agree. `test/support/trace.ts` holds the trace validity checks that `recording.test.ts` and `engine.test.ts` share.
- `examples/` imports from `../dist/`, not `src/`, so it needs a build first
  (`npm run example`, `demo:typing` and `demo:rust` build). The demos have no
  tests (run them by hand), except `demo:alerts`: `test/alerts.test.ts` imports
  `examples/alerts-demo.mjs` with `src/`.
- Traces (`traces/*.json`, `docs/TRACE.md`) are committed recordings of real runs: re-record them with `npm run record` after changing a demo, its timing, or a neuron's options. Record on an idle machine; `test/recording.test.ts` checks invariants and that each summary matches its replay. Other projects may copy `traces/` and `examples/trace/replay.mjs`, so a breaking format change bumps `version` (rules in `docs/TRACE.md`). Traces are public: `scenarios.mjs` descriptions name scripts by file name only, and the privacy test fails on paths, emails, hostnames and the machine's user name; private words are checked too but are never committed: list them one per line in `~/.config/agent-neuron/trace-denylist` (outside the repo, so every worktree sees it), or in the gitignored `.trace-denylist`, or in `$TRACE_DENYLIST` (comma separated).
- `network.idle()` waits only for `emit` work, not `send`. `onError` and
  `onDrop` fire only for `emit`; `send` still throws.
- Listeners on `emit` work (`onSignal`, `onError`, `onDrop`) that throw are
  swallowed, never reported as the neuron's error and never an unhandled
  rejection.
- `replyTo` is set only on answers the network makes; `cause` only when the
  caller passes `{ cause }` to `send`/`emit`. Both are ids, and the network
  keeps no signal store, so a caller that needs the signal behind an id keeps
  its own map. Every signal has a `trace`; build one by hand with
  `newSignal(from, payload, { to, replyTo, cause })`, or include `trace` in a
  literal. Signals from `Neuron.query` start their own trace.
- The `replaceable` map in `network.ts` keeps one abort controller per
  sender and `replaces` key and is never pruned.
- `test/rust.test.ts` runs `rust/target/release/catalogue` (and `test/composed.test.ts` `music`) and fail (on
  purpose, with the fix in the message) until `npm run rust:build` has run.
  `verify` builds it before the TS tests. The Rust build is not a `build: true`
  step, so `verify --skip-build` still builds it. Keep the crate std-only, so
  builds never need the network; `rust/target/` is ignored, `Cargo.lock` is not.
- The made-up catalogue lives twice, in `examples/typing-demo.mjs` and
  `rust/catalogue/src/lib.rs` (shared by the `catalogue` and `music` binaries),
  and the matching rule must stay the same in both (lowercase, from 3 UTF-16
  units, no trimming): change them together.
- The two typing demos share `examples/typing-demo.mjs`; `npm run demo:typing`
  output must not change when it is edited.
- The inner rules in `rust/music` share thresholds and data with the neurons in `examples/typing-demo.mjs` (tracks from 3 UTF-16 units, artist on an exact name, lyrics from 5, with the same 250 ms wait when it acts); change both together. The answer shapes differ (artist as the catalogue name, lyrics as the first match). `MUSIC_NO_LATENCY=1` skips the wait; the tests set it. `music --inspect` prints the inner trace and is the view from inside: the outer network never gets it.
- **The fixtures are the contract** (`spec/fixtures/`, SPEC section 8): a second
  engine is judged by them. A fixture changes only in the same commit as the
  SPEC text it pins; never edit an `expect` to make the engine pass. Fixtures are
  engine-independent: no real time, no ties the spec does not decide (a ping due
  at the instant of an injection), invented data. A new engine rule gets a fixture and, if
  it is a rule the engine could break quietly, a mutant in
  `test/conformance.test.ts`. Run one fixture: `npm run engine -- conform spec/fixtures --only <text>`.
- **A second engine was tried and stopped**: a Rust engine written from the spec text alone
  was a no-go (decision 0005; code on the branch `feat/rust-engine`, not merged). The TypeScript
  engine stays the only one. Any future engine must pass every active fixture, so the fixtures,
  not this engine's code, are the contract; where they and the text disagree, fix the pair in one commit.
- Virtual runs (`runNetwork(..., { host })`) swap a module-level `host` in
  `engine.ts` for their length: a virtual run needs no other run active, and no real
  run may start during it; real runs may overlap each other.

- A trace ends with a `finished` event (SPEC 7.2); fixtures compare it only with `"finished": true`, as notes
  need `"notes": true`, so a fixture about something else does not change when the trace gains an event.
- The conformance runner checks the live stream (SPEC 9.1) against the events of every run it makes, so an engine
  change that reorders, drops or hands over its events fails every fixture. A neuron with `effect` is never cut by
  staleness and fails as `unknown`, not `error` (SPEC 11); `effect` is refused inside a composed file.
- **Gotcha: a serve neuron's `timeout` is also its ready deadline on restart** (`startTimer` in `engine.ts`). A test that
  restarts one with a short `timeout` can flake on a loaded machine; keep such timeouts wide (hundreds of ms) rather than
  loosening the assertion.
- An `observer` function that throws used to crash the process; it now fails open (SPEC 9). That was a bug in the
  feature-1 engine, fixed with feature 2.
- A FIFO opened by `blocking` Node file APIs (`createWriteStream`) with no reader hangs `process.exit`; the CLI's stream sink opens
  non-blocking for that reason (`openSink` in `cli.ts`).

## Known limits

Open problems are GitHub issues with the `loop` label (`npm run issue --
list`), not lists in this file.

## Environment

None yet. A model API key, when one is needed, goes in `.env.local`
(gitignored), is documented here by name, and is never written to any file.
