# Decision record 0007: effects, the `unknown` outcome and the live trace stream (spec version 0.1, 2026-11-04)

The second feature of version 0.1 of [SPEC](../SPEC.md) adds the neuron option `effect` and the
`unknown` outcome (SPEC 11), a normative live trace stream (9.1) and the sealing wording (2.6). The
scope came from the same reviews as [decision 0006](0006-budgets.md); the semantics of `effect` and the
field budget are Kevin's. Where the builder or the lead chose, the row says **builder** or **lead**, and
**to confirm** means Kevin has not yet said yes.

## Kevin's decisions

| # | Decision | Reason |
|---|---|---|
| 1 | `effect` is a neuron option, the eighth and last planned (`threshold`, `within`, `maxHops`, `timeout`, `env`, `cwd`, `cost`, `effect`). | Does it say how neurons connect, or how one decides? Neither: it says how the *engine* treats a call, the class of `timeout`, `maxHops` and `cost`. A program cannot apply it to itself, because the engine is the one that cuts a call when its input goes stale. The option has no value but `true` and no default that matters, and a file without it is unchanged. |
| 2 | Staleness and the engine's timeout never cut an effect call; its answer is accepted and committed; only the forward is suppressed when the input went stale. (The timeout part is refined by lead decision 6.) | The call has done, or is doing, something that cannot be taken back. Cutting it stops nothing that already happened, and makes the record say less than the truth. |
| 3 | A crash or hang of an effect call is `unknown`, not an error that implies nothing happened. | An error says the call failed and, to every tool that retries on error, that doing it again is safe. For an effect that is exactly the wrong signal. |

## The builder's and the lead's choices

