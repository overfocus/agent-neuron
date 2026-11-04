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
  deliberate bug each (26 of them). Each must fail a fixture; this is what shows the fixtures bite.

## Specification gaps the fixtures forced a decision on

1. **Early-crash rule** (3.2). The text counted only starts that never reached `ready`;
   the engine (decision 0001) also counted a start that ends within 1 s of `ready` or hangs on a
   ping, and a pong clears the count. Now in 3.2. *For Kevin: this promotes an engine default to a rule.*
2. **Ping interval** (3.2). Unspecified; a fixture sets `pingEvery` and an engine must accept it.
3. **Same-instant ties** (6.2 rule 1). "Scheduled" is now defined for the cases a fixture can reach:
   injections go first, in listed order; calls started by one event wake in `to` order. Two ties
   are *not* decided and no fixture has them: a reply due exactly at `timeout` (the reference engine
   times out; "longer than timeout" suggests the reply should win), and a ping due at the instant of
   an injection. *For Kevin: decide the first.*
4. **Time zero.** The engine's clock starts when every `serve` neuron is ready (only Appendix C said so);
   CONFORMANCE.md 4.2 makes it a rule.
5. **Queued calls when the neuron fails** (3.2): "every input to it is an error". A call already queued
   gets the `start` error at the moment the neuron fails; the call running then gets `crash`.
6. **Notes**: `term` is written only if the process was alive when the group was signalled; `kill`
   only if something was left after 2 s. The `trigger` of an `inject` note is the `id` of the event
   that fired the observer (the text did not say), and takes part in id normalisation.
7. **8.5 made checkable.** "Except those that follow its injections" is operationalised as: events before
   the first `inject` note are identical with and without the observer, and for an `independent` observer
   the traces not rooted at an injection are identical as a whole.
8. **The `http` row of 6.1** has no fixture; the engine does not run `http` neurons.
9. **Level 0 exit by signal**: the text gives `signal S:`; the fixture scripts SIGKILL only.

## An engine bug the fixtures found

Threshold groups were keyed by trace id, and ids are numbered per scope, so two calls of a composed
neuron had the same inner trace id `1` and an inner `threshold` neuron could combine inputs of two
different calls (contradicting 2.6 "new inner trace, so threshold groups never mix calls"). The group
key now includes the scope. Fixture `composed/threshold-groups-never-mix-calls` and a mutant guard it.

## Budget fixtures (v0.1)

`spec/fixtures/budget/` holds the plan of the v0.1 brief as pending fixtures. The names and fields of
`reserve`, `commit`, the `budget` error and the `finished` event (`status`, `remaining`, `held`) are
provisional, written so the fixtures read; the v0.1 spec fixes them. Open: whether a run without a
`budget` gets `finished` events at all (the first fixture says it gets no accounting events).
