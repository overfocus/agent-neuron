# Positioning: a brain, a harness and the substrate — Lambda for engineering work

**Status: positioning note, no code (2026-11-10).** Nothing here changes the SPEC, the
schema, TRACE.md, a fixture or a decision record; it records how an outside framing of
agent infrastructure maps onto the family's layers, and the one place this repo
disagrees with it. Companion notes: [DURABLE-STATE.md](DURABLE-STATE.md) (the state
half), [P2P.md](P2P.md) and [decisions/0013](decisions/0013-p2p.md) (the compute half),
[WHITEPAPER.md](WHITEPAPER.md) (what the contract is for). It rides the maintainer's
substrate note of 2026-11-09 (not a doc) that DURABLE-STATE.md cites: a data engine
holding the customer's rules needs persistence that survives the process that produced
it — Lambda for engineering work is a use of that substrate, not a new component in it.

**Accepted (Kevin, 2026-11-10):** record the framing below as the repo's positioning —

1. The post's three layers have owners. (1) The harness is the wrapped program's own
   world (rigswig, or any Claude Code-class cloud harness), kept outside this repo;
   (2) the network file and its engine are this repo, and they hold the edge;
   (3) a brain that breaks down tasks sits one level above the engine as a
   compiler — task in, network file out, run, dissolve — and never inside it.
2. The post's ordering inverts here: "an agent exists only while the job runs" holds
   only because state was made a contract first (DURABLE-STATE.md shape C). Ephemerality
   is downstream of the state contract, not a substitute for it.
3. This note fixes positioning only; any build order (the compiler included) starts as
   its own planning assessment or decision record.

## The framing (the source, in its own words)

An outside post of November 2026 on agent infrastructure, quoted in part:

> It's not that complicated: 1. A great cloud agent harness. 2. A simple abstraction
> over sophisticated automations. 3. A brain that has the ability to break down tasks.
> … we should treat agent infrastructure the same way we treat cloud infrastructure.
> Every moment you spend building and maintaining infrastructure that gives you no
> competitive edge, you could instead be building unique customer-facing features. …
> cloud agents should function like Lambda but for engineering work: you should send a
> task, and an agent plus its computer should exist only while the job runs. They
> should scale up and down on demand.

The post names the form factor but not the layers' owners. This note does that.

## The stack, three ways

| The post's layer | Where it lives in the family | The house claim |
|---|---|---|
| 1. A great cloud agent harness | rigswig (the harness, public). timbre drives the Rust engine binary directly, no harness involved | This repo refuses to be one. A neuron is a program that reads JSON on stdin and writes JSON on stdout and links no library (SPEC 0, 3); the process is the boundary that lets any language join without bindings and makes a cancel a kill (0001 §5). Harnesses are the commoditising infra the post warns against building; the family treats them as swappable compute. |
| 2. A simple abstraction over sophisticated automations | The network file (SPEC 2): per neuron, threshold, staleness, budget, effect; an engine enforces it; 224 conformance fixtures pin it; three engines on main (when this was written; one since [decision 0023](decisions/0023-one-engine.md)) | The quorum, backlog, spend, oncall and colony demos are the "sophisticated automations"; the network file is the simple, diffable, reviewable thing you author. The abstraction is a contract, not a library API: the Python engine written from the text alone passed the same fixtures — the whitepaper's evidence that the text stands by itself. |
| 3. A brain that breaks down tasks | Nothing — deliberately. "Firing rules on the wrapper, not in a controller" (the whitepaper's title) | A brain that decomposes tasks is a compiler one level above the engine: it emits a network file, and the run it starts is ordinary. It never arbitrates, so the multiplayer position keeps its no-central-controller pitch (MULTIPLAYER.md). Lambda's runtime is never the thing that changes; only the code is. |

## The two Lambda halves, already in the tree

- **The compute half** — a task instantiates the computer, and it goes away after.
  Decision 0013's partitioned referees dispatch to peers; placement rides a `--peers`
  side file (the 0010 pattern); the hosted serve over the wire (0013 P1, merged in
  `777db7e`) is the first callable surface. Peers come up when a placement needs them
  and a peer's whole process group is torn down on exit (the demos' discipline); a
  peer-held call's bookkeeping stays local and a cancel is a wire cancel.
- **The state half** — what makes dying compute safe. DURABLE-STATE.md's shape C:
  persistence as a contract the engine takes — the table loaded by identity before the
  seed, the deposit subset (holds, grants, ground, accepted answers) written through to
  a journal beside the run, re-arm on a second run, refusals not merges on an identity
  mismatch. Lambda's real invention is not stateless compute (that half is cheap); it is
  that a run's durable record does not need anyone's server to outlive it. Here the
  journal is lines beside the run and the org directory is the policy repo the org
  already reviews as diffs.

## Where the framing is wrong, one place

"An agent plus its computer should exist only while the job runs" is true of the runs
this family builds only after the job is redefined to include waiting for people.
Decision 0008 puts human neurons on a page; the desk's open holds have to survive a
crash; async play puts human timescales inside one run — issue #30's closed-app case is
exactly a job whose computer was allowed to die with its asks. The computer dies; the
ask survives and re-arms. So the ordering the post implies — ephemerality first, the
brain as the differentiator — inverts here: ephemerality is the cheap half, and by the
post's own rule ("no edge, don't build it") the only infrastructure worth holding is the
contract — the SPEC, the conformance suite, the durable run record. The brain is the
commodity part; a contract a third engine can be written against is the edge.

## What this note deliberately does not decide

- **No build order.** Brain-as-compiler, if wanted, starts as its own planning
  assessment or decision record; this note fixes only where it must sit (one level up,
  not in `Engine`).
- **No SPEC change.** The compile target is today's network file, unchanged; no new
  top-level fields (SPEC 2.8's field budget still applies).
- **No harness work here.** Layer 1's answer stays rigswig plus whatever arrives.
- **No product surface.** Lambda for engineering work is a use of the substrate —
  serve, peers, journal — not a new component in it.
