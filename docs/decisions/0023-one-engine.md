# Decision record 0023: one engine (2026-11-12)

**Status: decided 2026-11-12 by Kevin. Built 2026-11-12 (the section "As built" at the end).** The TypeScript library, the TypeScript reference
engine and the Python engine are deleted from the repository. The Rust engine is the engine. The
contract is unchanged: the SPEC, the schemas and the fixtures.

## What is true today

| Piece | Where | What it is |
|---|---|---|
| The TypeScript library | `src/` outside `engine/`, about 440 lines | the first thing written: `Network`, `Neuron`, adapters. It predates the SPEC, does not read network files, and Appendix C lists how it differs |
| The TypeScript reference engine | `src/engine/`, about 2,050 lines | reads network files, runs real programs, and was the first engine the fixtures were checked against |
| The Python engine | `py/`, about 2,150 lines | written from the SPEC text alone, scripted, starts no programs |
| The Rust engine | `rust-engine/`, about 16,600 lines | the one that runs real networks: peers, the desk, confinement, MCP, live and hosted runs |

Every contract step since decision 0009 has been built three times. `maxInflight` (0021 L2) was a
queue in three engines; decision 0022 has three more steps of that size to come.

## The decision

1. **The three are deleted**, not archived and not moved to another repository. The history keeps
   them: the last commit that has them is the parent of the commit that removes them.
2. **The Rust engine is the reference.** `neuron-engine conform spec/fixtures` is the conformance
   run, and `neuron-engine check` and `run` are the commands the SPEC's informative text names.
3. **The contract does not move.** No normative SPEC text, no schema and no fixture's `expect`
   changes with this. The SPEC stays a text an engine can be written against; it now has one.
4. **Whatever only existed to serve the three goes with them**: their tests, the examples that
   import the library, the build and its configuration, the package entry points, the Python probes'
   place under `py/` (the probes themselves are fixtures and move to `spec/probes/`).

## What it costs, said plainly

- **The cross-check on the fixtures is gone.** Three independent engines had to agree with each
  hand-written expectation. With one, a fixture and the engine can be wrong together and nothing in
  the repository notices. A fixture's `expect` is written by hand from the SPEC text and reviewed as
  such; that was always the rule, and it is now the only guard.
- **The mutant test is gone.** A hundred deliberately broken TypeScript engines each had to fail a
  fixture: the measure of whether the fixtures pin the rules. Nothing replaces it here. A mutant run
  against the Rust engine would, and is not decided.
- **The evidence that the text stands alone becomes history.** The whitepaper's point that an engine
  written from the SPEC alone passed the same fixtures is still true of the commit before this one,
  and the documents say it in the past tense.
- **The npm package is no longer a library.** Nothing was published. A package that carries the
  engine binary (Appendix B) is still to come.

## What it buys

A contract step is built once. The gate is one toolchain. The repository says what the project is:
a contract and the engine that runs it.

## Not decided here

- A mutant run for the Rust engine, or a second engine by anyone else.
- Distribution of the binary.
- The trace's `library` header field (7.1): the format is unchanged.

## The build, with its gate

