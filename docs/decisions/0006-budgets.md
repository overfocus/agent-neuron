# Decision record 0006: budgets and the finished event (spec version 0.1, 2026-11-04)

Version 0.1 of [SPEC](../SPEC.md) adds spend limits and a per-trace `finished` event (SPEC 10,
7.2). The scope came from a proposal on value and cost accounting and four reviews of it (a
cloud-cost view, a payments view, a ledger view and a multi-agent-systems view). The decisions
below are Kevin's. Some were proposed by the builder while building and then confirmed by Kevin; they
are marked **confirmed**.

## Kevin's decisions

| # | Decision | Reason |
|---|---|---|
| 1 | Consumed cost commits on every outcome: an answer, quiet, a failure, a timeout, a cut, a crash. When the real figure is unknown, the reservation is committed and marked `estimated`. | A process that was started has spent what it spent, whether or not it answered. Releasing the reservation of a cut or crashed call would let a bad neuron or a stale input run for free, and would reward failing. Only an amount *owed to another party* (a later version) is released when there is no answer. |
| 2 | A top-level `budget` of named units; a `cost` per neuron that is both the reservation and the cap; the commit is the lesser of a claim and the reservation. | One declared number per neuron is enough to bound a trace and to read the bound from the file. A neuron cannot spend more than it declared, so the reservation is a true ceiling. |
| 3 | The pool is per trace. The file sets the ceiling; a producer may lower it at injection, never raise it. | The trace is the unit that is already isolated (threshold groups, `finished`). The producer knows what an input is worth; the file's author knows what the network may spend. Only the file may raise a limit. |
| 4 | A call that cannot be covered in every unit is refused as an `error` with code `budget`, not as a new kind of drop. | A refusal is a call that failed to start, and "a failure fails one call" (6.1) already says what happens. A drop means stale or expired work; a reader that filters drops would hide the spend limit. |
| 5 | Held inputs do not reserve. A threshold call reserves once, when it acts. A level 1 call queued for a slot reserves when the slot frees. | A hold has started no program. Reserving per held input would charge a group several times and starve the call it exists to feed. A queued call that reserved would hold money while doing nothing. |
| 6 | Branches draw from the pool in `to` order, the order of delivery (4.2). | The order is already deterministic (6.2); a second order for money would be a second thing to specify. |
| 7 | A composed neuron's `cost` caps its inside, and one total crosses the boundary. | Sealing (2.6) holds: the outside never learns what happened inside, only what the composed neuron was allowed to cost. A paid boundary is an `http` neuron to someone else's engine, whose accounting is its own. |
| 8 | A network with no budget writes no `reserve` or `commit` notes. | The simple case stays simple, and a trace without a budget stays as it was. |
| 9 | `maxHops` is still required for a cycle. | A free loop (cost 0) never runs out of budget; only `maxHops` ends it. A budget is a second bound, not a replacement. |
| 10 | Every trace ends with a `finished` event, with or without a budget; with a budget it carries the remaining amounts and a `status` of `done` or `exhausted`. | A reader of a trace should not have to infer its end. Additive under trace v1. The scripted observer of the fixtures sees events with an `id`, so it does not see `finished`; observers get it with the normative live stream of feature 2 (SPEC 9.1). *As refined by [0007](0007-effects-and-the-live-stream.md): the scripted observer is now shown `finished` too.* |
| 11 | Existing fixtures do not get `finished` events added. A fixture opts in with `"finished": true`, as it does for notes. | A change to the trace should not rewrite 150 fixtures that are about something else. |
| 12 | The neuron option count rises from 6 to 7 for `cost` (8 for `effect`, next). The network file gets a ninth top-level field, `budget`. | `cost` says how a decision is bounded, and the engine, not a program, must enforce it: a program that declared its own cost could understate it. Same class as `timeout` and `maxHops`. |
| A | **Under a budget every neuron but a source MUST declare a `cost`, composed neurons included.** `check` refuses a file with a `budget` and a non-source neuron without one. A cost of 0 is allowed: the neuron is metered and timed (zero amounts, and its `ms`). | Fail closed. A neuron that could spend without declaring it would escape the pool, and a forgotten cost is far more likely than an intended free neuron. Zero says "free, on purpose" and keeps the `ms`. |
| B | A composed neuron with no `cost` (possible only when the outer network has no budget) is bounded by its inner file's own `budget`, used as the inner pool. If the inner file has no budget either, the inside is unmetered, as in version 0. | A file that declares a budget means it, whether it runs alone or inside another. Nothing outside needs to know. |
| C | When a composed neuron has a `cost` and its inner file has a `budget`, the inner pool is the lower of the two, unit by unit. A unit the cost names that the inner budget lacks is limited by the cost; a unit the inner budget has that the cost lacks is not in the pool (refused inside). Neither file can raise the other's limit (SPEC 10.5). | The author of the outer file decides what a call to the neuron may cost; the author of the inner file decides what it may spend. Each is a limit, so the strictest holds. |
| D | The builder's other choices, confirmed (below). | |

