# Conformance fixtures (normative)

This file is part of [SPEC.md](SPEC.md) section 8. It says what a fixture is, how an engine
runs one, what is compared, and how an engine reports it, precisely enough to write a runner
from this text alone. The fixtures are in [`spec/fixtures/`](../spec/fixtures/). They are the
contract: an engine conforms when it passes every one that is not pending. A fixture changes
only together with the specification text it pins (one commit).

A fixture never starts a process and never reads a clock. The scripted programs and the
virtual clock below stand in for the process layer, so a fixture tests what an engine does
with an outcome (a reply, a crash, silence), not how it parses bytes, and two engines can be
compared event for event. Real-process behaviour is each engine's own tests' business.

## 1. Layout

`spec/fixtures/<area>/<name>/` holds `fixture.json` and the network files it uses (the
`network` file and any composed file it names, with the paths the files give). Any directory
that contains a `fixture.json` is a fixture; discovery is recursive and the order is by path.
A fixture's **name** is its path under the fixtures directory, with `/`: `staleness/running-call-is-cut`.

## 2. `fixture.json`

One JSON object. An engine MUST fail a fixture with an unknown top-level field.

| Field | Type | Default | Meaning |
|---|---|---|---|
| `fixture` | `0` | required | Format version of this file. |
| `description` | string | required | One line: what the fixture pins. |
| `network` | path | `network.json` | The network file, relative to the fixture directory. |
| `notes` | boolean | `false` | Compare `note` events (3). |
| `finished` | boolean | `false` | Compare `finished` events (3). |
| `streamFails` | boolean | `false` | The consumer of the live stream throws on every event (3, item 6). |
| `pending` | string | none | If present, the fixture is not run; the string is the reason (7). |
| `pingEvery` | integer ms | `10000` | The idle ping interval the engine uses for level 1 neurons in this run (4.4). |
| `refuses` | boolean | `false` | The engine must refuse to start (4.3); `expect` is then `[]`. |
| `inputs` | array | required | Injections: `{"at": ms, "source": id, "payload": any JSON, "replaces"?: string, "budget"?: units}`. |
| `script` | object | `{}` | The scripted programs, by instance path (4). |
| `observer` | array | none | A scripted observer (6). |
| `expect` | array | required | The events (SPEC 7), in order, ids normalised (3). |
| `expectWithout` | array | none | The events with the observer removed (6). |
| `independent` | boolean | `false` | The observer's injections only add traces of their own (6). |

`inputs` are the producer: each is injected into its `source` at virtual millisecond `at`
(SPEC 4.1). The network files are ordinary network files and pass `check` (SPEC 2.7), except those of a fixture with `refuses`; a fixture's directory also holds the composed files (SPEC 2.6) and the neuron files (SPEC 2.10) its network names. Their program
commands (`["neuron-a"]`) are names only: nothing runs them.

## 3. Comparison

The engine runs the network with the `inputs` and the script and produces its event list
(SPEC 7.2). A fixture passes when the list equals `expect`:

1. **Notes.** If `notes` is `false`, `note` events are removed from both lists before anything
   else. If `true`, they are compared like any event. The notes of SPEC 6.3 are MUST for an
   engine that claims conformance: a fixture with `notes: true` requires them, with the
   order and times 6.3 gives. The `reserve` and `commit` notes of a budget are among them.
   **Finished events.** The same for `finished`: if the fixture's `finished` is `false`, those events
   are removed from both lists; if `true`, they are compared like any event. Every engine writes them
   (SPEC 7.2), but only the fixtures that pin them compare them, so a fixture about something else does
   not change when the trace gains an event.
