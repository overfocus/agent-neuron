# Decision record 0020: one loop, fed from outside (2026-11-11)

**Status: decided (Kevin, 2026-11-11: every recommendation).** S4a is built and merged; the rest is not. This is S4
of [decision 0016](0016-the-self-describing-neuron.md): "top-level `run` becomes an ordinary
`peer --network`: no special driver." Reading the code showed that sentence names a destination
and not a step, so this record says what the step is. Nothing here changes the SPEC, the schema
or a fixture until a contract commit of its own.

## What the code is today

The Rust engine has two loops over one core, and they do not meet.

| | `run <file> --inputs <jsonl>` | `peer --network <file>` |
|---|---|---|
| Work comes in | every injection read from a file before the run starts, scheduled in advance | one call at a time over a socket, into the file's `entry` |
| Ends | when nothing is left (6.2 rule 6), then everything is torn down | never: the engine and its `serve` programs persist between calls |
| Concurrency | any number of traces at once | **one call at a time for the whole hosted graph** (a lock) |
| What it may contain | sources, a ground, `human` neurons | none of the three |
| What it reports | the trace, the live stream, answers | one answer; its events are discarded |

Neither is the other with a flag. `run` cannot take work that arrives later, which is the first
hole the 2026-11-11 review found: cron, webhooks and people all assume a process that stays up.
`peer --network` stays up, and can do nothing a real network needs: no source, no trace, no
person, and a second caller waits for the first.

## The decision

**One loop, and work reaches it from outside as it arrives.** The engine already has the seam:
every process, peer and desk reports into one queue that is stamped when read (`host.rs`), and
the loop takes the next thing due. An injection becomes one more thing on that queue. Then
`run` is that loop fed from a file, a hosted graph is that loop fed from a socket, and a network
that stays up is that loop fed from a producer that has not finished.

In four steps, each gated by every existing test passing unchanged:

- **S4a: one step.** *(built, `feat/s4a-one-step`)* The two loops shared about forty lines, copied.
  They are now one function, `Engine::real_step`, and each loop is that step plus what ends it. No
  behaviour changed: `cargo test` 148, conformance on five identical runs, and the four-peer demo.
- **S4b: an injection is a host event, and `run` can take them live.** `run <file> --inputs -`
  reads injections from standard input as lines arrive, each injected when it is read. The run is
  over when standard input has closed and nothing is left: 6.2 rule 6's "no injection is still to
  come" already says so. This is the process that stays up, with no new protocol: anything that
  can write a line is a producer.
- **S4c: a hosted graph is the same loop.** Calls to a `peer --network` graph enter through the
  queue and overlap as traces do in a run; the lock goes. A hosted file may then have sources,
  fed by an `inject` message on the wire, and its events can be read by whoever is entitled to
  them.
- **S4d: `run` is the hosted loop in one process**, and what is left in the driver that is not
  feeding or reading is written down as the list of global things (0016 point 7).

Accepted point 6 of 0016 (a source that carries a program) is S4b's queue fed from a child
process's output. It needs S4b and nothing else from this record.

## What a run that does not end has to settle

S4b makes these questions real. Each has a recommended answer.

1. **When is a live injection injected?** When its line is read; an `at` on the line is refused
   (it means "milliseconds from the start" and the producer cannot know the start). An inputs
   file keeps `at` as it is.
2. **The trace file is written at the end of a run** (`run.rs`), so a run that does not end never
   writes one, and a run that crashes loses it. Recommended: a live run requires `--stream` (the
   same events, one line each, as they happen) and refuses `--trace`.
3. **The engine keeps every event in memory** until the run ends (`engine.rs`, `events`). A live
   run would grow without bound. Recommended: a live run hands each event to the stream and keeps
   none.
4. **Answers on standard output** are printed at the end. Recommended: a live run prints each as
   it happens.
5. **A burst of injections starts a process each.** The SPEC has no limit on level 0 calls at once
   (Appendix B). Recommended: not here. It wants a decision of its own, because a limit that
   queues work is a rule the SPEC must state.

## What it costs

- **The engine core changes** (S4b and S4c touch the loop that the event order of 6.2 lives in).
  The fixtures run on the virtual host, which this does not touch; the real-process tests are the
  gate, and they assert invariants, never exact times.
- **SPEC text** for S4b: what a live producer is, and that `at` belongs to a file of inputs. For
  S4c: the wire, which decision 0013's P5 already owes.
