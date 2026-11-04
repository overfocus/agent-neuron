# Decision record 0002: the alert storm as a network file

Where each piece of `examples/alerts-demo.mjs` went, and what could not be
expressed. Files: `spec/examples/alerts.network.json`, `diagnosis.network.json`.

| Today's caller code | Now |
|---|---|
| `alerts` neuron with a no-op adapter | `"source": true` |
| `net.emit('alerts', json(a), { replaces: a.service })` | injection with a key (SPEC 4.1); finding 1 |
| the `net.connect` calls | seven one-way `links` |
| `onSignal`: only non-empty answers matter | quiet forwards nothing (SPEC 4.4) |
| `net.emit(s.from, s.payload, { to: ['hypothesis'], cause: s, replaces: d.service })`, and again hypothesis to diagnosis | automatic forward along outgoing links, `cause` the answer, key inherited |
| `new Neuron('hypothesis', ..., { threshold, within: 500 })` | `"threshold": 2, "within": 500` |
| `hypothesis`: `flags[0]`, sorted detector names, 500 ms wait | the program `hypothesis.mjs` |
| `callOf`, `runs`, `stats`, `cutSince`, `heldReplaced`, `expired`, `verdicts`, log lines | observers; counts derive from the trace, held vs running is `phase` |
| `diagnosis`: inner `door`, `topology`, `check`, merge `{...v, call, alert, detectors}` | `diagnosis.network.json`; `door` is not needed; the merge is each program passing its input through |
| `composedDiagnosis`'s `live()` abort check | engine rule: stale or timed-out work is cancelled, inner work included |
| `onError` log | `error` events |
| `idle()` then a sleep of `within` | a run is over when nothing is running or queued; held inputs expire (SPEC 6.2) |

Nothing needs a key in the file to read a payload. Findings, said plainly:

1. **The root key is set by the producer** at injection. The file cannot compute
   "keyed by service" without reading the payload. It is the only caller-side
   piece left, and it sits where the signal is born.
2. **`call`, a per-run counter, cannot exist at level 0** (a fresh process has no
   memory). It was demo instrumentation; calls are counted from the trace. A
   neuron that needs state uses `serve`.
3. **Merging is a convention between programs**, not an engine feature: `topology`
   and `check` copy their input and add to it.
4. **There is no network result**: the verdict is the answer event of `diagnosis`.

One behaviour changes on purpose: under #17 more calls may be cut than the
TypeScript demo cuts today.
