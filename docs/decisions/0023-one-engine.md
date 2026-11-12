# Decision record 0023: one engine (2026-11-12)

**Status: decided 2026-11-12 by Kevin. Not built.** The TypeScript library, the TypeScript reference
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
