# Handoff: the peer layer, and what is left (2026-11-09)

**Note, 2026-11-12:** [decision 0023](decisions/0023-one-engine.md) has since deleted the TypeScript library,
the TypeScript reference engine and the Python engine, and moved `py/probes` to `spec/probes`. Where the text below
names them (`src/`, `py/`, vitest, the mutants), it describes the tree it was written against.

## The goal

Build a recursive, composable, peer-to-peer neural agent graph: a neuron may
contain a whole graph, graphs may nest and compose behind one interface, and
peers may host and run parts of the same graph without a central controller.

**State.** `main` is `777db7e` (the peer-hosted serve, merged `--no-ff`; this
push carries that merge and this note): the serve P1 built on its peer
(`peer --serve`, slots/`maxInflight`/restart/cancel/crash over the wire), the
audit gate 0014 built and merged, the peer layer P0, P0.5, P1.1 and P2 before it.
Working tree clean. Gates on this tree: `cargo test` 123 (unit 2, check 16, desk
17, http 11, mcp 22, peers 26, real 29), fmt and clippy clean, `conform
spec/fixtures` 224/0/0 on identical reruns, `conform py/probes` 10/0/0, `npm run
demo:p2p` exit 0 with four peers, `npm run check` (vitest 264, on the identical
pre-merge tree).

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
none — it already holds). **Done, on `feat/audit-lifecycle` (2026-11-09)**, one
commit per item, decision record
[docs/decisions/0014-engine-lifecycle-audit.md](decisions/0014-engine-lifecycle-audit.md)
carrying the findings, the fixes and the measured gate (cargo test 117,
conform 224/0/0 five runs identical). Merge on Kevin's go; findings 10–17
remain queued after the serve build.

## Remaining, in the order I would take it

### 1. Peer-hosted `serve` (P1's big piece) — built (2026-11-09)

A placed `serve` runs on its peer (`peer --serve`; the hello's mode grew `serve`
and an unhostable worker is refused at the handshake): the wire carries
`serve_start`, `ready` (with `maxInflight`), the relayed lines, and `down`; the
relay never holds its host; an unanswered start fails the plant by the node's
deadline (the 0014 finding-5 cap). Slots, `maxInflight`, a restart, a cancel and
a crash mid-call all act over the wire, and the event lists equal the local
runs'. Commits `1d373f5`..`4191af4` (the red-side first, then the relay-latency
root cause and the remaining levers).

### 2. Budgets across the wire — pinned (2026-11-09, no engine change)

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

### 9. The Node question (decided to freeze, not delete; decided otherwise 2026-11-12)

*Decision 0023 deleted the TypeScript library, its engine and the Python engine without the mutant port named
below; what follows is the position of 2026-11-09.*

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
./rust-engine/target/release/neuron-engine conform spec/fixtures   # 224/0/0 then (277 since), five runs identical
./rust-engine/target/release/neuron-engine conform spec/probes     # 10/0/0 (py/probes until decision 0023)
npm run demo:p2p                                                   # four peers + a driver
npm run check                                                      # since decision 0023: the four lines above but the demo, in one command
```

Merges: a feature branch, `--no-ff`, a message in the repo's style; gate the
**merged** `main`, then push and compare `git ls-remote` heads. Kevin's go for
each merge.
