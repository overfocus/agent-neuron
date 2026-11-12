# Assessment: the SPEC, where it stands, and the gap to "a better alternative than an agent harness"

**Status: assessment for a go (2026-11-10).** Read first: [POSITIONING.md](../docs/POSITIONING.md)
(the framing accepted 2026-11-10), [MULTIPLAYER.md](../docs/MULTIPLAYER.md), [DURABLE-STATE.md](../docs/DURABLE-STATE.md),
[docs/P2P.md](../docs/P2P.md), [decision 0014](../docs/decisions/0014-engine-lifecycle-audit.md). Nothing here changes
the SPEC, the schema, a fixture or a decision record; a wave taken becomes its own
decision-record-first contract commit per [references/decisions-and-spec-changes.md] in the
family skill. The question put: version 0.1 of the SPEC is the coordination contract; what
is missing between it and the day a team picks it *instead of* pointing an agent harness at
the work?

**Note, 2026-11-12:** [decision 0023](../docs/decisions/0023-one-engine.md) has since deleted the TypeScript library,
the TypeScript reference engine and the Python engine, and moved `py/probes` to `spec/probes`. Where the text below
names them (`src/`, `py/`, vitest, the mutants), it describes the tree it was written against.

Gates on the tree this assessment reads (`17c8e22`): `main` has moved two docs-only commits
since the gates were measured at `290652e` (`b701697` DURABLE-STATE, `17c8e22` POSITIONING),
so the quoted gate numbers — `cargo test` 123, `conform spec/fixtures` 224/0/0 on identical
reruns, `conform py/probes` 10/0/0, `npm run check` (vitest 264) — are unchanged by argument
and re-run before any wave starts.

## Ground truth: what the SPEC pins today

Every row verified against the tree this session.

