# Durable state: persistence as a contract the engine takes

**Status: planning note, no code (2026-11-09).** Not a proposal to build today
and nothing here changes the SPEC, the schema, TRACE.md, a fixture or a decision
record — a decision record follows a go-ahead, and code starts only after that
doc commit. It scopes issue #30's item 1 (durable run state) as a pluggable
persistence layer, considers making it a neuron of the graph itself, and states
why that last idea lands one level up instead of inside the engine. It rides the
maintainer's substrate note of 2026-11-09 (not a doc): a data engine that holds
the customer's rules needs, as the substrate's control plane, persistence that
survives the process that produced it. Companion note: [MULTIPLAYER.md](MULTIPLAYER.md),
whose build order this one makes concrete for #30 item 1.

Gates on the tree this note was written against (`290652e`, all just measured):
`cargo test` 123, fmt and clippy clean, conform `spec/fixtures` 224/0/0 on
identical reruns, `conform py/probes` 10/0/0. Counts quoted below are either
"unchanged" by argument or from those runs.

## The problem

An hours-long run loses everything when the process dies: a `human` neuron's
open hold (`desk.rs:461` `Desk`, items answered at `:752` through the record
line), the grants the desk validates (`engine.rs:571` holds `desk::Grants`,
set at `:672`), the ground's deposits (`engine.rs:42` `GroundState`, planted at
`:898`), and every accepted answer line. Issue #30's case is a book distillation
running through a closed app: hold survives a restart, keeping key, owner and
item. The multiplayer reading says the same thing as an axis: "persistence —
the table persists between visits". And the substrate reading says it as a
requirement: the control plane's settings change as people and agents use the
system — the run record, the trail and the open asks have to *be* something
between runs, not nothing.

## The candidate shapes

| Shape | What it makes possible | What breaks | Keeps SPEC 6.2? |
|---|---|---|---|
| A. persistence as a neuron in the graph — work routes through it, later runs read it | persistence as composition; the customer sees and edits it as a graph | the durable thing dies with the run (A is a seam, not a store — after a reboot it is a corpse); if it meditates on who proceeds it becomes the central controller MULTIPLAYER refuses; routed-through state forces every internal mutation to become an event, inflating the trace and conformance | no, as written |
| B. engine-internal checkpointing — the run journals all state, resume restores it | resume mid-tick after a kill/upgrade | SPEC 6.2's internals (the wake queue `engine.rs:426`, slot bookkeeping, the id counter) become contract; a checkpoint is not a semantic record, so the journal cannot be replayed or audited; the interface budget pays forever for internals nobody outside the engine should see | no |
| C. persistence as a contract the engine takes — a folded-deposit state model (generalising 0012), a `--journal`-style side surface, and pluggable backends; composes as neurons one level up | holds, grants and deposits survive; async play, carry-over and the org directory ride the same layer; every backend speaks one protocol (the folded list) | — this is the proposed shape | yes, if the write path is a deliberate subset that loads before the seed |

## Why not A, why not B (each with its why)

1. **A stores nothing durable.** Every neuron — whatever it computes — lives
   inside `Engine::run()` (`engine.rs:1096`) and dies at `end_run_quietly`
   (`:1224`). A persistence neuron is therefore a client of state that must sit
   elsewhere; making the seam visible as a neuron is valuable, but making it
   *the store* guarantees the #30 bug persists.
