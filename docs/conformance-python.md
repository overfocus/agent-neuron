# The Python engine: conformance findings

A second engine, [`py/agent_neuron/`](../py/agent_neuron/), written from [SPEC.md](SPEC.md),
[CONFORMANCE.md](CONFORMANCE.md), the fixtures and the decision records, without reading or porting
`src/engine/`. Python 3.11 or later, standard library only (it also runs on 3.10 and 3.13). Run it with
`npm run conformance:py`.

## Result

**193 passed, 0 failed, 0 pending** on `spec/fixtures`, identical on repeated runs. Also 10 of 10 on
[`py/probes/`](../py/probes/) (extra fixtures written while probing, below), which the TypeScript engine
passes too.

How it got there, so the number is read honestly: the first complete run passed 177 of 193. The 16 failures
were my bugs and four places where the fixtures pin more than the text says (the first section below); none
was fixed by loosening the runner. The runner is the one CONFORMANCE describes, including the two-run
observer rule (8.5), the live-stream consumer that mutates the events it is given and may throw, and
prefix comparison of messages. A deliberately broken engine does fail it (a 1000 ms kill grace fails six
fixtures, a restart delay counted from the wrong instant fails one, and so on).

**Failing fixtures: none.**

## What this engine does not do

- **It starts no process.** CONFORMANCE says a fixture never starts one, and no fixture needs it, so the
  engine has only the virtual host: scripted programs, a virtual clock, scripted process groups. A real
  process layer (stdin/stdout pipes, `setsid`, signals, the environment of 3.1) is not written; nothing here
  proves SPEC 3 for real bytes.
- `http` and `human` neurons are refused at start, as the TypeScript engine does.
- It has `conform` only: no `check` command (the validator in `check.py` is used internally for the refusal
  fixtures and covers the errors of 2.7, not the warnings), no `run`, no trace file, no `--stream`.
- **Composed neuron and rule 7 (6.2):** an inner reply due at the instant of the composed neuron's `timeout` loses
  to the timeout, against the text. See ambiguity 3: the TypeScript engine does the same.

## Ambiguities and gaps in the specification

Most important first. "Assumed" is what this engine does. Where I used the TypeScript engine, I used it as a
black box: `node dist/engine/cli.js conform <dir>` on probe fixtures I wrote, never its source.

### 1. A restart that is due while a kill grace runs (SPEC 3.2, 6.2 rule 6, 6.3; CONFORMANCE 9)

- Fixture: `failures/l1-process-exits-and-leaves-a-survivor`.
- The text: the restart delay is `min(100 * 2^f, 5000)` "before each restart"; "a pending restart delay ... does not keep
  the run open"; a kill grace under way "keeps the run open". CONFORMANCE 9 lists "a restart delay and a pending kill
  grace racing" as **not covered**.
- The fixture covers it anyway: the process exits at 50 leaving a member; the restart is due at 250, the kill at 2050,
  and the fixture expects the `kill` note and **no `restart` note**. On the literal text the run is still open at 250
  (the grace), so a restart note at 250 would be expected.
- Assumed first: the delay counts from the moment the old group is gone. That passes the fixture, but a probe
  (`py/probes/p6-closestdout-ignoreterm-restart`, a process that closes stdout and ignores SIGTERM, with an injection
  pending) showed the TypeScript engine writes the restart at detection plus backoff (210) while the grace runs.
- Assumed now: the delay counts from detection; a due restart is skipped, silently, when nothing but a kill grace (and no
  queued call) is left to keep the run open. Both engines agree on the 193 fixtures and the probe.
- Needed: one sentence in 6.2 rule 6: a restart that falls due when only a kill grace keeps the run open is not performed.
  The fixture's description says this; the normative text does not.

### 2. A queued call keeps the run open across a restart (6.2 rule 6; no fixture)

- The text says the run is over when "nothing is running or queued". My first version stopped as soon as only the
  restart delay was left, leaving a call queued behind a crashed level 1 process unanswered. No fixture caught it; a
  probe did (`py/probes/q4-l1-crash-with-cancelled-budget`). The TypeScript engine restarts and runs the call.
- Needed: a fixture (the probe is one), or the text of 6.2 rule 6 saying the restart delay is work while a call waits.

### 3. Rule 7 for a composed neuron (6.2 rule 7, CONFORMANCE 4.1; no fixture)

- The text: "a call's outcome due at the same instant as its `timeout` is processed first ... (a level 0 program's reply,
  a level 1 reply, **the exit of a composed neuron**)".
