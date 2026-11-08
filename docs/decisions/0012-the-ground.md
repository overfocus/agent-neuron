# Decision record 0012: the ground (2026-11-08)

**Status: decided (Kevin, 2026-11-08)** — the colony plan, bands 1–3, first for a
recording of `agent-neuron-demos`' colony — **and built.** All three engines pass
224 of 224 (`spec/fixtures`, the 21 of this record included) and 10 of 10
(`py/probes`): the TypeScript reference (engine, fixtures and mutants:
`cc91db9`, `a4dc4b9`, `1370ec8`), the Rust engine (`ae4fcf6`, `22e4cce`,
`43138d8`) and the Python engine (`a83349e`, `e9d9725`, `15875a5`), five
consecutive conform runs identical in each. The demos carry it: the colony's
recruitment, dry patches and seeded drops on `feat/colony-comms` (`081ff13`…
`48c58f9`).

## The problem

The colony demo's pheromone lives in `env.json`, a file beside the run that the
engine never sees. The demo's own note admits it: "what a demo needs from the
engine goes into agent-neuron". `env.json` is the apology for a layer the
whitepaper left planned: a link layer that strengthens with use and evaporates
with time, and a routing that chooses proportionally to that strength
(docs/MULTIPLAYER.md, the colony reading: deposit rule, evaporation rule,
selection rule — "each is spec-writable"). Until the layer exists, the trace
cannot show why traffic moved, the viewer cannot draw the trail dying, and the
conformance suites cannot pin it.

The word `strength` is taken: in TRACE.md it is a signal field counting toward a
threshold ("Until then every sender counts 1", Appendix B). The trail layer is a
different mechanism (attractiveness of a *place*, not the vote weight of a
*sender*), so it gets its own word.

## The decision

1. **`ground` is the tenth top-level field**: `{deposit?, evaporate?, select?,
   base?, power?, seed?}`, all optional; absent means the section does not exist
   and the SPEC behaves exactly as v0.1. It is about how neurons connect, not how
   one decides — it is the link layer's strength, the whitepaper's adaptive one.
2. **The state is per place.** A place is a neuron id. A place's value starts
   absent (0); every value also carries the instant of its last write.
3. **Deposits.** When `deposit` is declared, every reply whose payload carries
   the named key field as a string and the named amount field as a finite number
   writes a place: key names the place, the amount adds to it (a negative amount
   is allowed: a trail can be marked dead). When `by` names a neuron, only that
   neuron's replies deposit. A write ages the old value to the reply's instant
   (`v·exp(-evaporate·Δs)`, the demo's own ageing) before adding — writes store
   the aged value; reads between writes never change the stored one. Each write
   is one `ground` event in the trace and the stream: `{type: 'ground', t, place,
   v, cause: <the answer's id>}`, no signal fields, ignorable by v1 readers like
   `unknown` and `note`.
4. **Selection is opt-in.** With `select: true`, a reply that names no `to`
   forwards along exactly one of the neuron's outgoing links, chosen by weight
   `max(0, base + g[to])^power` from the values aged to the reply's instant, with
   `base` 1, `power` 1 and an engine-wide generator seeded by `seed` (0) as the
   defaults. A reply that names `to` (4.4) and an injection's delivery list
   (4.1) are unchanged. The generator is pinned — one JSON pseudocode block in
   [SPEC 4.5](../SPEC.md), implemented the same in every engine — and the draw
   consumes exactly one word per omitted-`to` reply, whatever the outcome, so a
   run is byte-identical over repeated runs and a `--seed` take is reproducible.
   With a total weight of 0 (every trail dead), the reply forwards nothing.
5. **Check.** `check` refuses a `ground` that is not an object, unknown fields
   inside it, a `deposit` without `on`/`add`, a `by` that is not a declared
   neuron, a negative `base` or `evaporate`, a `power` at or below 0, a non-integer
   or negative `seed` — and a `ground` inside a composed network's inner file
   (2.6), whose scope question waits. The schema grows the same shape.
6. **The name is `ground`,** because that is the demo's word for it ("time
   acting on the ground") and `strength` on a signal means threshold weight.

Payload-field names in a network file: the standing rule ("no key reads a
payload") is about how a neuron decides — gates, filters, transforms are
programs. Deposits are the engine's own bookkeeping, as staleness is keyed by
`replaces` and the desk checks `answerable_by` (0010) against the payload at the
POST: the rule names record fields, routes no work, filters nothing, and does
nothing to a payload that lacks them.

## The colony gains

- env.json keeps only the hunger; the pheromone is engine state, visible in the
  trace and the stream, drawn as trail glow by the viewer.
- The ant replies without `to`: the engine picks the trail. A dead patch (the
  nest marks it with a negative deposit) loses its weight and the traffic moves.
- The nest broadcasts every delivery to the ants as its answer's forward: idle
  nestmates are recruited exactly as returning foragers excite a nest.
- Every take is reproducible: `--seed` seeds the file's draws and the drops.

## Not decided here

- Reading the ground from a program (a colony's ant would read its hunger as
  engine state; today the demo keeps hunger in a file). Rides on durable run
  state (issue #30, item 1), which also carries carry-over between runs — the
  half of Appendix B's strength item this record does not touch.
- Per-link (rather than per-place) strength, the caller-side strength state file
  of SPEC 9, `strength` on signals toward thresholds — unchanged.
- `ground` inside a composed network; a quiescence ("settled") engine trigger —
  stays issue #30 item 4; the colony demo computes its settle beat from colony
  data, since a tick-driven colony always has pending wakes.
- Runtime neuron spawn (Appendix B, hot reload): the demo's random drops are
  pre-declared with a seeded schedule.

## How it is checked

New fixtures under `spec/fixtures/ground/` (21) pin a deposit and its event,
ageing between deposits, a seeded draw (exact outcomes), a named `to` consuming
no word, a dead trail forwarding nothing, one word per omitted-`to` reply, a
refusal per `check` above, and a no-`ground` network forwarding on the v0.1
rule — plus six mutants the ground fixtures must catch (`1370ec8`), the check
refusals unit-tested in all three engines, and a 31-test Python unit suite
beside `py/`. Gates on the branch: `npm run typecheck` and vitest 264/264;
`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`
green, five consecutive identical conform runs in Rust and Python;
`conform py/probes` 10 of 10 unchanged. `agent-neuron-demos`' `npm test` (all
demos headless against the real engine) passes with the colony recruiting on
`feat/colony-comms`; its ground consumption is the colony work on this same
plan.
