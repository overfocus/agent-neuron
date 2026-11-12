# Trace format

A trace is a JSON file that records one run: which neurons there
were, and everything the network reported, in order, with times. It is made
to be replayed without running anything: a page can read the file and show
the run again on one clock. Traces are public and contain only invented data.

The engine writes one with `neuron-engine run <file> --inputs <jsonl> --trace <out.json>`. The files in
`traces/` are older: recordings of the TypeScript library's demos, made by its recorder before
[decision 0023](decisions/0023-one-engine.md) deleted both. They stay as examples of the format and cannot be
recorded again. Format name `agent-neuron-trace`, version `1`.

## Compatibility

- A reader ignores top-level fields, event fields and event types it does not know.
- Adding an optional field or a new event type stays version 1.
- Removing or renaming a field, changing what a field means, or changing the replay
  rules (below) bumps `version`. The replay rules are part of version 1.
- Ids (signal ids, trace ids) are opaque strings: compare them, never parse them.
- No file holds a `fail` event: the deleted recorder never wrote a failed run, and the event appeared only in
  the live stream of the deleted network view.
- Other projects may copy `traces/` and `examples/trace/replay.mjs`; a breaking change
  to either bumps `version`.

## The committed recordings

The alert storm is committed three times: `alerts.json` (`hypothesis` acts when
2 detectors agree, the demo's own setting), `alerts-threshold-1.json` and
`alerts-threshold-3.json`. Each is a separate real run of the deleted library's demo,
so its numbers are that run's own; the three share the same
summary fields and are not slices of one run. Threshold 3 may call nothing
at all, which is a result. `typing.json` and `suggest.json` record two more of its demos.

## Shape

One file, one object. The first fields are the header, then three sections.
The file is written one neuron and one event per line, so diffs read well.

```
format       "agent-neuron-trace"
version      1
demo         a free name for the run: "alerts", "typing", "suggest", or an engine's network `name` (readers must not assume one of the three)
description  one line about the run
library      the library or engine VERSION that ran it (no dates, hosts or paths anywhere)
engine       optional: the name of the engine that wrote it; absent in the recordings of the deleted TypeScript library
parameters   (the alerts recordings only) { threshold }: the setting this run was recorded with
budget       optional, from engines following docs/SPEC.md: the network file's `budget`, units to whole-number ceilings
neurons      [ ... ]   declared up front, including neurons that never send
links        [ ... ]   the connections, declared up front, so the network can be drawn before any traffic
events       [ ... ]   in the order the network reported them
summary      { ... }   the demo's own numbers
```

### neurons

```
id           the neuron's id; every from, to and neuron in the events is one of these
kind         what it wraps: "source", "js", "shell script", "model stand-in", "composed"; an engine
             following docs/SPEC.md writes "source", "run", "serve", "http", "human" or "composed"
description  a short sentence. A shell-wrapped neuron names its script by file name only
options      when set: { threshold, within, maxHops }
cost         optional, from engines following docs/SPEC.md: the neuron's `cost`, units to whole-number amounts
```

A composed neuron is one neuron: its inner network never appears.

### links

```
from, to     two neuron ids; a link is one-way in the format (the TypeScript library linked both ways, and its recordings hold both)
strength     optional number: how much a signal from `from` counts toward `to`'s threshold. Default 1
```

No engine writes strength yet, so no trace sets it. Replay already honours it (see Replay).

### events

`t` is whole milliseconds since the run started (monotonic, never decreasing).

| type | meaning |
|---|---|
| `signal` | a send or emit |
| `answer` | a neuron's answer, with a payload |
| `quiet` | an answer that says nothing (empty text): the neuron chose not to act. No payload |
| `drop` | work for a signal was dropped at `neuron`, `reason` `replaced`, `expired` or `hops` |
| `error` | `neuron` failed handling a signal; `message` |
| `unknown` | optional, from engines following docs/SPEC.md: a call of a neuron declared `effect` started and then failed (crash, hang, timeout, ...), so nobody can tell whether its effect happened. The fields of an `error`: `neuron`, `message`, `code`, payload. **A reader that does not know this type must not conclude that nothing happened**: it is in place of an `error`, and a count of failures must count it too (or separately) |
| `fail` | the run itself failed; `message` only |
| `finished` | optional, from engines following docs/SPEC.md: a trace has no unfinished work left. `trace`, `status` (`done` or `exhausted`) and, when the network has a budget, `remaining` (units to amounts). No signal fields, no `id` |
| `note` | optional, from engines following docs/SPEC.md: a process fact (`neuron`, `what`: `junk`, `cancel`, `term`, `kill`, `restart`, `retry`, `inject`, `reserve`, `commit`; plus `line`, `text`, `trigger` (the `id` of the event that fired an observer injection, or the `trace` of a `finished` event, which has no `id`; on a `retry` note, the `id` of the failed attempt's `error` or `unknown`), `attempt` and `wait` (a `retry` note: another attempt of the failed call follows, so that failure is not the call's outcome), `units`, `ms` or `basis`). No signal fields |
| `ground` | optional, from engines following docs/SPEC.md: a deposit wrote a place (SPEC 4.5, decision 0012). `place` (a neuron id), `v` (the stored value after the write), `cause` (the depositing answer's id). No signal fields, no `id`. An old reader ignores it |

Every event except `fail` carries the signal's fields: `id`, `from`, `to` (list), `trace`, `replyTo` and `cause` when set, and `hops`.
`trace` is the chain id: the id of the first signal of a chain, shared by every signal
that answers or follows it. A threshold neuron groups its held inputs by it. It is not
the file or the run: one file holds many traces.
A `signal` event may also carry `via`: `"emit"` or `"send"`, how it was sent. Absent means
`emit`. No committed recording carries it.

Optional fields from docs/SPEC.md, which readers may ignore: `forwarded` (`false`) on `answer`: the answer of an effect call whose
input went stale was recorded and not forwarded; `replaces` on `signal` (the stale-work key);
`phase` on `drop` (`waiting` or `running`: whether the work had started; a running drop of a threshold call
may name the crossing input even when a different, earlier input was the one replaced); `code` on `error` and `unknown`
(`exit`, `output`, `junk`, `http`, `protocol`, `timeout`, `crash`, `start`, `inner`, `neuron`, `budget`).

Budget accounting (docs/SPEC.md section 10) is written only for a network with a `budget`. All its
amounts are JSON integers (0 to 2^53 - 1): `reserve` notes carry `units`, the cost reserved when a call
starts; `commit` notes carry `units` (the amount consumed), `ms` (the call's wall time as the engine
measured it, whole milliseconds) and `basis` (`estimated`: the engine took the reservation because the
real figure is unknown; a later version adds `measured`). An `error` with `code` `budget` is a call refused
because its cost could not be covered. Every trace of an engine run ends with one `finished` event,
with a budget or without: `status` `exhausted` when a call was refused, else `done`, and `remaining`, what
is left of the trace's pool. A reader that does not know these ignores them, as it ignores all of the above.

Effects (docs/SPEC.md section 11): the `unknown` event is a new type, not an `error` with a new code, on purpose. An old
reader that skips events it does not know shows the call as neither failed nor retried, so it does not retry something
that may have happened; an old reader that read a new `error` code as a plain failure might. The `unknown` event
carries the payload of the input (only the crossing input's, for a threshold call), so a reconciling producer can read the id it
put there, if it is short and at the top level of the payload: a payload over 2000 characters is cut to a preview. A "not sent"
from a lookup is only as good as the other system's consistency (a timed-out call may still land), and any retry must be idempotent by the id (SPEC 11.4). The live stream (SPEC 9.1) carries
these events, and every other event of the trace, as they are written, one JSON object per line from `run --stream`.

For `drop` and `error` they describe the signal that was dropped or that failed
(`id` names an earlier event), plus `neuron`, `reason` or `message`, and the
payload. `replyTo` and `cause` name the `id` of an earlier event, and `hops` is
that event's `hops` plus one (0 with neither).

`payload` is `{ "type": "json", "data": ... }` in full, or `{ "type": "text", "text": ... }`
as is. Cap: a payload whose text (or JSON text) is longer than 2000 characters
is cut to its first 2000 and marked `"truncated": true` (JSON then has `preview`
instead of `data`). The committed traces have none that long.

### summary

The demo's own numbers, in the committed recordings; a trace the engine writes has an empty one.

- alerts: `threshold`, `alerts`, `flags`, `calls` (started), `finished`, `cut`
  (cut short as stale), `heldReplaced`, `expired`, `diagnoses`, `final` (the last
  diagnosis, or `null`).
- typing: `keystrokes`. suggest: `keystrokes`, `suggestRuns`.

## Replay

`examples/trace/replay.mjs` is plain JavaScript with no imports and no Node or
browser APIs, so it can be copied as is into a bundle. `examples/network.mjs` reads the engine's trace with it.

```js
import { replay, start, step } from './replay.mjs';
const state = replay(trace, 3200);   // everything up to t = 3200 ms (default: the whole trace)
const live = start(trace);           // or fold one event at a time...
const acted = step(live, event);     // ...step returns the threshold neurons that just acted
```

State is plain JSON:

- `t`: time of the last event folded in.
- `neurons[id]`: `sent` (signals), `answers`, `quiet`, `errors`, `acted` (times a threshold neuron met its threshold and ran), `dropped` (counts by reason).
- `edges[from][to]`: signals that crossed from `from` to `to`; a request and its answer are counted in opposite directions.
- `held[id]`: the inputs a threshold neuron is waiting on, `[{ id, from, trace }]`.
- `thresholds[id]`, `maxHops[id]`, `weights[to][from]`: the declared thresholds, hop limits and stated link strengths.

Held inputs follow `emit`: a `signal` to a threshold neuron is held; once
enough senders have an input held on one trace, all that trace's
inputs leave and the neuron has `acted`; a `drop` at the neuron removes its
signal. Each distinct sender counts for the `strength` of its link to the neuron
(`links`; default 1) and the neuron acts when the sum reaches `threshold`. A signal
with `via: "send"` is not held, and neither is one
whose `hops` exceed the receiver's declared `maxHops`.

Derived from a replay to the end, the alerts summary is: `calls` = `acted` at
`hypothesis`, `finished` = its `answers`, `cut` = the difference,
`heldReplaced` = its `replaced` drops minus `cut`, `expired` = its `expired`
drops, `diagnoses` = `answers` at `diagnosis`.