- Probe: composed `diag` with `timeout` 100; its entry answers at 0, its exit answers after 100 (so at 100). Both engines
  write the timeout error and the `cancel` note at 100, and no answer. CONFORMANCE 4.1 explains the mechanism for levels 0
  and 1 (the reply wake-up is set before the timeout's) but gives none for the inside of a composed neuron, whose exit
  reply is scheduled after the outer timeout was. Either both engines are wrong or the sentence in rule 7 is.
- I did not write this probe into `py/probes/` because it would pin the behaviour that contradicts the text.
- Needed: a fixture and a decision. This is the likeliest place where two engines that each pass 193 fixtures disagree
  with a third that follows the text.

### 4. The text of a `budget` error (6.1; CONFORMANCE 3.4)

- Fixtures: `budget/a-call-must-be-covered-in-every-unit`, `budget/a-refused-branch-does-not-finish-the-trace`,
  `budget/a-queued-level-1-call-checks-when-its-slot-frees`, `budget/an-injection-lowers-the-budget`,
  `budget/exhausted-in-the-middle-of-a-chain`, `effect/a-call-refused-for-budget-is-an-error-not-unknown`.
- 6.1 says the message starts `budget:`, "then what was short". CONFORMANCE 3.4 says expected messages hold only what 6.1
  fixes. The fixtures expect the prefix `budget: needs 1 calls, 0 left` (amount, unit, what the pool has), so another
  engine cannot pass by guessing the wording of "what was short".
- Assumed: `budget: needs <cost> <unit>, <pool> left`. With two short units I name the first in the order of the neuron's
  `cost`; no fixture has two short units.
- Needed: put the format in 6.1 (it is already normative in practice) or loosen the fixtures to `budget:`.

### 5. When `finished` is written relative to the next signal's delivery (6.2 rules 2 and 8; no wording)

- Fixtures: `budget/a-cut-call-commits-its-reservation`, `stream/every-event-in-written-order`.
- Rule 8: `finished` comes "right after the last event its last unfinished work caused". Rule 2: after a signal, first the
  drops it caused, then "for each target in `to` order, its delivery effects". A call cut by an injection finishes the old
  trace in the middle of that: the fixtures expect `drop`, `cancel`, `commit`, **`finished`** of the old trace, and only then
  the `reserve` note of the call the new signal starts.
- Assumed: the finished checks run after the stale drops of (a) and before the delivery effects of (b). A trace
  that the signal being delivered belongs to is not finished meanwhile ("still being delivered", 7.2).
- Needed: say so in rule 8, or give `finished` its place in rule 2.

### 6. The restart delay and the failed-start count `f` (3.2)

- Fixtures: `level1/early-crashes-double-the-backoff`, `a-pong-clears-the-failed-count`,
  `a-pong-clears-the-failed-count-again`, `a-life-of-exactly-1000-ms-is-proven`, `restart-after-a-long-life-waits-100-ms`.
- "`min(100 * 2^f, 5000)` where `f` is the number of consecutive failed starts" does not say whether the start that just
  failed is counted. The fixtures need it counted: the first early crash waits 200 ms, a proven life waits 100 ms.
- Assumed: `f` is incremented for the failed start, then the delay is computed; a `pong`, or a life of at least 1000 ms
  that does not end in a hang, sets `f` to 0 (first). Five consecutive failures fail the neuron.
- Needed: "`f` includes the start that just failed" (or a table: 200, 400, 800, 1600, failed).

### 7. Where the hang timer of a cancelled slot starts (3.2)

- Fixture: `failures/l1-timeout-and-no-cancel-reply-is-a-hang` (term at 200, not 100).
- 3.2: a cancelled `re` keeps its slot "until the neuron replies for it or `timeout` ms have passed". From what? The fixture
  says from the cancel (the call timed out at 100, was cancelled at 100, hung at 200).
- Assumed: from the moment `cancel` is sent, for a stale cut as well (no fixture pins the stale case separately).

### 8. A signal name is followed by a space (6.1)

- Fixture: `failures/l0-killed-by-a-named-signal` expects the prefix `signal SIGUSR1: ` with the trailing space, while
  `effect/a-call-that-could-not-be-started-is-an-error` expects `exit 127:` without one. 6.1 says "`exit N:` or `signal S:`, then
  stderr" and does not give the separator.
- Assumed: `signal S: ` always, then the stderr. Minor, but a prefix test makes it a rule.

### 9. Smaller points the fixtures leave open (all agree between this engine and the TypeScript one, by probe)

- **Time zero and `end.after`.** `end.after` counts from the process's own `ready`. A first-start process that is ready
  before time zero (CONFORMANCE 4.2: the instant the last `serve` neuron is ready) and ends before time zero is not specified.
- **A restart that never says `ready`** (probe `p2`): the start fails at `timeout`, the process is sent SIGTERM and a `term`
  note is written. 6.3 lists `term` for a process that "crashed, hung, or wrote a line over 1 MiB", not for one that never
  became ready.
- **`ready` exactly at `timeout`** (probe `p3`) is within `timeout`.
- **Junk line numbers** (probe `p4`): "line number (from 1, per process)" counts every stdout line, `ready` and `pong` included.
- **A pong that never comes** (probe `p5`): the hang is detected `timeout` after the ping, whether or not anything else keeps
  the run open; a pong deadline is not work.
- **A level 0 program that cannot be spawned** has a row only in Appendix C and in 11.3 (`exit`, 127), not in the 6.1 table.
  With a budget I write `reserve`, then `commit` just before the `error` (the TypeScript engine's events match through that).
- **Observer `when`** can only name fields the fixture can know: ids are opaque, so a rule cannot match on `id`, `trace`,
  `replyTo`, `cause`. Not said anywhere.
- **`also` lines** (CONFORMANCE 5.2) belong to the `reply` object, though the 5.1 table lists the call entry's fields without
  it. I read the table first and lost a few minutes; a sentence in 5.1 would help.
- **An answered call is finished before its own forward is emitted** (5.4 "finished work is unaffected", 4.4). Needed for
  `staleness/answered-call-is-not-cut-by-its-own-forward` when the forward replaces its own key; the order is implied, not stated.

## Differences from the TypeScript engine

None found on the 193 fixtures. On the probes the two engines differed twice before I changed this engine, both
described above (1 and 2). They also share the rule-7 gap (3). The probes are not a proof: they came from where I
thought the specification was thin.

## Time-consuming parts left undone

- A real process layer (level 0 and level 1 over pipes, process groups, signals, the 1 MiB limits on real bytes), and
  with it `check` and `run` commands and the trace file; the largest remaining piece, and the only one that tests SPEC 3
  against reality.
- `http` and `human` neurons.
- Fixtures for ambiguities 2 and 3, and a third engine to see which reading wins.
