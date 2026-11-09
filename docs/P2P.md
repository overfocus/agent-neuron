# P2P: one graph, several referees

**Status: planning note (2026-11-09), P0 built.** The destination and the
placement rule below are accepted — [decision 0013](decisions/0013-p2p.md) is the
decision of record — and P0 (one call on a peer, `run --peers`) is built and
gated. This note reads the Rust engine's own seams, names the three things "P2P"
could mean here, picks one, and puts a build order down with the contract cost of
each step.

## What "P2P" could mean, and which one this is

| | Shape | Keeps SPEC 6.2? | New contract | Cost |
|---|---|---|---|---|
| **A** | One referee per run; other hosts execute calls it dispatches | yes, unchanged | none at first | a hub: the referee is the single point of failure and the owner of the trace |
| **B** | Every peer runs the graph it hosts; a signal crosses a peer edge | per peer; a merged trace needs a rule | a wire format, placement, id scheme, cut routing, quiescence | the real work |
| **C** | Two engines, each with its own network file, wired at the boundary | each run, unchanged | none: this is already legal | one shim per boundary, and no rule spans the two runs |

**C is what the protocol already says.** SPEC 2.6: "A boundary that must hold
against the holder of the inside is not a composed neuron: it is an `http`
neuron to that party's own engine, which keeps its inside and its accounting to
itself." The engine has the client half (`http.rs`) and neither half of the
server side for injections: today the far side needs a small program that is
both an HTTP endpoint and a level 1 neuron. So C is real, ships this week, and
buys nothing but composition — threshold, staleness and budgets stop at the
boundary.

**This note plans B, built through A.** A is not a detour: the engine's host
boundary *is* the distribution boundary, so A is what B's first step looks like
from the inside (below). B is the one that matches the family's pitch — no
central controller, the graph is the referee — and the one the ground layer
(decision 0012) already assumes: a trail is shared state by construction.

## What the engine already gives you (the seam)

The Rust engine is a single-threaded decision core with every external
executor reporting into one queue. That is exactly the shape a peer executor
needs.

- **One queue, stamped at read time.** `Host` / `Post` / `Ev` (`host.rs:347`,
  `host.rs:66`) collect `L0Done`, `L1Line`, `L1Down`, `HttpDone`, `Desk`;
  `Engine::host_step` (`engine.rs:1173`) is the only consumer and advances
  `self.now` to the read's stamp. A remote call is one new `Ev` variant and one
  thread — the same shape `http.rs:85` and the desk's accept loop already use.
- **One reporting point.** `Engine::emit` (`engine.rs:686`) is where the trace,
  the live stream and the observer all see an event, and where sealing drops
  inner ones (`inst != 0`). A multi-peer trace becomes "emit also stamps the
  origin", one function.
- **Call lifecycle is already remote-shaped.** `Work` (`engine.rs:166`) carries
  `st`, `timeout`, budget reservation, effect marking, and `done` waits for the
  *program* to be gone, not for the reply (6.2 rule 6). A peer-held call has
  those fields already; `cancel` (5.4) is the only one that needs a wire.
- **Ids are opaque by rule.** "Ids (signal ids, trace ids) are opaque strings:
  compare them, never parse them" (TRACE.md), and the header names the scheme
  (`IDS_SIGNAL`/`IDS_TRACE`, `trace.rs:24`). A `peer-numbered` scheme is an
  addition the v1 reader already tolerates — no format break.
- **The ground is almost mergeable.** `GroundState.places` is `place ->
  (value, last write)` and `effective()` ages by `exp(-evaporate * (t-at))`
  (`engine.rs:42-76`). A deposit is additive and ageing is a pure function of
  time, so a folded list of `(place, v, at, origin)` merges commutatively. The
  current single pair per place *absorbs* the past on every write, which is the
  one thing to change before a trail can span peers.
