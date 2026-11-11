# Decision record 0016: the self-describing neuron, `may`, and the engine as a wrapper (2026-11-11)

**Status: decided (Kevin, 2026-11-11), nothing built.** This record reopens the field budget, names
the one option it spends, fixes how a neuron describes itself, and reverses one earlier decision.
Nothing here changes the SPEC, the schema, TRACE.md or a fixture: each build step below is its own
contract commit (SPEC text, schema and fixtures together), and each starts after this record is on
`main`.

## The problem

A review on 2026-11-11 asked what stands between version 0.1 and a network that stays up: many
agents, networks of agents, API and MCP connections, scheduled work, files, people and webhooks.
Three findings carry this record.

1. **A neuron is an arbitrary process with the full rights of whoever started the engine.** `env`
   limits the variables a program sees (SPEC 2.4), not the files it reads, the hosts it reaches or
   the ports it opens. A network that passes `HOME` to a neuron has handed it everything. Nothing in
   the file says what a neuron may touch, so a reviewer cannot diff it and an engine cannot enforce
   it.
2. **A composed neuron cannot act in the world.** A file used as a composed neuron MUST NOT contain
   an `effect` (SPEC 11.5), because sealing hides what the inside did. So every effect of every
   sub-network has to be lifted into the outermost file, and a team behind one wrapper can only
   report.
3. **The engine is a controller.** The whitepaper's claim is firing rules on the wrapper, not in a
   controller. The SPEC nearly keeps it: `threshold`, `within`, `maxHops`, `timeout`, `effect`
   and forwarding along a neuron's own links are all local to the neuron that receives the input,
   and staleness by (sender, key) is too, except where the newer signal never reaches a target
   that holds older work (SPEC Appendix E; corrected when S0 wrote the map). The Rust engine does not: one heap of wake-ups serves the whole graph,
   and under `--peers` the driver routes every signal (P2P.md's shape A, a hub by its own table).

Kevin's direction, the same day: sandbox and identity come first; open the field budget; neurons
describe themselves by composition only, never by inheritance; and for federation, the existence
of an engine that owns a run is an anti-pattern to the project's goals.

## The decision

### 1. The field budget is reopened, for one option: `may`

`effect` was "the eighth and the last planned" option (SPEC 2.8, CONTRIBUTING). A ninth is
admitted: **`may`**, what the neuron's program is allowed to touch. It is an object, shaped like
`cost`, with four members, each optional:

| Member | Value | Grants |
|---|---|---|
| `connect` | array of `host:port` | outgoing connections to those, and no others |
| `listen` | array of `address:port` | accepting connections on those, and no others |
| `read` | array of paths | reading those files or trees |
| `write` | array of paths | writing those files or trees |

- **Absent grants nothing**, in every member: the default `env` already has. A neuron with no `may`
  reaches no host, opens no port, and reads and writes nothing beyond what running its command
  needs.
- **One option, not four.** It costs the budget one place, and the four members are one question
  asked four ways.
- **It passes the budget's test by the precedent of `timeout`, `maxHops`, `cost` and `effect`**
  (decisions 0006, 0007): a limit is the engine's to apply, not something a program can declare
  for itself. A program cannot be trusted to jail itself.
- **Values are literals**, as everywhere in the file: no patterns that read a payload, no
  expressions. Whether a path or a host may carry a wildcard is a build question (below).
- **Who is calling is not `may`'s business.** A neuron that listens checks its own callers (a
  signature, a token): that is deciding, and deciders are programs. `may` says only what the
  neuron itself is allowed to do.

### 2. A neuron describes itself

- **A neuron is its program and one file that says what it is**: its kind, its options, its `may`,
  its `description`. A network file is that same file for a neuron whose inside is other neurons;
  a composed neuron has worked this way since version 0 (2.6).
- **The file that uses a neuron names it and links it.** It may **narrow** what the neuron
  declares and never widen it: a lower `cost`, a shorter `timeout`, a smaller `may`. The rule is
  10.5's, generalised: neither side can raise the other, and the lower wins.
- **Composition only.** Nothing arrives from above: no `extends`, no templates, no file-level
  default that falls onto the neurons inside it. A neuron has what it declares, or the fold of what
  it contains.
- **A container's description is the fold of its inside.** What a composed neuron declares must
  cover what its neurons declare; `check` reports an error where it does not. The `may` of a
  composed neuron is at least the union of its neurons' `may`.
