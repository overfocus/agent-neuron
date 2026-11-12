# Decision record 0021: a limit on calls at once (2026-11-11)

**Status: decided 2026-11-12, every recommendation accepted. Built: L1, the engine cap, and L2, the option (on `feat/l2-max-inflight`, not merged; see "L2 as built").** [Decision 0020](0020-one-loop-fed-from-outside.md)
left this open twice (its point 5, and the burst it reported). A run that stays up and a file that
other engines call both take work at a rate somebody else chooses, so the question is no longer
academic. Nothing here changes the SPEC, the schema or a fixture until a contract commit of its own
(L2's is that commit).

## What is true today

**Level 1 has a limit and a rule for it.** A `serve` program says how many calls it takes at once
(`maxInflight` in its `ready`, 3.2). The rest are *queued in order*, and "queued" is a complete state
in the SPEC: a queued call is dropped if it goes stale (5.4, phase `waiting`), reserves from the
budget only when its slot frees (10.3), has not started (so it is never `unknown`, 11), and keeps its
trace and its run unfinished (6.2 rule 6, 7.2). Its `timeout` counts from its start, so a queued
call waits for as long as it takes.

**Nothing else has a limit.** A `run` neuron starts a process for every input the moment it arrives;
an `http` neuron opens a request; a `network` neuron starts a composed call. Appendix B lists
"concurrency limits at level 0" as out of version 0.1.

## What the probes found

On the development machine (macOS, 2,666 processes allowed per user), 2026-11-11. Each call is a
shell that sleeps 0.2 s, so one call is two processes.

| Probe | Result |
|---|---|
| 400 calls at once to one `run` neuron | all answered, 0.9 s |
| 900 calls at once | all answered, 1.5 s |
| 400 at once, with the process limit lowered to about 150 above what was already running | **84 answered.** 256 failed `exit 127: Resource temporarily unavailable` (the engine could not start the program); 60 failed `exit 129` (the program started and could not start its own child) |
| A hosted file: 300 and 1,500 calls at once; 1,000 and 3,000 calls 3 ms apart | all answered |

Three things follow.

1. **Below the machine's limit the engine has no trouble**, at any burst tried.
2. **At the limit, failure is indiscriminate.** Which calls fail is a matter of timing. Some fail
   inside the program, after it started, where the engine can do nothing and an `effect` neuron's
   failure would be an `unknown`. Everything else the user runs on that machine fails to start
   processes too.
3. **A correction to 0020.** It reported a burst of 3,000 calls to a hosted file with 59 answered.
   That did not reproduce in five later attempts, on the engine as it was then or as it is now. Its
   cause is unknown; other engines were running on the machine at the time, and the likeliest reading
   is that the machine was near its process limit. 0020 is corrected to say so.

So the failure is not the engine's loop. It is that the engine will ask the machine for more than
the machine has, and that nothing in a network file or on the command line can tell it not to.

## The proposal

Two limits, because there are two different facts.

### 1. A cap for the whole engine, given on the command line

`run --at-once <n>` and `peer --at-once <n>`: at most *n* level 0 programs running in this engine at
once, across every neuron. A call that would exceed it is **queued**, in arrival order, and starts
when a program ends. Without the flag there is no cap, as today.

This is the one that answers the probes. How many processes a machine can hold is a fact about the
machine, like where it keeps its interpreters (`--confine`) or which peer runs what (`--peers`); a
network file never states it, because it differs from one machine to the next.

- **It counts processes the engine started**, including one that was cut and is still inside its
  kill grace: the cap is about the machine, and that process is still on it.
- **It does not count** `serve` programs (started once, counted by their own slots), `http` requests
  or people.
- **Queued means what it means at level 1**, every rule above. Nothing new is reported: a queued
  call simply has not started yet.

### 2. A limit per neuron, in the file: `maxInflight` as an option

`"maxInflight": 4` on a `run`, `http` or `network` neuron: at most that many calls of this neuron
running at once, the rest queued in order. No default: without it there is no limit, as today.

This is the fact about the program, not the machine: a script that takes a lock, an API with a rate
its owner knows, a model that fits twice in memory. It is the word level 1 already uses for the same
thing, said by the file because a level 0 program has no `ready` to say it in.

- **It is the neuron's own** (decision 0019's split): it holds wherever the program runs. A using
  entry may lower it and never raise it, the fourth option an entry may narrow.
