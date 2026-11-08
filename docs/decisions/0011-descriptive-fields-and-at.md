# Decision record 0011: descriptive fields, and `at` (2026-11-08)

**Status: decided by Kevin on 2026-11-08 and built.** The three engines pass 203 of 203: the TypeScript reference, the
Rust engine and the Python engine. The two new fixtures are in `spec/fixtures/descriptive/`.

## The problem

The ant colony in agent-neuron-demos is a map: a nest, food at real distances, ants that walk out and back. Its
viewer (agent-neuron-viz) could only place neurons by a force layout, because the network file says nothing about
where a neuron is. Kevin wanted the coordinates in the network file, so that a network's drawing comes from the
file and the engine, as everything else about it does (the viewer's rule: the shape comes from `describe_network`).

But the field budget (SPEC 2.8, CONTRIBUTING) admits a field only if it says *how neurons connect, or how one
decides*, and caps a neuron at 8 options with `effect` as "the eighth and the last planned". A position does
neither: it is for people looking at the network.

## The decision

1. **A second class of field: descriptive.** A descriptive field is for people and tools; no engine may act on it.
   It never changes routing, timing, events, the trace or what a program receives. An engine accepts it, checks
   its shape and may hand it to a tool that describes the network (`describe_network`). `description` was already
   one. The class is closed like the rest of the file: a new descriptive field needs a decision record of its own.
2. **`at` is the second descriptive field**, on every kind of neuron, beside `id` and `description`: `[x, y, z]`,
   three numbers from -1000000 to 1000000, in whatever unit the file's author picks. It is not an option: the
   8-option cap stands, and the test for options (connect or decide) is unchanged.
3. **What it costs.** A neuron's fixed fields go from `id`, `description` and a kind to those plus `at`; every engine's
   `check` learns one shape (three finite numbers); `describe_network` passes it through. The trace's neuron shape
   (TRACE.md) does not carry it: the trace records what happened, and `at` never makes anything happen.
4. **Fixtures pin "acts on nothing".** `descriptive/at-changes-nothing` is SPEC 8.4's worked fixture with an `at` on
   every neuron and the same expected events; `descriptive/refuses-an-at-that-is-not-three-numbers` is a refusal.

## Not decided here

- Any other descriptive field (a colour, a label, a group). Each would need its own record.
- What a viewer does with `at` beyond placing the neuron. agent-neuron-viz scales the coordinates to fit and
  animates neurons it is told walk (a viewer's choice, outside the specification).

## How it is checked

Both new fixtures in all three engines (`conform spec/fixtures` 203 of 203 each; `py/probes` 10 of 10); check tests
for a good and a malformed `at` in the TypeScript (`test/spec.test.ts`) and Rust (`tests/check.rs`) engines; and
`describe_network` passing `at` through (`tests/mcp.rs`).