One `feat/` branch, on top of `maxInflight` and `args`. Gate: nothing in the tree imports or names a
deleted piece except as history (a decision record, a dated note); the Rust engine's own gate passes
(`cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, `conform spec/fixtures` and
`conform spec/probes`); `npm run check`, if it remains, runs that gate; every demo and example left
in the tree runs.

## As built (2026-11-12)

On the branch `feat/one-engine-removal`, in two commits: the deletions, then the documents.

**Deleted.**

- `src/` whole: the library (`network.ts`, `neuron.ts`, `signal.ts`, `adapter.ts`, `adapters/`, `index.ts`) and
  the reference engine (`src/engine/`).
- `py/agent_neuron/` and `py/tests/`, and with them `py/`.
- `test/` whole: all 16 test files and `test/support/` imported the library, the TypeScript engine or a demo
  built on them. The mutant test went here.
- The build and test configuration: `tsconfig.json`, `tsconfig.build.json`, `vitest.config.ts`, every dev
  dependency (`typescript`, `vitest`, `@types/node`, `playwright-core`), and `package-lock.json`, which has
  nothing left to lock. From `.gitignore`: `dist/`, `test/.mutants/`, `.trace-denylist` (it served only the
  deleted privacy test) and `rust/target/`.
- The library's demos: `examples/first.mjs` and `examples/first/`, `typing.mjs`, `typing-demo.mjs`,
  `suggest.mjs`, `loop.mjs`, `alerts.mjs`, `alerts-demo.mjs` and its types, `rust.mjs`, `composed.mjs`,
  `examples/alerts/diagnosis.mjs` and `examples/alerts/alerts.json` (the library demo's copy of the 40 alerts;
  `spec/examples/alerts.inputs.jsonl` holds the same alerts keyed by service).
- `examples/notify.mjs`: it drove the TypeScript engine through its library options (`onEvent`, `observer`).
  Its network file and programs stay in `spec/examples/` and run with `neuron-engine run`.
- `examples/view/` whole: the page drew only runs the server made live through the library. It never read a
  trace file, so no half of it works without the library.
- `examples/trace/record.mjs`, `scenarios.mjs`, `tap.mjs` and the type files: the recorder of library runs.
- `assets/fonts/`: the fonts of the TypeScript engine's page for `human` neurons. The Rust desk uses the
  system's fonts.
- `docs/conformance-python.md`.
- **`rust/`** (the `catalogue` and `music` crates), on the maintainer's instruction of 2026-11-12. Its only
  users were `examples/rust.mjs`, `examples/composed.mjs`, `test/rust.test.ts` and `test/composed.test.ts`,
  the `rust:build`, `rust:test`, `demo:rust` and `demo:composed` scripts, and passages in the README,
  CONTRIBUTING and DEMOS, all removed here. `rust-engine/` never referred to it.

**Kept.**

- `rust-engine/`, unchanged but for its README, the description in `Cargo.toml` and one comment. None of its
  tests depended on a deleted piece.
- `spec/` whole. No schema and no fixture changed; `py/probes/` moved to `spec/probes/` by rename only.
- `spec/examples/` and the shell detectors and data they name in `examples/alerts/`.
- `examples/network.mjs`, rewritten: it starts `neuron-engine run` and summarises the trace it wrote, where it
  used to call the TypeScript engine in process. `npm run demo:network` therefore still exists.
- `examples/human/`, `examples/team/` with `examples/team-demo.mjs`, `examples/p2p/`: they ran on the Rust
  engine already.
- `examples/trace/replay.mjs`: a reader of trace files with no imports, used by `examples/network.mjs`.
- `traces/`: five recordings of the library's demos. They are valid files of the trace format, TRACE.md and
  the whitepaper quote them, and they cannot be recorded again.
- `assets/brand/` (the README's mark).
- `package.json`, with no dependencies: `check` (the gate below), `conform`, `engine:build`, `engine:fmt`,
  `engine:clippy`, `engine:test`, and `demo:network`, `demo:human`, `demo:team`, `demo:p2p`. `.nvmrc` stays:
  the demos' programs are Node scripts.

**The gate.** `npm run check` is `cargo fmt --check`, `cargo clippy --release --all-targets -- -D warnings`,
`cargo test --release`, a release build, `conform spec/fixtures` and `conform spec/probes`, all on
`rust-engine/`.

**Judgement calls.**

1. *The SPEC's normative text.* No rule changed. Section 0 (informative) has the Rust engine's commands. Five
   sentences in normative sections named a deleted piece and were changed by that much: 6.2 rule 6 lost the parenthesis "(the
   TypeScript `idle` does)"; section 9's bullet on the library's listeners is in the past tense; 9.1 names the
   Rust engine's library option (the `sink` of `run_with_stream`) where it named `onEvent` on `runNetwork`;
   11.4 no longer cites the notify demo beside the fixture; 11.6 says "`http` neurons are not run by every
   engine" where it said "by the reference engine", which is now false of the reference. Appendix A names
   `spec/examples/alerts.inputs.jsonl` where it named the deleted `examples/alerts/alerts.json`.
2. *Section 8 still says the traces in `traces/` "stay as smoke tests through replay".* Left word for word,
   and no test replays them now. Open: restore a replay check, or change the sentence with the next contract
   commit.
3. *Appendix C* is rewritten as "The engine in this repository". Every bullet that stated a fact about the
   Rust engine is kept. The bullets about level 1 and the engine defaults were written for the TypeScript
   engine; those the Rust engine's own comments and README cite as Appendix C (the clock's start, the failed
   start, the 100 ms settle, the end of a run, `exit 127`, the kill on a signal) are kept as its own, and the
   ones nobody checked against it are dropped (the unpruned staleness map, the microtask order, `performance.now()`,
   a queued stale call's missing `cancel` note, the `crash` message's wording). The per-signal measurements are
   kept, marked as taken with the TypeScript engine.
4. *Appendix D* said the page loads its fonts from its own origin. The Rust desk uses the system's fonts, and
   the sentence says so now.
5. *The storm's numbers.* The README's example output was 52 flags and 17 calls from the TypeScript engine. On
   the Rust engine, on a machine under load while this was built, runs gave 26 to 36 flags and 7 to 10 calls,
   always with 7 diagnoses. The README shows one such run and says the counts move with load. Not established:
   what the Rust engine gives on an idle machine, and whether it cuts more detectors than the TypeScript
   engine did.
6. *`RUST_TEST_THREADS=1`.* `rust-engine/.cargo/config.toml` sets it, but cargo reads that file only when
   started inside `rust-engine/`. `npm run engine:test` uses `--manifest-path` from the root, so it sets the
   variable itself.
7. *The team demo* builds and runs the debug binary by itself; it was left so.
8. *Dated documents* (HANDOFF, DURABLE-STATE, the planning assessments, the milestone sections of
   `rust-engine/README.md`, the whitepaper's measurements) keep their text and gained a dated note saying what
   decision 0023 removed; only commands a reader would run today were changed.
9. *Nothing replaces the privacy test of committed traces*, nor the deleted tests that ran the example
   networks; the examples were run by hand for this build.
