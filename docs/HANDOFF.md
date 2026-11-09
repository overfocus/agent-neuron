# Handoff: the peer layer, and what is left (2026-11-09)

## The goal

Build a recursive, composable, peer-to-peer neural agent graph: a neuron may
contain a whole graph, graphs may nest and compose behind one interface, and
peers may host and run parts of the same graph without a central controller.

**State.** `main` is `da0b7c7` (pushed): the peer layer P0, P0.5, P1.1 and P2 are
merged, as is the composed-neuron fix P0 exposed. Working tree clean. Gates on
this tree: `cargo test` 106 (unit 2, check 16, desk 17, http 11, mcp 19, peers 13,
real 28), fmt and clippy clean, `conform spec/fixtures` 224/0/0 on five
consecutive identical runs, `conform py/probes` 10/0/0, `npm run demo:p2p` exit 0
with four peers, vitest 264.

Where the design lives: [docs/P2P.md](P2P.md) (the plan, the shapes, the
breakages, the per-step gates), [decision 0013](decisions/0013-p2p.md) (what was
decided and what the boundary carries), and `rust-engine/README.md` ("Where it
stands, and the roadmap").

## What is built, in one line each

| | |
|---|---|
| P0 | `run --peers <file>` + `neuron-engine peer`: a placed `run` neuron's call is dispatched over newline JSON/TCP (v1) and its outcome returns as `Ev::L0Done`; the run's events equal a local run's |
| P0.5 | `peer --network <file>`: a peer is one neuron whose inside is a graph (payload in at `entry`, answer out of `exit`, inner failure is `inner` with the cause sealed on the worker's stderr); placement may name a `network` node |
| P1.1 | the `cancel` message: a cut stops the peer's program and is not held for the grace; `effect` on a dead peer is `unknown` |
| P2 | placement is a node *path* (`audit`, `review/wrap`), so a hosted graph places a composed node of its own: composition nests, three engines deep |
| fix | a host event that empties a composed call's inside now runs 2.6 step 3's check (the real host used to wait for the composed timeout) |

## The audit gate before them (0014, accepted Kevin, 2026-11-09)

Before priority 1, audit findings 1–9 are fixed with a failing-test-first
regression test each, and priority 2 gets its regression test (engine change:
none — it already holds). Findings 10–17 are queued after the serve build.
The findings, with file:line at `e407c21` and the accepted per-finding gates:
[docs/decisions/0014-engine-lifecycle-audit.md](decisions/0014-engine-lifecycle-audit.md).

## Remaining, in the order I would take it

### 1. Peer-hosted `serve` (P1's big piece)

The last call kind, and the only reply that carries `to` and `replaces` (3.2) —
today a peer runs one level-0 program per call and nothing long-lived. A
timbre-style crew (a writer, two reviewers) is `serve` neurons; until this lands
they can only run on the driver's machine.

Where: `engine.rs` (`new_serve`, `start_serves`, `signal_line`, `consume_line`,
`restart`, slot bookkeeping — all keyed by node index and assuming a local
process), `peers.rs` (the worker spawns an L1 process, relays its lines, answers
`cancel`, reports its exit). The wire grows `serve_start` / `ready` / `line` /
`down`; the hello already reports the peer's mode.

Gate: a peer-hosted `serve` test beside the existing level 1 tests (slots,
`maxInflight`, a restart, a cancel line, a crash mid-call), conformance untouched.
Trap: `start_serves` blocks the plant until every serve says ready; a peer that
never answers must be a start failure, not a hung plant.

### 2. Budgets across the wire (probably already true — pin it)

Under A the driver holds the pool and does `reserve_call`/`commit_call` for a
remote call exactly as for a local one, so the accounting should already be
right. Nobody has tested it.

Gate: a network with a `budget`, a placed neuron, and a second call the pool
cannot cover: the trace carries the `reserve`/`commit` notes and the `budget`
error. If it passes with no code, change nothing but the test.

### 3. A cancel a hosted graph's engine reads

`peer --network` runs the file inside `Engine::run()` and cannot see the cancel
flag until the graph's deadline. A cut of a hosted graph therefore waits for the
socket shutdown plus the backstop.

Where: `engine.rs` gains a cancel handle the run loop checks (also useful for MCP
jobs); `peers.rs` sets it on `cancel` in graph mode and reports the outcome.
Gate: cut a hosted graph whose exit sleeps; the run finishes inside the grace.

### 4. The graph boundary's `replaces` and composed `cost`

A remote composed call inherits the outer input's key instead of the exit reply's
`replaces`, and the composed neuron's `cost` does not cap the inner budget on the
far side (10.5). Both are fields on the existing `call`/`done` messages.
Gate: a remote composed neuron whose exit sets `replaces`, and a cost that binds.

### 5. P3 — the ground over the wire

`GroundState.places` keeps one `(value, last write)` per place, so each write
absorbs the past; a trail cannot merge across peers until deposits are kept as a
folded list `(place, v, at, origin)`. Then a `deposit` message and a gossip to the
peers, and `select` draws read the merged trail.

Gate: a two-peer colony drawing the same numbers as one engine (same `--seed`).
Trap: the draw is pinned in SPEC 4.5 — one word per omitted-`to` reply, whatever
the outcome; a replicated trail must not change the number of draws.

### 6. P4 — quiescence (issue #30 item 4)

"The run settled" has no trigger, and under peers it becomes distributed
termination detection: every peer must know every other has nothing open. The
traced approximation first (no open desk items, no pending wakes, the ids header
letting two reads agree).

Gate: a run that ends only when every peer is empty; a silent peer keeps it open
and then fails honestly.

### 7. P5 — the contract pass

Decision record, a SPEC section for placement and the wire's normative parts,
TRACE.md's merged reading (parked with B: no engine holds nothing today), and a
`peers` fixture area with a scripted peer (fixtures never open sockets).

### 8. The engine track, from issue #30 and the known gaps

- **Durable run state** (item 1): an open hold, the grants and the trail die with
  the run. The keystone under carry-over, async play and the ground's other half.
- **Presence** (item 3): a page-pending event on the MCP job pattern; item 2's
  grants are built, the org directory is not.
- `describe_network` does not carry `ground` (the demos' manifest patch
  evaporates); the `http` failure row has no fixture; SPEC questions #24 (both
  engines disagree on a composed exit at its timeout instant), #21, #20, #19.

### 9. The Node question (decided to freeze, not delete)

The TS engine stays the conformance oracle. If it is ever to go, **port the 91
mutants in `test/conformance.test.ts` to Rust first** — they are the only test of
the fixtures' quality, and Rust has none. Then the library and `examples/*.mjs`,
then the public claims (README lines 7 and 117, the whitepaper's engine section,
Appendix C).

## Traps this work paid for

- **A host event that empties a composed call's inside must run
  `wake_composed_calls`** (2.6 step 3). Both `host_step` branches do; a new
  host-event path needs the same call.
- **A remote call has no `gid`**, so `start_l0`'s timeout arm must include the
  `remote` flag; every later path (`terminate_l0`, `fire_l0_grace`,
  `kill_group_now`, run-over) reads the process group and reaches a remote call
  through the same `L0Proc`.
- **A peer's log line must never panic on a closed stderr pipe** — `eprintln!`
  does, and it takes the connection (and the call) with it. `writeln!(stderr,
  ...)` with the error dropped.
- **Ids are per engine**: a peer-hosted graph does not draw on the driver's
  counter. Compare traces with `t` zeroed and ids renormalised by first
  appearance (CONFORMANCE 8.3), which is what `tests/peers.rs` does.
- **The handshake reports the mode** (`program` / `graph`) and dispatch refuses a
  mismatch; a `network` node pointed at a program worker is one clear error.
- **A cut is a message now**: the socket shutdown is the fallback, the 2 s grace
  is the backstop.
- Real-process tests serialize (`RUST_TEST_THREADS = 1`); `free_port()` races a
  concurrent bind.

## The gate

```
cd rust-engine && cargo fmt --check && cargo clippy --all-targets -- -D warnings
cd rust-engine && cargo test
./rust-engine/target/release/neuron-engine conform spec/fixtures   # 224/0/0, five runs identical
./rust-engine/target/release/neuron-engine conform py/probes       # 10/0/0
npm run demo:p2p                                                   # four peers + a driver
npm run check                                                      # typecheck, rust/ workspace, vitest 264, build
```

Merges: a feature branch, `--no-ff`, a message in the repo's style; gate the
**merged** `main`, then push and compare `git ls-remote` heads. Kevin's go for
each merge.
