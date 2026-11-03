# agent-neuron

An experimental agent graph. An **agent-neuron** is a universal wrapper, or
container, that a single agent runs in. Neurons connect to each other
many-to-many: any neuron can send to and receive from any number of others,
so agents form a graph rather than a tree or a pipeline. A TypeScript (ESM, strict) library on
Node 22, tested with Vitest. There is no website and no deploy yet. The code
lives at `github.com/overfocus/agent-neuron`, private for now.

This is not one of the sibling Next.js sites and shares no files with
portfolio (`portfolio/docs/shared-files.json` does not list it).

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
npm run verify      # the gate: typecheck, rust test, rust build, test, build; writes .verify/receipt.json
npm run verify:receipt  # exit 0 only for a PASSED full verify of this clean tree
npm run ship -- --approved "<go-ahead>"  # after Kevin's go-ahead (/loop:ship)
npm run wt -- new <slug>  # a worktree on feat/<slug>; `list`, `gc [--apply]`
npm run ships -- summary  # what shipped this week
npm run issue -- add --title "..." --kind bug|debt|idea --source human --body-file <f>  # `list` shows the open ones
```

`verify`, `verify:receipt`, `ship`, `wt`, `ships` and `issue` are the agent-kit
loop plugin's scripts, run through `scripts/kit.mjs` (a copy of the kit's
template; keep it equal) and set by `agent-kit.config.json`. There is no
`check:browser`: nothing renders in a browser yet. Add it (and
`playwright-core`) when a UI exists, such as a graph visualiser.

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
| The demos (`npm run example`, `demo:typing`, `demo:loop`, `demo:suggest`; they build first. `demo:rust` and `demo:composed` live here too and need the Rust build; list in `docs/DEMOS.md`) | `examples/` | none: run by hand |
| Rust neurons (std-only): `catalogue` (library + binary; `npm run demo:rust`) and `music`, a composed neuron with its own inner network (`npm run demo:composed`) | `rust/` | `cargo test` in each crate; `test/rust.test.ts` and `test/composed.test.ts` drive the built binaries |

## Everywhere

- **The neuron interface is most valuable the simpler it is.** Every new
  capability must keep a neuron's interface small: prefer an option on what
  exists over a new method, a field over a new type, and a default that does
  nothing over a setting everyone must learn. A feature that makes the
  simple case harder to write is the wrong shape.
- Every feature that adds or changes visible behaviour adds or updates a demo
  and its row in `docs/DEMOS.md`.
- Library code has no side effects on import, and the core takes no
  dependency on one model provider: an agent inside a neuron is whatever the
  caller plugs in.
- Never put Kevin's name, email or anything personal in a request to an
  outside service, a fixture or a log.

## Things that are easy to get wrong

- ESM with `NodeNext` resolution: relative imports need the `.js` extension
  (`import { x } from './x.js'`), even from `.ts` files.
- A neuron with a `threshold` holds inputs inside `Network` (`hold` in `src/network.ts`),
  not in `pending`, so `idle()` does not wait for them and their `unref`'d timers
  never keep Node alive: a script that ends right after `idle()` never sees the
  `'expired'` drop (the suggest demo sleeps past `within` to show it). `send` and `query` skip the threshold.
- `examples/` imports from `../dist/`, not `src/`, so it needs a build first
  (`npm run example`, `demo:typing` and `demo:rust` build). The demos have no
  tests; run them by hand.
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

## Known limits

Open problems are GitHub issues with the `loop` label (`npm run issue --
list`), not lists in this file.

## Environment

None yet. A model API key, when one is needed, goes in `.env.local`
(gitignored), is documented here by name, and is never written to any file.