- **The staleness map is still never pruned** (Appendix C). A run that does not end makes that a
  leak in earnest. It is named here and not fixed here.

## Not decided here

- A limit on calls at once (point 5 above).
- Durable state. A live run that dies still loses its open work; `feat/durable-state` is unmerged
  and its order against this is 0016's open question.
- How a caller of a hosted graph is entitled to its events (identity, 0016 point 3).
- The Node and Python engines. They have no process that stays up and gain none here.

## The answers (Kevin, 2026-11-11)

Every recommendation is accepted: the four steps, standard input as the first live door, a live
run that requires `--stream`, refuses `--trace`, keeps no events and prints answers as they happen,
and no `at` on a live injection.

1. **The reading of S4**: one loop fed from outside, in the four steps above (recommended), or
   stop S4 at S4a and treat the rest as a separate project after durable state.
2. **The live door is standard input** (`--inputs -`) first (recommended), or a socket first. A
   socket is S4c's and needs the wire; standard input needs nothing.
3. **A live run requires `--stream`, refuses `--trace`, keeps no events and prints answers as they
   happen** (recommended, points 2 to 4 above).
4. **A live injection may not carry `at`** (recommended), or `at` is taken as a delay from the
   moment the line is read.

## Where it stands

Written and accepted 2026-11-11. S4a is merged. **S4b is built** on `feat/s4b-live-inputs`
(2026-11-11): `run <file> --inputs - --stream <path>`, an injection as a host event, a run that waits
for a producer that has not finished, `tests/live.rs` (5 real-process tests), and a paragraph in
SPEC 4.1 saying what a live producer is.

What building it found:

- **The engine never forgets a finished trace**, which this record did not name. It named the event
  list and the staleness map; the signals (with their payloads) and the work records are kept too,
  and the run-over check reads every work record at every step. Measured on one machine (20,000
  injections of a 1 KB payload through one `serve` neuron): memory grew about 2.6 KB per injection,
  to 59 MiB, and the time per injection from 0.05 ms to 0.09 ms. At one such injection a second
  that is about 220 MiB a day. A live run is good for hours or days, not for ever.
- **A refused live injection does not end the run**, accepted (Kevin, 2026-11-11): a file of inputs with a bad entry
  refuses the whole run before it starts, but a live producer's bad line arrives after others have
  been taken, and other producers may depend on the run. It is one line on stderr.
- **The SPEC paragraph makes a live producer optional for an engine** (MAY), as confinement is.
  Accepted (Kevin, 2026-11-11).

**Next, before S4c (accepted, Kevin, 2026-11-11):** forgetting finished traces. When a trace is finished (7.2) nothing can refer
to its signals or its work again, so they can go; the run-over check becomes a count. It is the
difference between a run that stays up and one that can.

**Forgetting is built** on `feat/s4-forget` (2026-11-11). A finished trace whose last work is done
loses its signals and payloads, its pool, its staleness entries and its threshold groups, the
inside of its composed calls included; a hosted call is forgotten the same way; and every scan for
open work starts at the first record that may still be open. Nothing a run reports changed.

- **The same measurement:** memory per injection fell from about 2.6 KB to about 0.33 KB and no
  longer depends on the payload's size (10.6 MiB after 20,000 injections, against 59 MiB); the time
  per injection stays at 0.05 ms where it had climbed to 0.09.
- **The fixtures now test it.** The conformance runner fails a fixture if the engine still holds
  anything of its traces when the run is over. All 246 pass; with forgetting switched off, 211 fail
  on that check, so it bites.
- **What is left, and why.** The work records stay, emptied. They are named by their place in a
  list, and a wake-up armed long ago may still name one; it finds it done and does nothing. Freeing
  them means naming work some other way, through the whole engine. About 0.33 KB a call, or 29 MiB a
  day at one a second: a run is good for weeks now, and still not for ever.

**The forgetting step is merged** (Kevin, 2026-11-11), and S4c follows it.

**S4c, first half, is built and merged** (2026-11-11): the lock is gone. A hosted
graph's engine runs the run's loop on a thread of its own (`Engine::serve_calls`); a connection hands
it a call and waits for that call's outcome; calls from different connections overlap as traces do
in a run. The wire is unchanged, and a connection still carries one call at a time. Four
real-process tests in `tests/peers.rs`, each of which fails on the engine before this step.