2. **Ids.** Ids become `#1`, `#2`, ... by first appearance over the whole list. Scan the events
   in order; within an event, scan the fields `id`, `trace`, `replyTo`, `cause` and (a note's)
   `trigger` in that order; a value not seen before gets the next number. The same string
   always gets the same number, so a `drop` that names an earlier signal's `id` shows `#n` of it.
   Ids are opaque: an engine may number them any way before this step.
3. **Events.** Lists are compared one by one, the whole list in order: the same length, and
   each event equal to its expected event in every field, `t` included, the order of keys
   irrelevant. A `via` field that is absent equals `"emit"`. A field the expected event does not
   have MUST NOT be in the actual one.
4. **Messages.** For an `error` or `unknown` event, `message` is compared as a prefix: the actual message
   must start with the expected one. The expected messages hold only what SPEC 6.1 fixes
   (`exit 1:`, `process failed`, `timed out after 100 ms`, a `neuron`'s own text, ...); what
   follows (stderr, line previews) is the engine's.
5. **Payloads.** As SPEC 7.2. Payloads over 2000 characters are not used in fixtures.

6. **The live stream.** The runner gives the engine a consumer of the live stream (SPEC 9.1) in every run, with
   and without an observer. The consumer keeps a copy of each event it is given, then alters the event it
   was given (so an engine that hands over its own events fails) and, if `streamFails` is true, throws. The
   run fails if the copies are not the engine's events, event for event in the same order, notes and `finished`
   included whatever `notes` and `finished` say: those flags choose what `expect` lists, not what the stream
   carries. With `streamFails` the events must still equal `expect`: the stream fails open.

A fixture that fails reports the first difference.

## 4. The virtual run

### 4.1 Clock

Virtual time is an integer count of milliseconds starting at 0, and it only jumps: to the next
**wake-up**. Processing an event takes no time. A wake-up is anything the run waits for: an
injection's `at`, the end of a scripted latency, a `within`, a `timeout`, the 2000 ms kill
grace, a restart delay, an idle ping. Wake-ups due at the same instant run in the order they
were set (SPEC 6.2 rule 1), with these fixed points so that engines agree:

- **Injections are set before the run starts, in the order `inputs` lists them.** At a tie they
  go before every wake-up the run sets itself, and among themselves in listed order.
- Calls started by one event (the targets of one signal, in `to` order) set their wake-ups in
  that order, so their scripted replies at one instant are processed in that order.
- Held inputs set their `within` wake-up when they are held.
- **A call's timeout is an ordinary wake-up**, set when the call starts: at level 0 after the program's own
  wake-ups (hence the last point of this list), at level 1 when the `signal` line is written. At an instant it has no place of
  its own: set before another call's reply or a held input's `within` expiry, it goes before it; set after, after
  (SPEC 6.2 rule 1). So with `s` linked to `a` (timeout 100, never replies) and then `b` (replies after 100), the
  error of `a` comes before the answer of `b`; with the links the other way round, after.
- A call that ends at the very instant its input goes stale counts as stale (6.2 rule 5); with
  an injection that replaces it, this follows from the first point.
- **A call's outcome due at the instant of its `timeout` is processed first** (SPEC 6.2 rule 7):
  a program's reply wake-up is set before the call's timeout wake-up, so the reply wins.
- A scripted process that ends (by its own script or by a signal) is known to the engine at
  that same instant, without a wake-up of its own: the exit is processed inside the step that
  ended it.
- **A call that waited for a place under `maxInflight` starts at the end of a step** (SPEC 6.2 rule 9),
  where a step is the processing of one wake-up and everything it causes. The start is not a wake-up:
  it follows the step's last event and goes before the next wake-up, at the same instant or not. The
  call sets its own wake-ups then (its scripted reply, then its timeout), so they go after every
  wake-up already set for their instants.

Fixtures avoid the one tie the specification does not decide: a ping due at the instant of an
injection.

The run is **over** (SPEC 6.2 rule 6) when no work is running, queued or held, no injection is
still to come, no observer injection is pending, and no cancelled work is left behind. A kill grace
already under way for any process group the engine has terminated, cancelled or crashed, keeps the run
open until the group is gone, and its `kill` note is recorded. The engine then stops its programs
silently (no events). A held input keeps the run open until its `expired` drop. A pending restart delay
or idle ping does not: the engine stops the programs silently when the run is over, so no `restart`
note follows.

### 4.2 Time zero

Every `serve` program (in every composed instance) is started first, in the network's order.
Time zero is the instant the last of them said `ready`; `at`, `t` and every latency count from
there. A fixture whose programs are all ready at once has its start-up at 0. A restarted
process's latencies count from its start.

### 4.3 Refusal

If a `serve` program fails its first start (no valid `ready` within `timeout`, a first line that
is not a `ready` object, a `ready` whose `v` differs from `hello.v` or whose `maxInflight` is
not an integer of at least 1), the engine refuses to start (SPEC 3.2): it reports no run at
all. The same holds for a network file that does not pass `check` (SPEC 2.7: a budget with a neuron
that has no cost, say) and for an injection the engine refuses to the producer (SPEC 4.1, 10.2: one
that raises a budget). A fixture with `"refuses": true` passes when the engine refuses and fails when it starts.

### 4.4 Pings

The idle ping interval is the engine's (the specification gives none); a fixture that depends
on it sets `pingEvery`, and the runner passes it to the engine for this run. A ping is due
`pingEvery` ms after the process said `ready` and then every `pingEvery` ms, and is sent only
when nothing is running (SPEC 3.2); a tick is skipped while an earlier ping is unanswered.

## 5. The script

`script` maps an **instance path** to a scripted program: a `run` or `serve` neuron's `id`; for
a neuron inside a composed neuron `outer/inner`, the ids joined by `/`. The scripts replace
every program. Each value is an object:

```json
{ "starts": [ {start} ... ], "calls": [ {call} ... ] }
```

Both are optional. `starts` is for `serve` neurons only.

### 5.1 Calls

A **call** entry answers one call of the program:

| Field | Meaning |
|---|---|
| `when` | A JSON value. The entry matches a call whose input value (SPEC 3.1: the bare payload, or `{"payload", "inputs"}` for a threshold above 1) is deeply equal to it. No `when`: matches any input. |
| `after` | Latency in ms from the start of the call (level 0: when the process starts; level 1: when the `signal` line is written). Default 0. |
| `reply` | The outcome (5.2). |
| `ignoreTerm` | Level 0 and 1: the program survives SIGTERM until it ends or is killed with SIGKILL (default: SIGTERM ends it at once). |
| `leaves` | Level 0: when the program ends, by itself or on SIGTERM, a member of its group stays alive. It ignores SIGTERM and ends only on SIGKILL (5.4). |
| `onCancel` | Level 1: `{"after": ms, "reply": {...}}`. When a `cancel` for this call arrives, the neuron abandons its scheduled reply and sends this one `after` ms later. Without it the neuron ignores `cancel` and replies as scheduled. |

Each call uses **the first entry not yet used** that matches. A call no entry matches is a
**harness failure**: the fixture fails and says so. (So a call that must not happen is
"pinned" by leaving no entry for it.)

### 5.2 Replies

At level 0 (a program that reads stdin once):

| `reply` | The process |
|---|---|
| `{"answer": P}` | prints the JSON value `P` and exits 0 |
| `{"quiet": true}` | prints nothing and exits 0 |
| `{"error": "text"}` | writes `text` to stderr and exits 1 |
| `{"junk": "text"}` | prints `text` (not JSON) and exits 0 |
| `{"crash": true}` | is killed by SIGKILL |
| `{"crash": "SIGUSR1"}` | is killed by the named signal (any signal name) |
| `{"hang": true}` | never ends by itself |
| `{"overlong": true}` | prints more than 1 MiB |
| `{"spawnFails": true}` | cannot be started (no such executable): the engine's start fails at once |

At level 1 (a neuron on the protocol of SPEC 3.2) the neuron writes, at `after`:

| `reply` | Lines |
|---|---|
| `{"answer": P, "replaces"?: K or null, "to"?: [ids]}` | `{"type":"answer","re":R,"payload":P,...}` with the optional fields as given |
| `{"quiet": true}` | `{"type":"quiet","re":R}` |
| `{"error": "text"}` | `{"type":"error","re":R,"message":"text"}` |
| `{"junk": "text"}` | the line `text` (not a JSON object with a `type`) |
| `{"crash": true}` | nothing; the process exits with status 1 |
| `{"hang": true}` | nothing, ever (a `cancel` may still be answered through `onCancel`) |
| `{"overlong": true}` | one line over 1 MiB |
| `{"send": {obj}}` | the line `{"re":R, ...obj}` (obj may override `re` or `type`: for protocol errors) |

Any level 1 reply may carry `"also": [ ... ]`, written first: an object becomes the line
`{"re":R, ...object}` (R the call's `re`; an object may set its own `re`), a string becomes a
line as it is. Used for unknown message types, replies for an unknown `re` and junk lines.

### 5.3 Starts (level 1)

The n-th process the engine starts for a `serve` neuron (n from 1, restarts included) uses
`starts[n-1]`; past the end of the list, or without `starts`, a start is `{}`.

| Field | Meaning |
|---|---|
| `ready` | ms from the `hello` line to the `ready` line (default 0); `false`: never says `ready` |
| `maxInflight` | the integer sent in `ready` (default: the field is absent) |
| `readyV` | the `v` sent in `ready` (default 1) |
| `first` | a line (string) or object written instead of `ready` |
| `pong` | ms to answer each `ping` (default 0); `false`: never |
| `end` | `{"after": ms, "how": "exit" or "closeStdout" or "error"}`: `after` ms after `ready`, the process exits with status 1 (`exit`, the default), closes its stdout but stays alive (`closeStdout`), or writes `{"type":"error","message":...}` without `re` (`error`) |
| `ignoreTerm` | as for calls |
| `leaves` | as for calls: when the process ends, a member of its group stays alive (until SIGKILL) |
| `stderr` | text written to stderr when the process starts |

### 5.4 Processes and signals

A scripted process is in the process group the engine starts. SIGTERM to the group ends it at
once (unless `ignoreTerm`), SIGKILL always; a level 1 process also ends when its stdin is closed. A process that has ended
no longer replies. The engine learns that a process ended at the same virtual instant. With `leaves`, the
group still has a live member after its process ended: SIGTERM does not end it, SIGKILL does. The
engine's notes about the group are those of SPEC 6.3: `term` only if the group still had a live
member when SIGTERM was sent, `kill` only if it had when SIGKILL was sent (after the 2000 ms
grace, or at once for level 0 output over 1 MiB), each after the event that reports the call.

### 5.5 People (`human` neurons)

A `human` neuron's person is scripted like a program: `script` maps its instance path to `{"calls": [...]}`, each
call entry with `when` and `after` as in 5.1 (the first unused entry whose `when` equals the input; a question no
entry matches is a harness failure). The engine asks the person when the call starts; the reply comes `after` ms
later, a wake-up set when the call starts and before the call's timeout, so a reply due at the instant of the timeout
is processed first (SPEC 6.2 rule 7), as for a level 0 program. The replies a person can give:

| `reply` | The person |
|---|---|
| `{"answer": "text"}` | answers with the text: a text payload (SPEC 3.4) |
| `{"quiet": true}` | answers with nothing: quiet |
| `{"hang": true}` | never answers (a `timeout`, a stale input or the end of the run ends the call) |

Any other reply, an `answer` that is not a string included, is a harness failure: a person replies with text. A call
that ends without an answer (stale, timed out) cancels its pending reply. The other fields of 5.1 (`ignoreTerm`,
`leaves`, `onCancel`) have no meaning for a person and are ignored. A `human` neuron's `timeout` may be up to a day
(SPEC 2.4), so a virtual run may last that long.

## 6. Observers

`observer` is a list of rules. A rule is `{"when": {...}, "inject": {"after": ms, "source": id,
"payload": P, "replaces"?: K}}`. Each rule fires once, on the first event (of the outer
network) that is a signal, answer, quiet, drop, error or unknown (events with an `id`) or a `finished` event (notes
are not shown to an observer), whose fields include every field of `when` with an equal value; it
then injects `payload` into `source` `after` ms later (SPEC 9: asynchronous, into a `source`
only). The engine records the injection as a note `{"type":"note","t":T,"neuron":SOURCE,"what":"inject","trigger":ID}`
(`ID` the `id` of the event that fired the rule, or, for a `finished` event, which has none, its `trace`) just before the injected signal.
A rule on `finished` can match on `type`, `status` and `remaining` (the trace is not known to the fixture). An observer
sees the outer network only. An empty list is an observer that only reads.

SPEC 8.5 is checked by running twice, and which list belongs to which run is fixed:

1. The run **with** the observer MUST give exactly `expect`.
2. The run **without** it (same inputs and script, the rules removed) MUST give exactly `expectWithout`,
   if the fixture has one. With no `expectWithout`: if the observer has no rules, that run MUST equal `expect`;
   if it has rules, that run is compared with no list at all, only by the two checks below. `expectWithout`
   is never a stand-in for `expect`, and `expect` is never a stand-in for a missing `expectWithout` when
   the observer has rules.

And two checks that need no expectation of their own: the events before the first `inject` note
are identical in both runs, and if `independent` is true (or nothing was injected), removing
the events of the traces that start at an injection leaves exactly the events of the run
without the observer. Notes take no part in that second check (a note names a neuron, not a
trace, so those caused by an injected trace's work cannot be told apart).

## 7. Pending

A fixture with `pending` is written but not yet implementable: it is part of a later
specification version. A runner reports it as pending, with the reason, does not run it and
does not count it as failed. None is pending today: the `budget/` fixtures that were the plan for
version 0.1 pin its budget (SPEC 10) and the `finished` event.

## 8. The command-line contract

An engine MUST provide

```
<engine> conform <dir> [--only <text>]
```

which runs the fixtures under `<dir>` (those whose name contains `<text>`, if given), and
prints on stdout one line per fixture, in name order:

```
pass <name>
fail <name>            (then the first difference, each line indented by two spaces)
pending <name>: <reason>
```

then a last line `N passed, M failed, K pending`. The exit status is 0 if no fixture failed,
1 if one did (a harness failure counts), 2 for a usage error or if `<dir>` holds no fixture.
The wording of the difference is the engine's.

The reference engine is the Rust engine in `rust-engine/`: `neuron-engine conform spec/fixtures`
(`npm run conform` builds it first, and runs the probes in `spec/probes/` after the fixtures). Until
[decision 0023](decisions/0023-one-engine.md) the runner was a TypeScript engine's, a Python engine written
from the text alone passed the same fixtures, and a mutant test (a hundred deliberately broken engines that
each had to fail a fixture) measured whether the fixtures pin the rules; those are deleted, and one engine
runs the fixtures now.

## 9. What the fixtures do not cover

`http` and `human` neurons; real processes, bytes and framing (the junk and over-long outcomes
are scripted, not parsed); payloads over 2000 characters; `check` (SPEC 2.7); the fail-open
behaviour of a *slow* stream consumer or observer (one that throws is `streamFails`), or a stream written to a file or pipe (the engine's own tests); the file formats of `trace` output. The 6.1 row for
`http` has no fixture until an engine runs `http` neurons.

Not covered, on purpose or for lack of a model:

- **Signal names** beyond `SIGKILL` and `SIGUSR1`: the fixtures prove the name (not the number) is used, not that
  an engine names every signal the operating system can deliver. That mapping is each engine's own tests' business.
- **A survivor in a real process group.** `leaves` models a member that outlives its process, for a cut level 0 call
  and for a level 1 process that exits by itself. It does not model one for a level 0 timeout, a level 0 output over
  1 MiB, a level 1 hang or an over-long line, nor a member that ends by itself during the grace.
- **A composed neuron's `exit` reply carrying `to`**: only its `replaces` is pinned (SPEC 2.6).
- **A restart delay and a pending kill grace racing**: a restart that is due while the old group is still being
  killed.
