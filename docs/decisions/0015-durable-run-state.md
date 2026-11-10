# Decision record 0015: durable run state — the journal, P0+P1 (2026-11-10)

**Status: decided (Kevin, 2026-11-10)** — accepted on the go "go with recs" against
[planning/assessment-spec-vs-harness.md](../../planning/assessment-spec-vs-harness.md)
(pending decision 1: W1 first), riding [DURABLE-STATE.md](../DURABLE-STATE.md)'s shape C
and the recommendations its "Decisions wanted" section already marked. W1's waves order,
auth, version and library decisions are the assessment's; this record carries the build
answers of the durable-state pass itself. Build proceeds on `feat/durable-state`, one commit
per coherent unit, merge on Kevin's go.

**What this record scopes:** DURABLE-STATE's P0 (the folded-deposit list) and P1 (the
journal). Nothing here touches the SPEC, the schema, TRACE.md or a fixture: P0 changes no
run-visible behaviour, and P1 is an opt-in side surface on the `--grants`/`--peers`
precedent (the contract text pass is DURABLE-STATE's P5, deferred). Conformance stays
224/0/0 unchanged — a default that does nothing.

## The accepted answers

1. **One folded-deposit model for all four state classes** (holds, grants, ground trail,
   accepted answers ride one `(place, v, at, origin)` list per table),
   not bespoke shapes per class — so the journal, the org directory and the P2P `deposit`
   wire message speak one protocol. (DURABLE-STATE rec 1.)
2. **`--journal` is an opt-in side surface**, the `--grants`/`--peers` pattern; never a
   default. A run without it behaves byte-identically to today. (Rec 2.)
3. **Identity is the peers' tree-agreement mechanism, reused** — whichever the peer
   handshake (`peers.rs`) already performs: the journal is loaded only when the mechanism
   agrees, and a mismatch refuses the run the way a peer mismatch refuses. Never a merge
   of two houses' tables. (Rec 3.)
4. **Write-through, not settle-checkpoints**: every mutation of the deposit subset becomes
   one journal line at the commit (a WAL is honest; replay against the trace survives).
   (Rec 4.)
5. **Re-injection reads at sources/desk only in this pass** (today's event kinds, one
   note); a general select-over-history rule is not built — the draw-budget question it
   raises stays pinned in SPEC 4.5 before it ever is. *Flag:* DURABLE-STATE's item 5
   carried no marked recommendation; this is the minimal reading of P1's needs, named here
   so it can be overridden.

## The contract (P1), from DURABLE-STATE's shape

The backend is a run-folder journal file: append-only lines beside the run. One backend
protocol, the folded list; std-only, nothing fetched.

1. **load(table, identity) → state** — before the plant, when `--journal <file>` is
   passed and the file is not empty: the table loads by the identity check of answer 3;
   a mismatch refuses before any process starts.
2. **record(event) → one line** — the deposit subset written through as it commits:
   a hold armed or released (key, owner, item, armed-at), a grant accepted, a ground
   deposit, an accepted answer (the record line the desk already writes, its durable twin).
3. **rearm(run)** — a restarted run re-arms from the journal: open holds return to the
   desk with key, owner and item; the ground loads its carried deposits (before the seed —
   the seed stays a function of the network file, SPEC 4.5's one-word discipline
   untouched); grants reload arrives in P2, not this pass.

## The build order, with each gate

- **P0 — the folded list.** `GroundState.places` generalised from one `(value, at)` per
  place to the folded list; the visible value, every draw and every `ground` event byte-
  identical (a fold that agrees on origin keeps the value; the draw reads the folded value
  exactly as SPEC 4.5 says). Gate: `cargo fmt --check`, clippy `-D warnings`,
  `cargo test`, conform `spec/fixtures` 224/0/0 on five identical runs, `conform
  py/probes` 10/0/0 — by measurement, not argument.
- **P1 — the journal beside the run.** `--journal <file>` (a side surface, the
  `--grants` pattern): write-through of the deposit subset per the three contract points,
  load with the identity refusal, re-arm on a second run. Tests beside `desk.rs`'s: a run
  killed mid-hold re-arms with the same key/owner; a journal from a different network file
  refuses. Gate: the new tests green; conform unchanged (opt-in surface).

## What this deliberately does not build

P2 (grants and the directory as reloadable journal state), P3 (quiescence reading the
journal), P4 (the peer trail over P2P's `deposit` message), P5 (the SPEC contract text);
multi-writer storage, merges or conflict rules (one table, one owner, refusals not
merges); any database or daemon; presence; replication beyond the peer trail. The
assessment records the rest of the wave order (W2 takes P2+P3; W3 carries P4-P5 and the
0013 contract pass).

## Where it stands

Build on `feat/durable-state` after this record's commit; this section is updated on the
merge report (the gate reruns there; main gains nothing until Kevin's go).