| # | Decision | Reason | Status |
|---|---|---|---|
| 4 | **Only a started call is protected.** A held input or a level 1 call queued for a slot is dropped when stale (`replaced`, phase `waiting`) as for any neuron. | A waiting call has done nothing, so there is nothing to protect, and a replaced input is exactly what the key means. Carrying out an effect for an input already known to be stale would be a new effect nobody asked for. | builder, to confirm |
| 5 | **The not-forwarded fact is a field on the `answer` event: `"forwarded": false`**, present only on the answer of an effect call whose input went stale. | A note is optional and compared only with `notes`; this is a routing fact a reader of the main stream needs. A `drop` would say the work was dropped, which is false: it ran and was recorded. A field is invisible to a v1 reader, sits on the event it qualifies, and `answer` with no `forwarded` means forwarded, as before. Quiet and failures need no mark: nothing would have been forwarded. | builder, to confirm |
| 6 | **A hung or timed-out effect call is killed at its `timeout` and recorded `unknown`** (cancelled as any call is: SIGTERM then SIGKILL, `cancel` at level 1). | The alternative, letting it run past its timeout, makes a hung program hold the run open for ever and makes the timeout mean nothing for the one kind of neuron most likely to hang on the outside world. The record stays honest: `unknown`, code `timeout`. | **lead, Kevin to confirm** |
| 7 | **`unknown` is a new event type**, with the fields of `error` (`neuron`, `message`, `code`, payload), not an `error` with a new code. | Under trace v1 readers ignore what they do not know. An old reader that skips `unknown` shows a call that neither failed nor was retried. An old reader that met a new `error` code might count it a plain failure and retry. Neither is ideal; only the first cannot cause a second effect. The cost is that a failure counter that does not know the type misses it, which TRACE.md says in so many words. | builder, to confirm |
| 8 | **Every failure of a started effect call is `unknown` except `budget`, `start` and `neuron`.** | After the call has started, a crash, a hang, a timeout, a non-zero exit, junk, an over-long or malformed reply all leave the same question open. `budget` and `start` mean the call never started. `neuron` is the neuron saying, itself, that it failed (a level 1 `error` reply), which is the one case where the program knows. A level 0 program that wants to say "it did not happen" has no channel but exit status, which is also what a crash looks like, so it reads as `unknown`: use level 1 (`error` reply) for that. A level 0 program that could not be *spawned* never ran and is an `error` (`exit` 127). | builder, to confirm |
| 9 | **The cost of an effect call commits as estimated, on every outcome**, `unknown` and a stale answer included. | Decision 1 of 0006: a started call has spent what it spent. Nothing new is needed. | follows 0006 |
| 10 | **Reconciliation is not engine machinery.** The documented pattern: an id in the payload, a declared `reconcile` source linked to a `lookup` neuron, an injection by a producer or observer that saw the `unknown` (SPEC 11.4). The engine never retries, never holds a key, and never waits for the reconciliation. | The brief's "held until reconciled" is met by what the engine does not do: nothing is forwarded and nothing else runs on the unknown's behalf. A durable hold (survives a restart, blocks a key, has an owner) is the open problem for human approval too and needs state the engine does not have. The injection is already a rule (4.1, 9) and already recorded. | builder, to confirm |
| 11 | **A composed neuron is never an effect and its file may contain none**; `check` reports an error. | A composed call is cut as a whole, recursively and without reports (2.6), and sealing hides what the inside did, so neither "never cut" nor "recorded as unknown" could hold across the boundary. Putting the effect in the outer file keeps the guarantee and the record where the trace shows them. It can be relaxed later; it cannot be tightened. | builder, to confirm |
| 12 | **A threshold effect call is stale when any input it was built from is stale**, as in 5.4; its answer is then not forwarded. | One rule for threshold calls, already fixtured as issue 17. | follows 5.4 |
| 13 | **`unknown` does not change the `finished` status** (`done` or `exhausted`). | `status` speaks of the budget only. The `unknown` event is in the trace for any reader that wants it. | builder, to confirm |
| 14 | **The live stream is the trace.** Every event of the outer trace, once, in written order, nothing more, notes and `finished` included; offered after the step that wrote it (a microtask in the reference engine), as copies; a consumer's failure is ignored (it fails open). Library option `onEvent` on `runNetwork`; command line `run --stream <path>`, a JSON line per event, to a file or a pipe (not stdout, which carries the answers). The CLI sink never blocks a thread and never holds the run: it opens non-blocking (a pipe needs a reader already open), the trace and results are written first, and a reader that stalls is cut after a 1 s grace. | One definition (the stream equals the trace) is easier to specify, to implement a second time and to test than a second list of events. A copy and a swallowed failure are what fail-open means in-process. A file or pipe lets a consumer in another process keep up on its own. | builder, to confirm |
| 15 | **The conformance runner checks the stream against the events of every run**, and `streamFails` makes the consumer throw. | Testing it once would pin one scenario; testing it in every run pins the order and the completeness against every rule the fixtures know, for free. | builder, to confirm |
| 16 | **The scripted observer is shown `finished` events** (and `unknown`, which has an `id`), and an injection it makes after `finished` names the trace as its `trigger`. | Observers get `finished` (decision above), so the fixtures' observer must be able to, or the rule is untested. A `finished` event has no `id`, and the trace id is the nearest thing the note can name. Runs without an observer are unaffected: it removes only its own injections (8.5). | builder, to confirm |
| 17 | **Sealing is between neurons, not against the holder of the file** (2.6): a composed network run from a local file is sealed from the other neurons and from the outer trace; a boundary that must hold against whoever holds the inside is an `http` neuron to that party's own engine. | Sealing was never a promise against the operator, who can read the file and has the `inner` tool. Saying so keeps the word from promising more than the engine can do. | Kevin's wording in the brief |

## Known limits, on purpose

- The stream's buffer is unbounded (SPEC Appendix C), and an in-process consumer that blocks the thread
  blocks the engine's thread; the command-line form through a pipe avoids that.
- A level 0 program cannot say "it did not happen" (decision 8).
- `unknown` is not held (decision 10), not retried, and not counted in `finished`.
- Effects inside a composed file (decision 11), `http` and `human` effects (not run by this engine).

## What it costs

Against the field budget: one option (the eighth), nothing else in the file. The reference engine grew by
38 lines (1,450 to 1,488 in `src/engine/`), most of it the stream. A neuron's interface does not change: a
program neither sees nor reports that it is an effect, and a file with no `effect` behaves as it did (the
174 earlier fixtures pass unchanged; `demo:typing` and the budget demo print what they printed). The
specification gains section 11 and 9.1, a row in 6.1 and 7.2 and a paragraph in 2.6; the conformance suite gains
17 fixtures in `effect/` and `stream/` and 2 observer fixtures, and 20 mutants.

## Not in version 0.1

Retries, an engine-held `unknown`, usage claims, an owed ledger. A trace explains what was spent and what
happened; it does not prove it to anyone else.