2. **A's mediated table arbitrates.** The moment the store assigns items or
   holds locks it is the multiplayer server MULTIPLAYER.md refuses ("would be
   the central controller the whitepaper positions against"). The line that
   keeps the family's no-scheduler pitch: the store answers what was last
   deposited at a place; the graph decides.
3. **A inflates the event model.** Slot tables, wake queues and id counters are
   engine-internal and sub-tick (`Wake`, `engine.rs:426`; the host queue the
   `Ev` variants of `host.rs:70` report into). Reconstruction from routed
   messages would demand one event per internal mutation, and both the trace
   (TRACE.md) and the conformance word budget (SPEC 4.5) break.
4. **B turns internals into contract.** A checkpoint of engine internals must
   be pinned to survive a refactor, which is exactly the kind of surface the
   house rules keep closed; and a checkpoint that only the engine can read is
   not an audit artifact, so it defeats the substrate use (the org needs to read
   its own record).

## The state model: folded deposits (generalising 0012)

The ground already taught the model: state is deposits at places. Today one
value plus one instant per place (`GroundState.places`, `engine.rs:42-54`) —
the P2P plan's P3 already names the upgrade to a folded list `(place, v, at,
origin)` and a `deposit` message over the wire, because a trail cannot merge
across peers when writes absorb the past. That upgrade is shared ground: the
journal and the wire want the same folded list.

| State class | Deposit shape | Where it lives today |
|---|---|---|
| Open hold (a desk item someone must answer) | deposit at the item's place: key, owner, payload, armed-at | in memory only (`desk.rs:461`) |
| Grant (who may answer what) | deposit in the directory: subject, item kind, scope | a side file today (`engine.rs:571`, loaded at plant) |
| Ground trail | the folded list itself | in memory only (`GroundState.places`) |
| Accepted answer / decision record | the record line the desk already writes (`desk.rs:752`) plus the trace | stderr of a dead process |

One shape means one backend protocol; the wire (P3), the journal (this note)
and the org directory all speak it.

## The contract

The engine already has the precedent for pluggable lifecycles: the TS
reference's `Host` interface (`src/engine/engine.ts:46`) is swapped for the
virtual clock (`src/engine/virtual.ts:16`, `VirtualHost implements Host`).
Persistence takes the same shape — a backend the run is constructed with, not
a participant in the graph:

1. **load(table, digest) → state.** Before the plant, the engine asks the
   backend for the table by key and by identity of the house rules. Identity
   reuse: the peers already load and agree on the network file tree
   (`peers.rs:165`, tree carried and re-keyed at `:213`, `hello` at `:685`) —
   decision 0013's wire section named an FNV-1a text digest for that agreement;
   the journal cites the same agreement (whether the digest itself or the
   loaded tree check is the cited artifact is a decisions-wanted item). A
   mismatch refused like any peer mismatch — never a merge of two houses'
   tables.
2. **record(event).** Write-through: every mutation of the deposit subset
   (hold armed/released, grant accepted, deposit, answer accepted) becomes one
   journal line at the moment the run commits it — the trace remains the
   narration, the journal becomes the state, and the desk's `desk answer:`
   line gains a durable twin. A sketch, not a locked shape:
   `{"k":"hold:arm","table":"…","node":…,"key":…,"owner":"…","at":…}`.
3. **rearm(run).** A restarted run re-arms from the journal: open holds return
   to the desk with the same key and owner, the ground loads its carried
   deposits, grants reload from the directory deposits. To the graph this
   reads as injections at the run's start — the desk becomes the persistent
   table MULTIPLAYER names. The restart's events are the same event kinds the
   trace already carries, so TRACE.md gains at most one note, not a family.
4. **quiescence reads it.** "No open desk items" — item 4's traced
   approximation — stays computable across a reconnect by counting the
   journal's open holds, which is exactly the hard case a reconnect makes.

## The backends, cheapest first

| Backend | What it is | What it costs |
|---|---|---|
| run-folder journal (default) | append-only lines beside the run (the trace's own folder discipline), std-only | the whole contract + one format; no dependency |
| the policy repo | the same journal committed to the versioned file the org reviews — policy and record as one diffable surface | nothing new: a path, not a protocol |
| a peer-hosted trail | rides P3's `deposit` message: the folded list is wire-native, and the trail outlives any one driver; cross-peer reads need P4's quiescence | the P2P pass this sits inside |
| a customer store | never built in: their adapter is a neuron/link in their graph, an ordinary `http` boundary (2.6) | zero protocol surface |

## What breaks if each decision goes the other way

1. **Bespoke state shapes per class** → every backend implements four
   protocols and the P3 wire work doubles; the folded list is one shape both
   layers already want.
2. **State entangled with the seed** (the carried table mixing into SPEC 4.5's
   pinned generator) → the draw's inputs stop being readable in the trace.
   Loading happens *before* seeding; the seed stays a function of the network
   file, and SPEC 4.5's one-word discipline stays untouched.
3. **Checkpoint-only writes** → a killed run loses everything after the last
   checkpoint (issue #30's closed-app case again), and the journal can no
   longer be replayed against the trace, so the audit loses its replay.
4. **Journaling all engine state** → shape B; internals become contract.
5. **A merge on identity mismatch** → two houses' tables fuse silently; the
   substrate's whole claim is that the rules are knowable, so the honest answer
   to a foreign table is a refusal the record keeps.

## Build order, each step with its gate

- **P0 — the folded list.** Generalise `GroundState.places` to the folded
  deposit list: the visible value and draw at every place unchanged (a fold
  that agrees on origin keeps the value; SPEC 4.5's draw reads the folded
  value the same way). Fixture: none — the run-visible behavior is unchanged,
  conform stays 224/0/0 by argument and by the five-run identity.
  Gate: `cargo fmt --check`, clippy `-D warnings`, `cargo test`, the identity.
- **P1 — the journal beside the run.** `--journal <file>` (a side surface, the
  `--grants`/`--peers` pattern): write-through of the deposit subset, load with
  the identity refusal, and re-arm on a second run: an open hold returns to the
  desk with key, owner and item intact. Tests beside `desk.rs`'s: a run killed
  mid-hold re-arms with the same key/owner; a journal from a different network
  file refuses. Gate: new tests; conform unchanged (opt-in surface — a default
  that does nothing).
- **P2 — grants and the directory as deposits.** The 0010 grant file becomes
  one loadable-from-journal state; refusals keep recording the same way.
  Gate: desk tests, conform unchanged.
- **P3 — quiescence's approximation reads the journal.** The traced
  "settled" reads open holds from state, which makes a disconnected table a
  participant the trigger can see. Gate: beside the peers/mcp suites; conform
  unchanged.
- **P4 — the peer trail.** The P2P P3 pass (`deposit` message, gossip, merged
  reads) consumes this state model; its two-peer colony gate is this note's
  cross-peer gate, run inside that pass rather than duplicated here.
- **P5 — the contract pass.** SPEC text for the journal's normative parts (the
  line format, the identity refusal, the re-arm event kinds), TRACE.md's note,
  and fixtures: the restart itself is pinned through a synthetic second phase
  on the virtual `Host` (fixtures never open sockets and never touch real
  files — a two-phase virtual run pins the semantics; the journal *format* is
  normative text plus engine tests, not a fixture artifact). A decision record
  carries the accepted answers.

## What this deliberately does not build

Multi-writer storage, merges or conflict rules (one table, one owner, refusals
not merges); any database or daemon (a journal file and the P2P ground are the
storage); a control-plane UI (standing policy is the git-diffable file set —
the substrate reading's fourth requirement is met as data, not as a product
surface); presence (#30 item 3 rides the MCP job's notification pattern at
`mcp.rs:113,387`, orthogonal to persistence); replication beyond the peer
trail; the adaptive/strength layer (carry-over waits on the same keystone but
is its own pass, MULTIPLAYER §colony reading).

## Decisions wanted

1. **One folded-deposit model for all four state classes** vs bespoke shapes —
   recommended: one model, so the journal, the org directory and P3's wire all
   speak one protocol.
2. **`--journal` opt-in vs always-on default** — recommended: opt-in side
   surface (a default that does nothing keeps every existing trace, test and
   demo identical; async play turns it on).
3. **Identity: the 0013-style digest of the file tree vs the loaded-tree
   check the peers already perform** — recommended: whichever the peers
   already implement, so one agreement mechanism serves wire and journal.
4. **Write-through vs settle-checkpoints** — recommended: write-through (a WAL
   is honest; these runs are small; replay against the trace survives).
5. **How much of the past the graph may see** — re-injection at sources/desk
   only (today's event kinds, one note) vs a general select-over-history rule
   (a draw-budget question, pinned in SPEC 4.5 before it is built).