| Area | Spec'd | Pinned by | Where things stand |
|---|---|---|---|
| Network file: closed schema, literal values, kinds (`source`/`run`/`serve`/`http`/`human`/`network`), 8 options | SPEC 2 | `spec/network.schema.json`, `check` (2.7) positions | complete; field budget 2.8 (10 top-level fields) enforced by the rule that a new field must say how neurons connect or how one decides (`CONTRIBUTING.md:126` demands the decision record) |
| Per-neuron deciding: `threshold` (distinct senders, one trace), `within`, `replaces` staleness (cuts held *and* running work), `maxHops` | SPEC 5 | fixtures `threshold/`, `staleness/` | the core, and the part nothing else in the market has as a written, conformance-checked contract |
| Failure taxonomy + same-instant event order (6.2's 8 rules) | SPEC 6 | 6.1 table rows, `runend/`, notes fixtures | complete; two engines byte-identical on it |
| Trace v1 + replay, live stream (the stream is the trace, fails open) | SPEC 7, 9.1 | `stream/`, recorded traces | complete |
| Budgets: per-trace pools, reserve/commit, composed caps, refusals fail closed | SPEC 10 | `budget/` | complete, except usage claims (`basis` is `estimated`; SPEC 7.2, 10.4) |
| Effects: never cut, `unknown` on an ambiguous failure, idempotent-by-id reconcile pattern | SPEC 11 | `effect/`, `observer/` | complete (reconciliation is a pattern, no machinery, 11.4) |
| The ground: strength per place, evaporation, seeded one-link draw, pinned RNG | SPEC 4.5 | `ground/` | in-run only; carry-over waits on durable state (Appendix B) |
| Composition + sealing | SPEC 2.6 | sealing fixtures | complete; sealing is between neurons, not against the file's holder (SPEC 2.6) |
| Human neurons: the desk, grants, choose-options, the record line | SPEC 3.4 + Appendix D (0008, 0010) | `tests/desk.rs` 17 | single-run, 127.0.0.1, one token holder; dies with the run |
| Peers/placement | **not in the SPEC** — engine + side file | `tests/peers.rs` 26 | P0/P0.5/P1/P2 built (0013); SPEC text is P5, pending |
| Persistence | **not in the SPEC** | none | planning note only ([DURABLE-STATE.md] shape C, P0–P5 not started) |
| Identity/auth on any boundary | **"There is no authentication in version 0."** (SPEC 3.3, `SPEC.md:428-429`) | — | peers use pre-shared tokens on 127.0.0.1 (0013); `https` refused at plant (`rust-engine/README.md:156-158`) |

Engines (as of this assessment; decision 0023 has since kept only the Rust engine): the TS library (`src/`) is the pre-SPEC library whose differences Appendix C
catalogues; the TS reference engine (`src/engine/`) is a conformance oracle; the Python
engine is text-only proof (scripted, no processes); the Rust engine is the only one that
runs real networks end to end — and the family's production engine (timbre drives it).
Nothing is on npm (`README.md:7`); there is no CI (`rust-engine/README.md:459`, the gate
is local).

## The constraint map

Any "gap fix" lives inside these, or it needs a decision record that overrides them.

1. **2.8's field budget is real.** Presence, journal, placement never become top-level
   fields; they ride side files passed at run — the `--grants`/`--peers` pattern (0010,
   0013). SPEC 2.8 says exactly this.
2. **No central controller.** A multiplayer server that assigns holds locks or arbitrates
   is the thing the whitepaper refuses (MULTIPLAYER.md "Two doors"; the store answers what
   was deposited, the graph decides). Any gap fix that looks like a scheduler is out.
3. **The whitepaper's §7 line stands:** single-engine text, "not a wire protocol between
   parties" — peers share a run, not a bus (0013).
4. **std-only in the Rust engine** (nothing in `Cargo.toml`): no TLS, no async, no crypto.
   Auth/https shapes pass through this constraint or amend it explicitly.
5. **A default that does nothing** (family rule): every new surface — journal, presence,
   peers — is opt-in, so conform stays 224/0/0 untouched.
6. **The brain stays out of the engine** (POSITIONING, accepted 2026-11-10): a decomposer
   may compile to a network file one level up; no SPEC item may grow a planner into `Engine`/`Engine::run()`.

## The honest comparison

The SPEC's coordination core is already ahead of every harness on the axes a team feels:
rules as data a reviewer can diff (MULTIPLAYER "the DNA" table), a spend ceiling the engine
enforces before a call starts (SPEC 10.3), staleness that cuts *running* model calls (5.4 —
harnesses don't), `unknown` instead of a lying `error` (11.3), and a trace whose event order
a second engine must reproduce (6.2, 8). A harness has four things agent-neuron has no text
for at all — and those four are the whole gap:

| A harness gives | agent-neuron today | The gap |
|---|---|---|
| An agent with a loop that decomposes tasks | nothing — neurons wrap programs; a wrapped harness is a neuron | not a SPEC gap (constraint 6); a *compiler* one level up. Recorded, not built. |
| The run survives the session | "dies with the process": open holds, grants, trail, accepted answers (DURABLE-STATE problem section; issue #30 item 1) | SPEC gap №1: persistence (shape C, P0–P5, planned 2026-11-09, not started) |
| A person talks to it from anywhere, any time | desk bound to 127.0.0.1, one token holder per run, the page closes when the run ends (Appendix D; 0008's deliberate rule) | SPEC gaps №2–4: org directory (grants→journal), presence, quiescence, a hold that survives a restart (#30 items 2–4; DURABLE-STATE P2–P3) |
| You `npx install` it and go | clone, build Rust, run `npm run check`; no package, no CI, no release | not a SPEC gap: distribution (npm/pip binary per Appendix B, CI absent, audit findings 10–17 queued) |

So the gap decomposes exactly. **The coordination contract is done-shaped; the missing SPEC
text is persistence, the multiplayer desk, and the distribution contract; and the missing
product is the door.** Use-case honesty confirms where this lands: the solo "one harness"
case is a weak fit the repo never claimed (USE-CASES ranks by fit; a scheduled job outranks
a swarm there), while the team/org case is the strong one the README already leads with
(`README.md:11-42`) — and that case is blocked by #30 items 1–4, not by anything in the
contract.

## Waves

Order carries the family's already-accepted build orders; each wave is decision-record-first
where it touches the contract.

- **W1 — durable state (DURABLE-STATE P0+P1).** The folded-deposit list generalising 0012
  (P0: run-visible behaviour unchanged, conform 224/0/0 by argument), then `--journal` with
  the identity refusal and re-arm (P1). Tests specified in the note: *a run killed mid-hold
  re-arms with the same key/owner*; *a journal from a different network file refuses*.
  Gate: fmt, clippy `-D warnings`, `cargo test`, conform identity. The keystone: carry-over,
  async play, the surviving hold all wait on it (DURABLE-STATE and MULTIPLAYER both name it).
- **W2 — the table finished (DURABLE-STATE P2+P3 = #30 items 2–4).** Grants and the
  directory as journal deposits; presence as a page-pending event on the MCP job pattern
  (item + URL + token-scoped handle; contents never cross MCP); quiescence's traced
  approximation reading the journal; the human hold that survives a restart. Gate: desk and
  MCP suites; a restarted run's page shows the same items with owners.
- **W3 — the distribution contract (0013 P3+P4+P5, after HANDOFF items 3–4).** The engine
  cancel handle a hosted graph's engine reads; the graph boundary's `replaces` and composed
  `cost`; then the ground over the wire (folded list + `deposit` + gossip, gate: a two-peer
  colony drawing the same numbers as one engine), distributed quiescence (gate: a run ends
  only when every peer is empty; a silent peer fails honestly), and the contract pass:
  decision record, a SPEC section for placement and the wire's normative parts, TRACE.md's
  merged reading, a scripted-peer fixture area (fixtures never open sockets).
- **W4 — hygiene that blocks trust.** Audit findings 10–13 then 14–17 (0014 queue: deep
  JSON cap, unknown-source refusal, refusal teardown, descendants, serve generations,
  envelope checks, stream self-check, dead helper); SPEC questions #24/#21/#20/#19 resolved
  (cross-engine disagreement on a composed exit at its timeout instant is the one both
  engines feel); the http failure-row fixture (0004). No new surfaces; makes the third
  engine's claims watertight.
- **W5 — the door.** CI running the family gate on every push (`.github` absent today);
  the npm/pip binary per Appendix B's "esbuild and ruff" pattern; the harness-as-neuron
  recipe documented in-repo (a Claude Code-class CLI as a level 1 `serve` neuron with
  `env`/`timeout`/`effect` and the MCP door alongside); one demo with a model wired in and
  real spend numbers on the trace. Gate: install-and-run from the package on a clean tree.
- **Not a wave, parked:** the brain/compiler (constraint 6). Its deliverable is a decision
  record *saying* it sits outside the engine; its build, if ever, starts as its own
  assessment after W1.

## Non-goals

- Teaching the engine to plan (constraint 6). A harness keeps its loop; the network takes
  its coordination, budgets, and record.
- A multiplayer/despotic server that assigns items (constraint 2).
- Value leaving the trace: owed ledgers, settlement, spend-per-window stay out (SPEC 10.8).
- Windows, Wasm, hot reload, byte-level fixtures (Appendix B; untouched by the goal).
- Beating a harness at solo work. The claim is team and fleet runs; USE-CASES already
  ranks the solo cases weak, and saying so is part of the assessment's honesty.

## Pending decisions (answer by number)

1. **Wave order.** Recommended: W1 first (keystone; two accepted orders name it), W2 next,
   then W3; W4's #24 and the findings 10–13 slice can ride in parallel with W1 as a
   second track; W5's CI slice immediately regardless. Alternative: W5 first on the
   argument that nothing matters until someone can install it.
2. **Auth/https policy** (blocks W4 and any internet-reachable run; MULTIPLAYER's fences
   already say "no `https` … from internet-reachable runs"). Recommended: keep the core
   std-only; auth stays a wrapped boundary or an explicit pre-shared-MAC trust mode that
   says so on stderr (the P2P note's line). Alternative: add a TLS dependency and lose the
   "std-only, nothing fetched" property the whitepaper leans on.
3. **Version policy.** Recommended: keep network `version: 0`, additive through W3's
   contract pass, and make the 1.0 call *then* (SPEC header: "may change in any way until
   it is called 1"). Alternative: open a 1.0 review now and batch the contract passes.
4. **The parked brain.** Recommended: record the compiler position as a decision pointing
   at POSITIONING, with no build order. Alternative: commission its assessment after W1
   lands.
5. **The TS library.** (Decided otherwise 2026-11-12: decision 0023 deleted it.) Recommended: leave as conformance oracle this pass; the 91-mutants
   port stays HANDOFF item 9's precondition. Alternative: freeze it harder now (README
   claims rewritten), which costs review time the waves don't have.

## Decisions

**Accepted (Kevin, 2026-11-10, "go with recs"):** every recommendation above —

1. Wave order W1 (durable state, [DURABLE-STATE.md](../docs/DURABLE-STATE.md) P0+P1) first;
   W4's #24 and the findings 10–13 slice ride parallel; W5's CI slice immediately. W2 next,
   then W3.
2. Auth/https: the core stays std-only; auth is a wrapped boundary or an explicit
   pre-shared-MAC trust mode that says so on stderr. No TLS dependency.
3. Version: network files stay `version: 0`, additions additive; the 1.0 review happens at
   the W3 contract pass, not before.
4. The brain: a decision record stating the compiler position pointing at POSITIONING.md —
   no build order, no engine change. (Recorded here; 0015 is W1's record, not this.)
5. The TS library (deleted since, decision 0023): conformance oracle this pass; the 91-mutants port stays HANDOFF item 9's
   precondition for going further.

W1's own build answers are [decision 0015](../docs/decisions/0015-durable-run-state.md),
written before its code started.