## Builder's choices, confirmed by Kevin

1. **Amounts are JSON integers from 0 to 2^53 - 1, never strings.** 9007199254740991 is the largest
   whole number every JSON reader holds exactly. A string would make every reader parse and every
   fixture quote, to cover amounts nobody needs: a unit that needs finer steps is counted in smaller
   ones (cents, not dollars). `check` rejects anything outside the range. If a later version needs
   bigger amounts, strings can be added then, as a new type for `units`.
2. **`ms` is always the engine's measurement, and is not labelled; `basis` labels the units.**
   `basis` says `estimated` now and gains `measured` when a neuron can report usage.
3. **`status` is `exhausted` only if a call at that level was refused for budget, else `done`.** A pool
   spent to exactly 0 with nothing refused is `done`; a failed trace is `done` (failures are `error`
   events). **A refusal sealed inside a composed neuron shows as `done` outside**: sealing means the
   outside sees only the composed neuron's own failure (code `inner`).
4. **`held` was dropped from `finished`**: a trace is finished only when nothing is held, so the count
   would always be 0.
5. **The network file's `version` stays `0`.** The new fields are additions; the spec document is
   called 0.1 and the schema `$id` is unchanged. An engine that does not know `budget` or `cost`
   rejects the file, as it rejects any unknown field.
6. **The message of a refusal is `budget:` and what was short** (`budget: needs 6 credits, 4 left`);
   fixtures compare the prefix.
7. **The injection field is `budget`**, an object of units to amounts. An injection that raises a
   ceiling, names a unit the file lacks, or gives a non-integer is refused to the producer before the
   run. A network file that does not pass `check` is refused the same way (both are `refuses` fixtures).
8. **The `term` and `kill` notes are written before the signal is sent**, not after. Where a process
   ends at once (the virtual clock, a real process that exits quickly) the call's end, and so its
   `commit` and the trace's `finished`, would otherwise come first.
9. **`finished` follows the last event of the trace** (6.2 rule 8); a process note that follows its
   cause at the same instant (the `kill` after an over-long output's error) comes after it. The inside
   of a composed neuron counts as over when the composed call ends.
10. **The `reserve` and `commit` notes of an inner trace stay inside** with the rest of the inner events.
11. The spec section for budgets is a new section 10, so existing references keep their numbers.

## What it costs

The reference engine grew by about 90 lines (of 1,450 in `src/engine/`); the spec by a section (10)
and a few rows. A neuron's interface does not change: a program neither sees nor reports a cost in
version 0.1, and a network with no `budget` or `cost` is the same file it was.

## Not in version 0.1

Usage claims in level 1 replies (so that a commit can be below the reservation), an owed ledger,
spend per source or per time window, settlement observers, and anything that moves value. A trace
explains what was spent; it does not prove it to another party. Cost never feeds strength on its own, and
approval gates stay unweighted, when strength lands.

Next in 0.1 (feature 2, built: [decision 0007](0007-effects-and-the-live-stream.md), which refines the wording below: a *started* call is never cut, waiting work is dropped as usual, a hung call is killed at its timeout, and `unknown` is recorded, not held by the engine): the `effect` option (staleness and the engine's timeout never cut the call; its
answer is accepted and committed; only the forward is suppressed when the input went stale), the
`unknown` outcome after a crash or hang of such a call (held until reconciled), a normative live trace
stream for observers, and the sealing wording: a local composed network file is sealed from other
neurons, not from whoever holds the file.