- **The transport inventory, honestly.** `TcpListener` exists once, bound to
  `127.0.0.1` for the desk (`desk.rs:470`). `TcpStream` exists as a client
  (`http.rs`). `Cargo.toml` has no dependencies: no TLS, no crypto, no async.
  Whatever the wire is, it is hand-rolled TCP plus the JSON layer, like the
  MCP server and the desk.

## What breaks, and cannot be waved away

1. **The total order.** SPEC 6.2 promises one list two engines agree on. Across
   peers there is no single clock. The honest replacement: a hybrid logical
   clock (physical ms + counter, ~30 lines) per event on the wire, 6.2 holds
   *within* a peer, and a merged trace orders by `(stamp, peer)` — deterministic
   given the stamps, not equal to any single-engine order. Conformance is
   untouched (it never opens a socket); a merged trace is a new reading, and
   the merge rule has to be written down.
2. **Staleness.** 5.4 cuts *running* work, and running work lives on exactly one
   peer. Arrival-order dropping handles held and queued work for free; a running
   call needs a `cut {scope, key, value}` message to every peer. `ScopeKey` is
   `(instance, node)` (`engine.rs:82`) — instance ids are per-engine run numbers,
   so they need a peer namespace before they can be routed.
3. **Budgets.** `pools` is `trace -> Units`, reserved and committed inside one
   engine (`engine.rs:480` region). Two options: the trace's owner arbitrates
   every reservation (a round trip per call, and an owner per trace — not a
   controller per network), or the pool is split into declared per-peer shares
   at the plant (no coordination, weaker guarantee: one peer can starve while
   another sits on units). Recommend shares first, owner later.
4. **Quiescence.** "The run is over" (6.2 rule 6) becomes distributed
   termination detection: every peer must know that every other peer has nothing
   open, which is exactly issue #30 item 4. This is the keystone again — and it
   is a real algorithm (Dijkstra–Scholten over the signal traffic, or a
   heartbeat-and-view exchange), not a flag.
5. **Partition versus death.** A peer that stops answering may be dead or
   cut off. An effect call on it is `unknown` (11.3) either way, never retried;
   a peer that returns must not resurrect committed work. Call ids and the `re`
   discipline make that checkable per call.
6. **Security, with no TLS.** The desk's fence is loopback-only and dies with the
   run (`desk.rs:2`); a P2P run leaves the loopback by definition. Two choices:
   trust the network you run on (tailnet/VPN) with a per-peer pre-shared MAC as
   defence in depth — a hand-rolled SHA-256/HMAC, ~150 lines, known-vector tests
   — or no authentication at all, which is not defensible even on a lab. If
   humans must answer from another machine, the desk leaves loopback and 0008's
   rule list needs a new decision, separate from this note.
7. **Field budget (SPEC 2.8).** A placement field says neither how neurons
   connect nor how one decides — it is a third class, and the budget's question
   ("connect, or decide?") has no room for it without an argument. Avoid it:
   placement rides a side file passed at run time, the way `--grants` does
   (decision 0010). The network file stays the house rules; deployment stays
   the deployer's.

## The wire (first sketch, not fixed)

Newline-delimited JSON over TCP, one request/response per line, the MCP
server's framing without the JSON-RPC envelope. Versioned `v: 1` like the
level 1 protocol; an old peer refuses a version it would misread.

| Message | Direction | Carries |
|---|---|---|
| `hello` | both | peer name, `v`, the network tree's digest (FNV-1a over the file text: agreement, not authentication), the neurons it hosts |
| `call` | driver to host | signal id, trace id, node id, hops, payload, replaces key, timeout, cost, `effect` |
| `answer` / `quiet` / `error` | host to driver | payload, `to` resolved by the driver, replaces key; or the 6.1 code and message prefix |
| `cancel` | driver to host | signal id, the reason (stale, timeout) |
| `cut` | driver to all | scope, key, value — a newer input cut running work here |
| `deposit` | any to all | place, amount, at (the ground, replicated) |
| `settled` | host to driver | nothing open for this peer on this trace |

