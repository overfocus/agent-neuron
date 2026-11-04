# Trace format

A trace is a JSON file that records one run of a demo: which neurons there
were, and everything the network reported, in order, with times. It is made
to be replayed without running the library: a page can read the file and show
the run again on one clock. Traces are public and contain only invented data.

Files are in `traces/`. Format name `agent-neuron-trace`, version `1`.

## Compatibility

- A reader ignores top-level fields, event fields and event types it does not know.
- Adding an optional field or a new event type stays version 1.
- Removing or renaming a field, changing what a field means, or changing the replay
  rules (below) bumps `version`. The replay rules are part of version 1.
- Ids (signal ids, trace ids) are opaque strings: compare them, never parse them.
- `record` never writes a failed run, so `fail` events appear only in the network
  view's live stream, never in a file.
- Other projects may copy `traces/` and `examples/trace/replay.mjs`; a breaking change
  to either bumps `version`.

## Record

```sh
npm run record -- alerts                  # traces/alerts.json
npm run record -- typing
npm run record -- suggest
npm run record -- alerts --threshold 1 --out traces/alerts-threshold-1.json
```

It builds first, runs the demo once and writes the file. `--out` picks another
file. Record on an idle machine: the demos are timing-dependent (alerts 150 ms
apart or 900+ ms apart, a shell detector under 150 ms), and the numbers in a
trace are what that run produced, not constants. Re-record after changing a
demo, its timing or a neuron's options (`test/recording.test.ts` replays every
committed trace and fails if a summary no longer matches).

The alert storm is committed three times: `alerts.json` (`hypothesis` acts when
2 detectors agree, the demo's own setting), `alerts-threshold-1.json` and
`alerts-threshold-3.json`. Each is a separate real run, recorded with
`--threshold`, so its numbers are that run's own; the three share the same
summary fields and are not slices of one run. Threshold 3 may call nothing
at all, which is a result.

## Shape

One file, one object. The first fields are the header, then three sections.
The file is written one neuron and one event per line, so diffs read well.

```
format       "agent-neuron-trace"
version      1
demo         "alerts" | "typing" | "suggest"
description  one line about the run
library      the library VERSION that ran it (no dates, hosts or paths anywhere)
parameters   (alerts only) { threshold }: the setting this run was recorded with
neurons      [ ... ]   declared up front, including neurons that never send
links        [ ... ]   the connections, declared up front, so the network can be drawn before any traffic
events       [ ... ]   in the order the network reported them
summary      { ... }   the demo's own numbers
```

### neurons

```
id           the neuron's id; every from, to and neuron in the events is one of these
kind         what it wraps: "source", "js", "shell script", "model stand-in", "composed"
description  a short sentence. A shell-wrapped neuron names its script by file name only
options      when set: { threshold, within, maxHops }
```

A composed neuron is one neuron: its inner network never appears.

### links

```
from, to     two neuron ids that are connected (links are two-way in the library)
strength     optional number: how much a signal from `from` counts toward `to`'s threshold. Default 1
```

Nothing in the library uses strength yet, so no trace sets it. Replay already honours it (see Replay).

### events

`t` is whole milliseconds since the run started (monotonic, never decreasing).

| type | meaning |
|---|---|
| `signal` | a send or emit |
| `answer` | a neuron's answer, with a payload |
| `quiet` | an answer that says nothing (empty text): the neuron chose not to act. No payload |
| `drop` | work for a signal was dropped at `neuron`, `reason` `replaced`, `expired` or `hops` |
| `error` | `neuron` failed handling a signal; `message` |
| `fail` | the run itself failed; `message` only |

Every event except `fail` carries the signal's fields: `id`, `from`, `to` (list), `trace`, `replyTo` and `cause` when set, and `hops`.
`trace` is the chain id: the id of the first signal of a chain, shared by every signal
that answers or follows it. A threshold neuron groups its held inputs by it. It is not
the file or the run: one file holds many traces.
A `signal` event may also carry `via`: `"emit"` or `"send"`, how it was sent. Absent means
`emit`. The recorder does not write it yet (the library does not report it).

For `drop` and `error` they describe the signal that was dropped or that failed
(`id` names an earlier event), plus `neuron`, `reason` or `message`, and the
payload. `replyTo` and `cause` name the `id` of an earlier event, and `hops` is
that event's `hops` plus one (0 with neither).

`payload` is `{ "type": "json", "data": ... }` in full, or `{ "type": "text", "text": ... }`
as is. Cap: a payload whose text (or JSON text) is longer than 2000 characters
is cut to its first 2000 and marked `"truncated": true` (JSON then has `preview`
instead of `data`). The committed traces have none that long.

### summary

The demo's own numbers.

- alerts: `threshold`, `alerts`, `flags`, `calls` (started), `finished`, `cut`
  (cut short as stale), `heldReplaced`, `expired`, `diagnoses`, `final` (the last
  diagnosis, or `null`).
- typing: `keystrokes`. suggest: `keystrokes`, `suggestRuns`.

## Replay

`examples/trace/replay.mjs` is plain JavaScript with no imports and no Node or
browser APIs, so it can be copied as is into a bundle. Types: `replay.d.mts`.

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
with `via: "send"` is not held (the library does not hold `send`), and neither is one
whose `hops` exceed the receiver's declared `maxHops` (the library refuses it).
The network view uses `step` too, so there is one implementation.

Derived from a replay to the end, the alerts summary is: `calls` = `acted` at
`hypothesis`, `finished` = its `answers`, `cut` = the difference,
`heldReplaced` = its `replaced` drops minus `cut`, `expired` = its `expired`
drops, `diagnoses` = `answers` at `diagnosis`.