What building it found, all three in the engine before this step:

- **A hosted graph that sat idle failed its next call at once.** The engine's clock stood still
  between calls, so a call that arrived after an idle gap longer than its timeout had its timeout
  armed in the past. A call's clock now starts when it arrives. Any hosted graph called less often
  than its timeout had this.
- **A hosted graph that was quiet was reported as failed.** An inside that settled with no reply from
  `exit` (2.6 step 3) came back over the wire as an `inner` error. It is quiet now, event for event
  what the same graph gives run locally.
- **A deadline could fire a wake-up early.** When the engine's wait ran to a deadline that came
  before its next wake-up, the wake-up fired at once: a run given a time limit reported one spurious
  call timeout as it stopped. The deadline's instant is now the deadline's.

And one thing found and not fixed: **a burst the worker cannot keep up with fails badly.** 3,000
calls sent 3 ms apart to a hosted graph of one program: 59 answered, the rest were reset or timed
out after their whole 20 seconds, on the engine before this step and after it alike. At 12 ms apart
all 6,000 of three rounds answered. This is point 5 above (a limit on calls at once), still a
decision of its own.

Memory of the hosted engine over those 6,000 calls grew by about 0.5 KB a call, the emptied work
records named above.

**S4c, second half: every recommendation below is accepted (Kevin, 2026-11-11); not built yet.** A hosted file with sources, fed over the
wire, and events a caller can read are both changes to the wire, and neither has a text to build to:

1. **What an `inject` on the wire is.** Recommended: a line `{"type":"inject","source","payload",
   "replaces"?,"budget"?}` on a connection to a `peer --network` graph, the same object a live
   producer writes to standard input (4.1), injected when it is read, with no reply; a refused one
   is a line on the worker's stderr, as a live producer's is.
2. **Whether one hosted file can be both** called as a neuron and fed at its sources. Recommended:
   yes. `entry` and `exit` are needed only to be called; sources only to be fed; a file may declare
   either or both, and a `call` to a file with no `entry` is refused as it is today.
3. **Where a hosted graph's own outer events go.** Today they are dropped. Recommended:
   `peer --network <file> --stream <path>`, the worker's own file on its own machine, the same lines
   a live run writes. Reading events *over the wire* waits for identity (0016 point 3), because it
   is the first thing a connection could read that is not its own call's answer.
4. **Whether the hosted file may then declare a `ground`**. Recommended: not yet. 2.6 refuses it in a
   composed file, and a file that is fed and never called is no longer only composed; that wants the
   SPEC's word on what a hosted file is, which is the wire's contract commit (0013's P5).

**S4c, second half, is built** on `feat/s4c-inject` (2026-11-11): the `inject` line, a file that is
called, fed or both, and `peer --network <file> --stream <path>`. Five more real-process tests in
`tests/peers.rs`. The SPEC gains one sentence in Appendix C; the wire still has no text of its own
(0013's P5).

How it is built, which the answers above did not settle: **the hosted file is the worker's own outer
network.** Before, the file sat inside a wrapper as a composed neuron's inside, so nothing of it was
ever reported and it could have no source. Now the file is instance 0 and a caller enters through
one node the file does not declare (`__call`), a composed neuron whose inside is the file. A call is
still 2.6's composed call, and what the caller is told is as sealed as before; the worker's own
stream shows the file at work, and a call's trace there starts with a signal from `__call`.

What building it found:

- **The work that answered a composed call was itself cut.** When `exit` replied, the engine
  cancelled the rest of the call's inside, the replying work included, before marking it done. It
  was never seen, because the inside is sealed. It is done first now. Conformance is unchanged.
- **A reading that wants a yes or no: the cut of the rest is in the worker's stream.** When `exit`
  answers a call, the work still running for that call is cut (2.6 step 2, "without reports"). To the
  caller nothing is reported, as before. In the worker's own stream the cut shows as it would in any
  run: a `drop`, a `cancel` note, a `term` note, and the drop's reason is the word `replaced`, which
  is the only reason a cut has today. That word is wrong here; a right one is a SPEC change.
- **`__call` is a name in the stream that no file declares.** It cannot collide (an id may not begin
  with an underscore), and a reader that looks every `from` up in the file will not find it.

Not done: S4d; an `inject` cannot say which connection it came from (identity, 0016 point 3); the
MCP route still cannot host a file.

