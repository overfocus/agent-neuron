# Decision record 0004: the conformance fixtures

Why the fixtures look as they do, and the places where building them found the specification
silent. The format itself is [CONFORMANCE.md](../CONFORMANCE.md).

## Shape

- **A seam, not a second engine.** The engine takes a `Host` (time, timers, starting a program,
  signalling a process group). The real one is the default and is what runs; the virtual one
  (`src/engine/virtual.ts`) is a clock that jumps and fake child processes whose output arrives on
  that clock. So a scripted level 1 neuron goes through the same `Server` code (framing,
  `re`, queue, cancel, ping, restart) as a real one, and a fixture tests the engine's own logic.
- **Directories, not one file.** A fixture's network files stay real network files, so `check`
  validates them and a Rust engine reads them as it reads any network.
- **Prefix for error messages.** SPEC 6.1 fixes only how a message starts; comparing the whole text
  would make fixtures depend on stderr handling and previews.
- **A fixture that expects a harness failure does not exist.** A call with no script entry fails the
  fixture; leaving a call without an entry is how a fixture says "this must not run".
- **Mutants.** `test/conformance.test.ts` runs the fixtures against the engine source with one
  deliberate bug each (33 of them). Each must fail a fixture; this is what shows the fixtures bite.

## Decisions (Kevin, 2026-11-04) and the gaps they close

**A. A reply due at the instant of the timeout wins.** "Longer than timeout" means strictly
longer; a call's outcome due at the same instant as its `timeout` is processed first (SPEC 6.2 rule 7,
CONFORMANCE 4.1). Level 0 had it the other way (the timeout was armed before the program's wake-up);
the engine now starts the program first. Fixtures at both levels and a mutant for each.

**B. No `phase` on an `expired` drop, ever** (SPEC 5.2, 7.2: MUST NOT). A mutant writes one.

**C. Behaviour that lived only in Appendix C or code is normative** (SPEC 3.2, 6.2 rule 6, 6.3): the
over-long line rule (`output` for exactly one running call, else `crash`; killed and restarted), junk
counting every call that holds a slot including cancelled ones, the run waiting for cancelled work
(process group gone, or slot free), and the process notes. One order rule for notes: the event that
reports the call (drop or error) first, then `cancel`, `term`, `kill`; `term`/`kill` only when the group
still had a live member; `restart` at spawn; no `cancel` for a queued or held input; `junk` when the line
is read. Level 0 output over 1 MiB used to write `kill` before the error; it now writes the error first,
so this decision 6 and CONFORMANCE 5.4 say the same thing. The notes are MUST (fixtures with `notes: true`
require them), no longer SHOULD.

**D. Confirmed by Kevin:** (1) the early-crash rule of 3.2, worded: a start fails if it ends less than
1000 ms after `ready` or is killed as hung; a pong, or a life of 1000 ms or more that does not end in a
hang, clears the count (a fixture pins exactly 1000 ms); (2) time zero is when every `serve` neuron is
ready; (3) the 8.5 check as implemented; (4) `error` messages compared by prefix; (5) a call queued
when a neuron fails becomes a `start` error, and the running call gets `crash`.

Other gaps, still as decided while building:

- **Ping interval** (3.2): unspecified; a fixture sets `pingEvery`. A ping tick is skipped while a ping is
  unanswered. A ping due at the instant of an injection is not decided and no fixture has it.
- **Same-instant order** (6.2 rule 1): injections first in listed order, calls started by one event in `to`
  order; a scripted process's exit is known at once (no wake-up of its own).
- **Observers:** they see events with an `id` only (not notes); the `trigger` of an `inject` note is that
  event's `id`; the independence check of 8.5 ignores notes, which name no trace.
- **Refusal:** only an engine's refusal to start counts as "refuses"; any other exception fails the fixture.
- The `http` row of 6.1 has no fixture (the engine does not run `http` neurons); level 0 exit by a signal
  other than SIGKILL is not scripted.

## An engine bug the fixtures found

Threshold groups were keyed by trace id, and ids are numbered per scope, so two calls of a composed
neuron had the same inner trace id `1` and an inner `threshold` neuron could combine inputs of two
different calls (contradicting 2.6 "new inner trace, so threshold groups never mix calls"). The group
key now includes the scope. Fixture `composed/threshold-groups-never-mix-calls` and a mutant guard it.

## Budget fixtures (v0.1)

`spec/fixtures/budget/` held the plan of the v0.1 brief as pending fixtures with provisional shapes.
They are active now, their shapes fixed by SPEC 7.2 and 10 ([decision 0006](0006-budgets.md)): `status`
is `done` or `exhausted`, and `held` was dropped (a trace is finished when nothing is held). A run without a
`budget` does get `finished` events (Kevin's decision), only no accounting.