- **A tenth option.** 0016 reopened the budget for `may`; this spends one more.
- **Not on `serve`** (the program says its own) and **not on `human`** (a person's queue is the
  desk's).

### 3. Over a limit, work waits; it is not refused

Both limits queue. The alternative is an error (`busy`, say) for the call that does not fit, which
pushes the waiting onto whoever sent it, and in this model the sender is another wrapper with no
retry rule. Level 1 already queues, and one rule is better than two.

### 4. The wait has no time limit, and that is named, not fixed

A queued call's `timeout` has not started. Under a rate that stays above what the limit lets
through, the queue grows without end, in memory, for as long as the run lives. Level 1 has had this
since version 0.

What empties a queue today is staleness: a producer that sends on a `replaces` key has its older
queued work dropped by the newer (5.4). For the cases that most need a limit (a webhook that fires
too often, a cron that overlaps itself) that is the right answer and it already works.

A bound on the queue itself (a length, or an age) is a rule the SPEC would have to state for level 1
as well. It is left for the evidence of a real overload.

### 5. A program that cannot be started stays what it is

With no cap set, or a cap set too high, a spawn the machine refuses is `exit 127`, an error, and for
an `effect` neuron an error and not an `unknown` (it never started). That is today's behaviour and
it is right.

## What it costs

- **The engine cap** (point 1): the Rust engine only, real-process tests only, off by default, so
  conformance is untouched. One sentence in the SPEC, allowing an engine to queue a level 0 call for
  a limit of its own, with level 1's meaning of queued.
- **The option** (point 2): a contract commit. The schema (both files), SPEC 2.4, 2.10, 5.1 and
  Appendix B and E, a fixture area, and the queue in all three engines. The Rust and TypeScript
  engines have the level 1 queue to build on.
- **Fairness is arrival order and nothing more.** One neuron that floods the engine cap makes every
  other neuron wait behind it. The per-neuron option is the tool for that, which is one reason to
  have both.

## Not decided here

- **A bound on a queue** (point 4).
- **A cap on connections** to a peer. Each connection is a thread, and nothing limits them.
- **Limits by trace or by caller** (one noisy producer among several): identity first, 0016 point 3.
- **Backpressure to a live producer.** Standard input and the wire take lines as fast as they are
  written; the engine never asks a producer to slow down.

## The build order, with each gate

- **L1: the engine cap.** Gate: with `--at-once 5`, a burst of 100 calls never has more than 5
  programs alive (measured by the programs themselves), all 100 answer, a queued call that goes
  stale is dropped as `waiting` and never starts, and a run with the flag unset is unchanged. The
  third probe above, with the cap set below the lowered limit, answers every call.
- **L2: the option**, as its own contract commit with the three engines. Gate: fixtures for a
  `run` neuron with `maxInflight` 1 and 2 (order, a stale queued call, the budget reserved when the
  call starts), each refusal, and narrowing from a neuron's file.

## Decided (2026-11-12)

Every recommendation was accepted.

1. **Both limits**: the engine cap and the option.
2. **The cap is a command-line flag, `--at-once <n>`**, on `run` and `peer`.
3. **The option reuses the word `maxInflight`.**
4. **Over a limit, a call waits.** It is not refused.
5. **No bound on a queue yet.**
6. **The engine cap first** (L1), then the option as its own contract commit (L2).

## L1 as built (2026-11-12)

`run --at-once <n>` and `peer --at-once <n>` in the Rust engine (`rust-engine/README.md`), and one
informative sentence in SPEC Appendix C. No schema, fixture or normative text changed.

The gate, measured:

| Check | Result |
|---|---|
| 100 calls at once across two neurons, `--at-once 5`, counted by the programs | never more than 5 alive; all 100 answered; no error |
| The same burst without the flag | all at once, as before |
| A call replaced while it waits for a place | one `drop`, `replaced`, phase `waiting`; its program never started; no cancel |
| A hosted file and a program worker, each `--at-once 1`, three calls at once | one program at a time; all answered |
| The third probe again: 400 at once, process limit 150 above what was running | without the cap 83 answered; with `--at-once 50` all 400, in 2.8 s |

Three things the build settled that the proposal left loose.

1. **A place is held until the program is off the machine.** For a call that ended by itself that
   is its exit; for a cut call it is the end of its group, so a program that ignores SIGTERM holds
   its place through the kill grace, as point 1 said it should.
2. **A placed call is the peer's.** The driver does not count a call it sent to a peer; the peer
   counts it under its own `--at-once`. `peer --serve` refuses the flag.
3. **A caller's clock does not wait for the callee's queue.** A driver's timeout for a placed call
   starts when the call is sent, so waiting for a place at a program worker counts against it. To
   start that clock at the program's start, the wire would need a word for "started"; it has none,
   and none is added here. A caller of a hosted file is a composed call, whose timeout has always
   covered its whole inside. **Open, for the wire's own SPEC text.**

## L2 as built (2026-11-12)

The contract and the three engines in one commit, on `feat/l2-max-inflight`: `maxInflight` in both
schemas (`run`, `http` and `network` neurons, and the entry that names a neuron's file; `run` and
`http` neuron files), SPEC 2.4, 2.6, 2.7, 2.8, 2.10, 5.1, 6.2 rule 9, 6.3, 10.3, 11.2 and Appendix B,
C and E, CONFORMANCE 4.1, and 26 fixtures in `spec/fixtures/maxinflight/`. The TypeScript, Python
and Rust engines each hold the queue; all three pass every fixture, the 246 that were there
untouched. A neuron without the option takes the path it took.

The gate, by fixture:

| Check | Fixture (`maxinflight/`) |
|---|---|
| `maxInflight` 1: arrival order, a waiting call keeps its trace unfinished | `one-at-a-time-in-arrival-order` |
| `maxInflight` 2 | `two-at-once-the-third-waits` |
| a stale waiting call is dropped, phase `waiting`, and never starts | `a-waiting-call-that-goes-stale-is-dropped`, `a-waiting-effect-call-is-dropped-when-stale` |
| the budget is reserved when the call starts | `a-waiting-call-reserves-when-it-starts`, `a-waiting-call-is-refused-for-budget-only-when-it-starts`, `a-waiting-call-refused-as-it-starts-gives-its-place-to-the-next` |
| `timeout` counts from the start | `timeout-counts-from-the-start-of-a-waiting-call` |
| each refusal | the eight `refuses-*`: on `serve`, `human` and a source; zero; not an integer; an entry wider than its file; in a `serve` neuron's file; on an entry that names a `serve` neuron |
| narrowing from a neuron's file | `a-narrowed-max-inflight-is-the-one-that-holds`, `the-neurons-own-max-inflight-stands-when-not-narrowed`, `an-entry-may-set-a-limit-its-neurons-file-does-not` |

Each refusal was checked to fire on its own rule in each engine's `check`. Real processes: one test
in `test/engine.test.ts`, four in `rust-engine/tests/peers.rs` (the programs' own count under
`maxInflight` 2; both limits on one call; a stale waiting call; a placed neuron) and one in
`rust-engine/tests/http.rs` (an `http` neuron, one request at a time).

What the build settled that this record left loose. Each follows level 1 or L1 where they speak, and
each is pinned by a fixture unless it says otherwise. **All are for the maintainer to confirm.**

1. **Where a waiting call starts.** At the end of the step in which a place came free: after every
   event the step caused, before any other work due at that instant (SPEC 6.2 rule 9). This is where
   level 1's queued call starts when a reply frees its slot, and where L1's `cap_drain` runs. A place
   is not taken inside the step that frees it, so an input delivered later in that step waits behind
   the calls already waiting (`a-place-freed-in-a-step-is-taken-at-the-end-of-the-step`).
2. **A place is held until the program is over**, as a cancelled level 1 call keeps its slot and as
   L1 holds a place: a cut `run` call through its kill grace
   (`a-cut-call-keeps-its-place-until-its-program-is-gone`). An `http` call's place is free when its
   request is over or cancelled, a `network` call's when the composed neuron has replied or was cut
   (no fixture for the `http` half: no conformance engine but Rust runs `http`).
3. **One order for every neuron's waiting calls.** When places of several neurons come free in one
   step, the call that arrived first starts first; a neuron that is full never holds up another's
   (`calls-waiting-at-one-neuron-do-not-hold-up-another`). Which of two neurons' calls starts first in
   that case has no fixture: it shows only in the order of wake-ups set in one step.
4. **The limit is per neuron per instance.** A neuron inside a composed neuron counts the calls of
   every call to that instance together, as an inner level 1 neuron's slots are shared
   (`inside-a-composed-neuron-the-limit-is-shared-by-its-calls`).
5. **A neuron file that states no `maxInflight` has no limit, so an entry may set any**
   (`an-entry-may-set-a-limit-its-neurons-file-does-not`). The other reading (an entry may restate
   only what the file states) would make the option unusable on a neuron somebody else wrote.
6. **Where the option may be written.** In the file of a `run` or `http` neuron; inline on `run`,
   `http` and `network`; on an entry only when its file is a `run` or `http` neuron (0019's rule that
   an option is allowed only where it is on an inline neuron of the file's kind).
7. **A threshold call waits as one call**: its inputs crossed and left their group, so none expires
   while the call waits (`a-threshold-call-waits-as-one-call`).
8. **A call refused as it starts gives its place on** within the same end of step
   (`a-waiting-call-refused-as-it-starts-gives-its-place-to-the-next`).
9. **Both limits on one call** (Rust, real processes only): the call takes its neuron's place first
   and keeps it while it waits for the engine's cap. A neuron placed on a peer has its `maxInflight`
   held by the driver; the peer's `--at-once` still counts the process.
10. **A composed call's `timeout` covers its inside's waiting**, as it always covered everything
    inside it: a call waiting for an inner neuron's place has started as far as the outer call is
    concerned. Only a call waiting for its own neuron's place has a clock that has not started.

One change to L1 on the way: `cap_drain` now checks for finished traces after each start, so a call
refused for its budget as it leaves the cap's queue reports its trace's `finished` then and not at
the next event.

Not done: a demo and its row in DEMOS.md; the trace header does not list `maxInflight` among a
neuron's `options` (it lists `threshold`, `within` and `maxHops` only, as before); the wire still has
no word for "started", so point 3 of "L1 as built" stands for a placed neuron that waits at a
worker's cap.