Caps match the local ones: 1 MiB payload, 1 MiB line, 256-byte keys. Auth: a
pre-shared secret per peer, a monotonic counter and a MAC per line, or an
explicit trust mode that says so in stderr.

## Build order

Each step states what it proves and which existing gate it must not move.

| Step | Scope | Proves | Gate |
|---|---|---|---|
| **P0** *(built 2026-11-09)* | `--peers <file>` flag; the peers file names addresses, tokens and neuron ids; a `peers` module; one loopback peer that runs a single `run` neuron | the seam: a call dispatched over TCP settles through the host queue and lands in the trace as a local one | `tests/peers.rs` (6 tests, all green); `cargo test` 98; `conform` 224/0/0 on five identical runs and `py/probes` 10/0/0, unchanged; a peer run's trace equals the local run's event for event, the wall-clock `t` zeroed |
| **P1** | the full call lifecycle: cancel, timeout, crash, a peer-hosted `serve` with slots and restarts, `effect` on a dead peer is `unknown`, budget reserve/commit against the trace owner | A is complete: one referee, many hosts | peers tests beside `real.rs` (serialized, `RUST_TEST_THREADS = 1`); every 6.1 code exercised across the wire |
| **P2** | links that leave a peer: `answer.to` resolved at the driver, `cut` routed, per-peer trace segments with the `peer-numbered` header scheme and a merge reader | B's routing half; the trace question answered in code | a two-peer network in `tests/peers.rs` whose merged trace equals the single-engine trace of the same graph |
| **P3** | the ground over the wire: deposits as a folded list, `deposit` gossip, `select` draws read the merged trail | the colony layer on shared state; the P2P answer to 0012's open half | a two-peer colony fixture-shaped test: the same seeded draws as one engine when the deposits are unioned |
| **P4** | quiescence: termination detection over the traffic, `settled`/view exchange, the run's end across peers | issue #30 item 4, distributed | a run that ends only when every peer has nothing open; a partitioned peer keeps it open and then fails honestly |
| **P5** | the contract commit: decision 0013, SPEC section for placement and the wire's normative parts, TRACE.md's merged reading, a `peers` fixture area with a scripted peer | the change is the protocol's, not one engine's | all three engines (or an honest scope line saying which), fixtures 224 + the new area, five-run identity |

Sizing, honestly: P0 is a day, P1 a day or two, P2 and P3 are the real build
each, P4 is research (the algorithm is known; making it deterministic for
fixtures is not), P5 is a contract pass like 0012's.

## Fixtures never open sockets

CONFORMANCE fixtures script programs and people on the virtual clock and start
nothing. A peer is therefore scripted on the virtual host like a program: the
fixture's script answers `call` messages, and `cut`/`settled` are scripted
facts. That is how B's non-local behavior gets pinned at all, and why P2 comes
before P5.

## What this deliberately does not build

No discovery (peers are named in the peers file; the file is the house rules),
no membership gossip, no consensus, no TLS, no DHT, no moving a live `serve`
between peers (a restart is local), no composed neuron split across peers (a
composed instance is atomic: sealing plus its shared inner processes make it the
natural unit of placement).

## Decisions wanted

1. **Destination B, seam A.** Accepted (Kevin, 2026-11-09): peer execution under
   one referee first, partitioned referees as the target.
2. **Placement in a side file (`--peers`), not a SPEC field.** Accepted (Kevin,
   2026-11-09): keeps 2.8's budget whole until the shape is known; `check` gains
   a warning rule (a command that is not found is a warning only where that neuron
   is hosted).
3. **The clock rule:** a hybrid logical clock per peer and a merged trace ordered
   by `(stamp, peer)` — accepting that a multi-peer trace is deterministic but
   not equal to a single-engine one.
4. **Budgets:** declared per-peer shares first, an owner per trace later.
5. **Security posture:** pre-shared MAC per peer inside a trusted network, no
   TLS, desk stays on loopback in v1 (remote humans are a separate decision).