- **The description is static.** It is read and enforced before the program starts, so it is a
  file, never something a program says in its `ready` line. `ready` may keep carrying what only a
  running program knows (`maxInflight`).

### 3. The no-crypto decision is reversed

Accepted on 2026-11-10 (the spec-vs-harness assessment, decision 2) and stated in decision 0013
(point 4 and its security posture): the Rust engine stays std-only, with no TLS and no crypto, and
peers trust a pre-shared token inside a trusted network. **That no longer binds.** Wrappers that
federate must authenticate each other, and a neuron that listens faces whatever can reach it;
neither is defensible on a shared token.

What replaces it is not decided here (below). Until it is built, the existing peer layer keeps its
loopback tokens and says so, and `https` stays refused.

### 4. The engine grows into a wrapper; it is not rewritten

The destination: **the unit that runs is one neuron under its own wrapper.** A network is a neuron
whose wrapper contains wrappers, and the outermost one is nothing special. Wrappers on one host may
share a process as an optimisation, never as an authority. The test of the shape: a neuron can move
to another host and behave the same.

The path, in order:

1. **Mark every rule of SPEC 4 to 11 as wrapper-local or global**, in the SPEC's own text. No
   behaviour changes; conformance stays as it is. This is the map the later steps read.
2. **Top-level `run` becomes an ordinary `peer --network`**: no special driver. What is left in
   the driver after that is, by construction, the list of global things.
3. **Each global thing gets a local answer** (point 7), each its own contract commit.

A rewrite is refused: the fixtures are the contract, and the engine that passes them is the asset.

### 5. `effect` folds outward

By the fold rule of point 2, a composed neuron that contains an effect is itself an effect at its
boundary: a started call is never cut by staleness, and an ambiguous failure is `unknown`, not an
error. SPEC 11.5's ban (a composed file MUST NOT contain an `effect`) is lifted in this point's
contract commit, which closes finding 2: a team behind one wrapper may act in the world, and the
outer file shows that it can.

The contract commit has to answer what 11.5 was protecting: what "started" means for a composed
call (the input entering the inside, or the first inner effect call starting), and how an inner
`unknown` reaches the boundary without breaking sealing (2.6).

### 6. A source may carry a program

`"source": [command]` beside `"source": true`. The program is the producer of SPEC 4.1, moved
inside the file: it emits payloads unprompted, each a root signal, with the replaces key it
chooses. It is how a neuron becomes a listener or a clock, and a live source program keeps the run
open (6.2 rule 6 already says "no injection is still to come"). It spends no field: the kind's
value widens. It adds one level 1 message, an unsolicited emit, which an old engine ignores as an
unknown type. A source with a program takes `may`, `env` and `cwd` like any program; which other
options it takes is the contract commit's to say.

### 7. The global things get local answers

Four things only a whole-graph engine can see today. Each gets an answer one wrapper can give.

| Global today | The local answer | Standing |
|---|---|---|
| The budget pool of a trace (10.2, 10.3) | The wrapper of the source that started the trace holds the pool. A call's wrapper asks it to reserve before the call starts; it grants or refuses. An owner per trace that does arithmetic and never routes | accepted |
| "The run is over" (6.2 rule 6) | Only traces finish. A wrapper tells its sender when its own call has ended and everything it forwarded to has reported done; the source's wrapper then writes `finished`. A batch run is over when no source is live and every trace is finished | accepted |
| One total event order (6.2) | Order is normative within a wrapper. A whole-network trace is a merge of the wrappers' lists by stamp, an event never before the one that caused it | accepted as the direction |
| The ground (4.5) | Deposits gossiped as a list that merges (P2P.md P3), and a generator per neuron in place of the engine's one | its own decision record |

- **The pool's owner keeps 10.3 as written**, so the `budget/` fixtures stand. The alternative, an
  allowance carried on the signal and split at each fan-out, is refused: with 10 left and two
  targets costing 6 each, a split refuses both where today the first runs. The costs taken: a round
  trip per call start, and a call whose owner cannot be reached is refused (it fails closed). The
  owner is also where a limit per day would sit, since a source sees every trace it starts; that
  limit is not decided here.
- **The done report rides the path a reply already takes** (4.3). On one host it takes no time, so
  `finished` lands at the instant it does today. A host that goes silent needs a new outcome, a
  trace that finishes saying part of it is unaccounted for; the contract commit names it.
- **The merge equals today's list on one host**, so the fixtures stay the test. Across hosts two
  runs of the same inputs may interleave differently, as real runs already do under load. What is
  given up is a trace as one authoritative list; TRACE.md gains the merged reading.
