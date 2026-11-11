# Decision record 0017: the cut a link carries, and readiness that folds outward (2026-11-11)

**Status: decided (Kevin, 2026-11-11), nothing built.** It follows
[decision 0016](0016-the-self-describing-neuron.md): S0 wrote SPEC Appendix E, the map of which
rules one neuron's wrapper can apply alone, and the map found five things 0016 has no answer for.
This record answers the two that stand in front of 0016's S4 (top-level `run` as an ordinary
`peer --network`) and parks the other three with a reason each. Nothing here changes the SPEC, the
schema, TRACE.md or a fixture; each answer below is its own contract commit.

## The problem

**The cut.** SPEC 5.4: emitting a signal with key *K* from sender *S* makes every older signal of
(*S*, *K*) stale, and its unfinished work with it. When the newer signal reaches the same target,
that target can do this alone: it sees a newer (*S*, *K*) input and cuts its own older work. But a
level 1 reply may name a subset of its links (`to`, 3.2), and a network with `ground` forwards
along one drawn link (4.5). Then a target can hold older work for a key whose newer signal went
elsewhere, and no input arrives to tell it. One engine holding every neuron's work hides this. A
wrapper per neuron cannot.

**The start.** SPEC 3.2: a `serve` neuron that does not reach `ready` within its `timeout` makes
the engine refuse to start. One neuron's failure decides for the whole network, and "the engine"
is the only thing that can decide it. With a wrapper per neuron there is no such place.

## The decision

### 1. A keyed signal's key reaches every outgoing link

When a neuron's wrapper emits a signal with a key, each of the neuron's outgoing links carries
something: the targets in the signal's `to` get the signal, and **every other outgoing link gets a
cut**, which carries the sender and the key and nothing else.

- **A cut is the arrival of a newer (sender, key) input without the input.** The target applies
  5.4 exactly as written: waiting work is dropped, running work is cancelled, a started effect call
  is marked and not cut (11.2). A cut for a key the target holds no work for does nothing.
- **The sender keeps no record of where older signals went.** The alternative is a map from each
  key to the targets that may still hold its work, sent a cut only where needed. That is the
  staleness map Appendix C already lists as never pruned, moved to the sender. Sending the cut to
  every other link is stateless and costs one small message per link the signal skips, bounded by
  the neuron's own out-degree.
- **Only two things ever skip a link:** a level 1 reply that names `to`, and a draw of the ground
  (4.5), which any reply that names no `to` makes in a network that selects. An injection always
  goes to every link, and so does an answer that draws nothing, so they send no cut.
- **A cut is not an event.** The `drop` it causes at the target is the record, as today. On one
  host a cut takes no time, so every event lands where it does now and no fixture moves.
- **A cut that cannot be delivered is a staleness miss, not a failure.** The older work finishes
  and its answer forwards. That is the cost taken: 5.4 becomes best effort across a link that is
  down. An effect call is unaffected, since it is never cut.

### 2. Readiness folds outward

A neuron's wrapper is ready when its program is (3.2's `ready`; a level 0 neuron is ready at
once). **A network's wrapper is ready when every neuron it contains is ready.** It applies to a
composed neuron and to the outermost network alike, by the fold rule of 0016 point 2.

- **Nothing a wrapper is not ready for is delivered to it.** A network that is not ready by its
  members' deadlines has failed to start; from outside it is one neuron that did not reach
  `ready`, and its container decides in turn.
- **Today's behaviour is the one-host case.** The outermost network not becoming ready is the run
  the engine refuses to start, and the inside of a composed neuron starting with the engine (2.6)
  is the fold one level down. No fixture moves.
- **Only what a wrapper contains is folded.** An `http` neuron names another party's endpoint; it
  is ready when its own wrapper is, whatever the far side is doing.
- **After the start, nothing changes.** A neuron whose restarts fail five times is failed, and its
  inputs are `start` errors (3.2); the network around it keeps running.
- **A source with a program** (0016 point 6) is a program like any other: its network is not ready
  until it is.

## What it costs

- **One new message between wrappers** (the cut), and a line in SPEC 5.4 saying a key reaches
  every outgoing link. P2P.md's wire sketch already names a `cut`; this fixes who sends it and to
  whom (the sender, to its own links, not a driver to all).
- **5.4 weakens across a dead link**, from "is stale at that moment" to best effort. On one host
  it is as strong as today.
- **SPEC 3.2's sentence is rewritten** from "the engine refuses to start" to the fold, and
  Appendix E's G6 row and the link row change their marks. Neither changes a fixture.
- **No field, no option, no trace event.**

## Parked, with the reason

| Appendix E | What | Why it waits |
|---|---|---|
| G5 | Ids that do not collide | Ids are opaque by rule (TRACE.md) and P2P.md already plans a scheme per peer. It matters only when no single wrapper numbers a run, which is after S4. |
| G7 | The desk: one page for every `human` neuron of a run | Appendix D is informative, and 0016 leaves identity for people undecided. The two are one question and want one record. |
| G8 | Observers: a reader of the whole stream | It depends on the merged trace (0016 point 7, accepted as a direction only). An observer of one wrapper works today. |

## The answers (Kevin, 2026-11-11: every recommendation)

1. **The cut: to every other outgoing link, stateless** (recommended), or only to the targets that
   hold older work, which needs a map at the sender that something must prune. **Accepted: every
   other link, stateless.**
2. **A cut that cannot be delivered is a miss** (recommended), or it is retried until the link
   returns, which needs a queue per link and a rule for when to give up. **Accepted: a miss.**
3. **Readiness folds outward** (recommended), or a neuron that cannot start is simply failed and
   its network runs without it. The second is simpler and changes today's behaviour: a run would
   start with a dead `serve` neuron where it now refuses. **Accepted: the fold.**
4. **The three parked items stay parked** (recommended), or one of them is wanted before S4.
   **Accepted: they stay parked.**

## Where it stands

Written and accepted 2026-11-11. No contract commit has started; both answers are wanted before
0016's S4.