- **The ground is the one answer that changes pinned numbers**: SPEC 4.5 pins one generator per
  engine and the `ground/` fixtures assume it. It is not decided here.

## What it costs

- **The field budget:** a neuron's options go from at most 8 to at most 9. SPEC 2.8 and
  CONTRIBUTING's "the eighth and the last planned" are rewritten in `may`'s contract commit.
  `may` is the last planned, in the same sense `effect` was.
- **Every engine's `check`** learns `may`'s shape, the narrowing rule and the fold rule. These are
  refusals a fixture can pin.
- **Enforcement is not fixture-shaped.** Fixtures script programs and start no process, so the
  jail itself is pinned by real-process tests per engine and per platform, as the process layer is
  today. The TypeScript and Python engines can check `may`; whether they enforce it is a scope line
  to write, not to assume.
- **Enforcement is platform work**, different on Linux and macOS, in an engine that is POSIX only.
- **The std-only property goes**, or is narrowed to the conformance core. The README, the
  whitepaper and the Rust engine's README all lean on "nothing fetched"; they change when the
  replacement is built, not before.
- **SPEC 11.5 is rewritten** (point 5), with the fixtures that pin its refusal.
- **The level 1 protocol gains one message** (point 6), and `source` stops meaning "no program".

Points 5, 6 and 7 were proposed in the review as flags and accepted by Kevin on 2026-11-11.

## Not decided here

- **How a file names a neuron that describes itself.** Today a leaf neuron's kind and options are
  written inline, and only a composed neuron is named by path. Whether a new kind field names any
  neuron's file, or `network` widens, is the first build question; it may cost a field, and that
  cost is stated then.
- **What replaces std-only:** a TLS dependency, signed messages between wrappers, or both; how a
  wrapper holds a key; how the network file states which ids are trusted at which addresses.
- **The sandbox mechanism** per platform, and what an engine does on a platform where it cannot
  enforce `may` (refuse to run is the reading that fails closed).
- **`may`'s edges:** wildcards in hosts and paths, how `cwd` and the command's own files are
  reached, whether `env` should ever move inside `may` (it stays where it is for now).
- **The ground's local answer** (point 7): gossiped deposits and a generator per neuron rewrite
  fixtures, so they take a record of their own.
- **A budget per day or per source** (SPEC 10.8 keeps it out of version 0.1).
- **Identity for people** (the desk off loopback, who a person is) and for outside callers. This
  record covers what a neuron may do, not who anyone is.
- **The order between this work and the waves** accepted on 2026-11-10. Durable state (0015) is
  built on `feat/durable-state` and unmerged; this record does not move it.

## The build order, with each gate

- **S0: the map.** SPEC 4 to 11, each rule marked wrapper-local or global. Gate: a docs commit;
  `conform spec/fixtures` unchanged on every engine.
- **S1: `may` in the file.** Decision-record-first is this record. SPEC 2.4 and 2.7, the schema,
  fixtures for the shape, the narrowing refusal and the fold refusal. Gate: all three engines pass
  the new fixtures; `check` tests in TypeScript and Rust.
- **S2: `may` enforced.** The Rust engine applies it to `run` and `serve` on one platform first.
  Gate: real-process tests that a neuron with no `may` cannot connect, listen, or read or write
  outside its command's needs, and that each member grants exactly what it names.
- **S3: the neuron file.** After its naming question is answered. Gate: a network built from
  neurons that describe themselves equals the inline one, event for event.
- **S4: `run` as `peer --network`.** Gate: every existing real-process and peers test unchanged;
  what remains in the driver is listed.

Points 5 and 6 are contract commits of their own, outside this order: the source program can land
any time after S1 (it takes `may`), and the effect fold after S3 (it reads the fold rule). Point
7's three accepted answers follow S4, which is what lists them.

## Where it stands

Written and accepted 2026-11-11. **S0 is written** on `feat/s0-local-global-map`: SPEC Appendix E
(informative) marks each runtime rule local, link, global or static. It found four global things
beyond point 7's four: ids that must not collide, the start (one `serve` neuron that is not ready
refuses the whole run, 3.2), the desk's one page per run, and observers. It also found one cut a
link must carry: 5.4's, to a target the newer signal never reaches. None of the five has an answer
in this record; [decision 0017](0017-the-cut-a-link-carries-and-readiness.md) answers the start and
the cut and parks the other three. S0 merged to `main` on Kevin's go, 2026-11-11.
