# Multiplayer: several people deciding how agents run

**Status: concept note for the maintainer (2026-11-08).** Not a proposal to
build today; no code. It names the layer this note thinks the family is
growing into — several people (in a company, on a team) and several worker
hosts deciding together, quickly, how agent work proceeds — maps it onto the
four SPEC gaps already collected as issue #30, marks the pieces that note
does not cover, and puts a build order down. Nothing here changes the SPEC;
it scopes the next pass.

## The concept, one paragraph

An agent (or a fleet) runs long work in an organisation. At its decision
points, several people need to converge quickly on how it proceeds: approve
or rewrite, choose between alternatives, grant or tighten budget, let an
irreversible effect proceed. agent-neuron's desk (decision 0008) is already a
single-run decision protocol — each item carries the ask whole
(`payload`, `t`, on the desk's host surface), a person answers, and every
accepted answer lands as one auditable line (`desk answer:` on the run's
stderr) with who and when. What it lacks is the multiplayer half: several
people, roles, presence, persistence. The claim of this note: **the four gaps
of issue #30 are exactly the multiplayer features**, and the run is the
multiplayer table — the page is the door people use, an MCP endpoint is the
door workers use, and the network file is the house rules.

## Timbre today: multi-worker, single-player

Timbre's rewrite run already coordinates a crew through the network's own
rules: a writer and two reviewers as separate workers, the reviewers meeting
in `threshold: 2` joins (`net/timbre.network.json`), stale work cut by
`replaces`. No controller decides; the graph is the referee — which is the
family's own pitch, kept. What is single-player: one person holds one token
on one page, per run (0008's deliberate rule). Everyone else in the run is a
worker. Timbre is thus the existence proof for multiplayer *among workers*;
the missing multiplayer is the human side.

## The team problem: slop grenades and invisible lock-ins

A solo developer with a strong harness is extraordinarily productive. The same
harness class aimed at a shared codebase by a team is not merely less
productive — it fails in two specific ways, because the rules travel as one
person's private context:

- **Slop grenades.** Plausible, individually-passing contributions the
  codebase's own norms were never in a position to stop. Each detonates
  later, in maintenance: a duplicated abstraction, a second config system, a
  rewritten helper under a new name. Review-by-review, taste is all the team
  has, and there is no artifact a reviewer can point at.
- **Invisible architectural lock-ins.** The contribution that silently picks
  a library, a data shape, a coupling: the decision is made inside a diff,
  and the team inherits it without ever having been asked. Lock-ins stay
  invisible exactly until they are load-bearing.

Both are failures of a missing surface, not of the agents. The rules existed;
they lived in prompt context — private, per developer, unaudited,
unversioned. The claim for teams is that the rules can travel as **data on
the network instead**: diffs to review rather than context to trust.

## The DNA of the neurons

Every kind of rule the team wants to govern is already a field on, or a
neighbour of, the network file:

| Team rule | Where it rides | Today |
|---|---|---|
| What an agent may spend | the file's `budget` and each neuron's `cost`; a manager may lower a pool mid-run, never raise (decision 0006) | built |
| What must never happen without a person | `effect: true` — never cut, `unknown` on crash, never retried (decision 0007) | built |
| How many must agree | `threshold` N-of-M distinct senders on one trace (SPEC 5) | built |
| What the neuron may read | the file's `env`: the engine passes through, by name, only what the file names (SPEC section 0) — a spec doc the network chose to hand it, not whatever the agent globs | built |
| Who decides | `answerable_by` on the item payload, checked at the desk's POST against the run's grants (decision 0010) | built |
| How the work is judged | the guide and rules as data — timbre's proven pattern: the guide is a draft to correct, learning folds the person's edits back, reviewers score each answer against the guide, and keep-token invariants fail a rewrite before anyone approves | built, on prose; generalises to code review against the spec |
| What supersedes what | `replaces`, `maxHops`, staleness — the amended rule cuts the old rule's running work (SPEC 5.4, 7.2) | built |

Two conclusions, stated plainly:

- **Specs and guidelines travel as payloads and named files, not prompt
  slop.** A team's spec is introduced the way timbre's guide is: a document
  the run carries, named in `env`, scored against, held as keep-tokens where
  it is load-bearing. When the spec itself changes, the change is a decided
  contribution on the same surface — the policy repo is diffable, versioned
  in git, and approved by the same people who approve the code.
- **The decision record is the audit no harness keeps.** Every vote, accept
  and refusal lands as one accepted line at its POST, beside the trace that
  shows which rule fired. Months later the team can say who approved the
  lock-in — because it was never invisible: it was an item someone approved,
  by name, inside a budget and gates that live in git.

The demo in this file's build order shows one slice — one quorum, one gate,
one effect. The DNA layers stack on that same surface without new engines:
each is a field in a file the team already reviews.

## The axes

| Axis | Today | The org wants |
|---|---|---|
| Workers | many, per step, coordinated by threshold + staleness (built) | workers on other hosts joining via MCP; claim races decided by `replaces` |
| Humans | one, the token holder (0008) | several, in roles: co-author/editor/lead — some answer some items |
| Presence | none: the page never says an item waits (issue #30 item 3) | "a decision needs you" in the pocket: badge, ping, Slack |
| Persistence | the hold dies with the run (0008 "not in this step"; 0007 row 10) | the table persists between visits (meetings, timezones, restarts) |
| Closure | "the whole run settled" has no trigger (issue #30 item 4) | the table closes when the round is done |

Two doors, no conductor: the page answers; MCP starts and watches runs. A
multiplayer *server* that assigns items, holds locks and arbitrates would be
the central controller the whitepaper positions against. The engine — or a
re-armed instance of it — stays the referee; the website is surface.

## What is already protocol-native

| Org decision pattern | Where it lives |
|---|---|
| Quorum ("two engineers agree") | `threshold` N-of-M distinct senders, `within` (SPEC 5) |
| Review quorum, then a lead's yes | timbre exactly: two reviewers meet at `threshold: 2`, then the person |
| Irreversible actions need a person | `effect: true` — never cut, `unknown` on crash, never retried (SPEC 11; decision 0007) |
| Spend decisions mid-run | budgets (decision 0006): per-trace pools; a producer may *lower* one when it injects, never raise — a manager tightening an agent's budget is a protocol operation |
| Decisions that expire | `replaces`: a newer input cuts stale waiting and running work — an ask answered after the situation changed does not linger |
| Choose between alternatives | the page's shapes: draft against proposal (Appendix D), "ok 1 / ok 2" rounds in timbre |
| Escalation chains | ordinary links and routing: a lead's neuron listens where the IC's answers arrive |
| The compliance record | the desk's answer lines land as accepted, in the moment — with item, kind, text (the family's one-line recording convention) |

## The colony reading

The "neuron" name is already a loose analogy (the README says so). Ant
colonies earn a closer look in this note because they are the canonical
working systems for three things the family cares about: coordination
without a manager (stigmergy), the strengthen-and-weaken link layer this
family has left planned, and scale-out worker behaviour of exactly the
shape the claiming axis needs (response thresholds). The mapping, per
mechanism:

| Colony mechanic | In agent-neuron | Built? |
|---|---|---|
| Stigmergy: coordination through deposits in a shared environment, not messages (Grassé) | the run folder, the desk and the trace are that environment; open asks are the trail the next worker or person reads | built — the desk and trace already work this way |
| Pheromone reinforcement: a useful trail strengthens with each pass | strengthen-on-use: an event deposits strength on the links its chain crossed; `weight()` today always returns 1 | links: planned (whitepaper, "adaptive (planned, not built)") |
| Evaporation: unused trails decay | staleness is built for *work* (`replaces`, the `'expired'` drop); link-strength decay between runs is not | work: built; links: planned |
| Response thresholds: a worker self-selects when a stimulus passes its own bar; no scheduler assigns tasks (Bonabeau et al.; Gordon's task allocation) | `threshold` is already per-neuron — a worker host acting on "an item waits for me" is a threshold neuron over exactly that stimulus | semantics: built; network-hosted workers: the claiming axis below |
| Alarm and recruitment: cheap, scoped, local signals bring the right neighbours | the page-pending notification with a token-scoped handle | gap (issue #30 item 3) |
| Polyethism: role specialisation with no identity infrastructure | grants and roles on the desk | gap (issue #30 item 2) |
| Nest state that outlives any individual: the trail persists between visits | durable run state (holds, grants, deposits) beside the run | gap (issue #30 item 1) |

Three conclusions the reading gives the build order, stated honestly:

1. **The colony names the mechanism the whitepaper left open.** The planned
   adaptive layer (connections strengthen and weaken, carry over between
   runs) has a working external model with a handful of local rules:
   reinforce on use, evaporate over time, choose proportionally to
   strength at a decision point. Each rule is spec-writable — a deposit
   rule (which events strengthen which links), an evaporation rule (what
   decays, when), a selection rule (which is threshold again). What it
   costs: carry-over between runs is colony-style durable state, so the
   learning layer waits on the same key as async play — issue #30 item 1
   (durable run state) is the keystone under both.
2. **No scheduler is the shared discovery, not a borrowed one.** Colonies
   coordinate at scale with nobody assigning tasks; this family's pitch —
   no central controller, the graph is the referee — is the same
   statement. The protocol shape for claiming a worker axis is then
   colony-shaped: workers self-select on stimulus they are gated for
   (grants as the threshold table), duplicates are tolerated and pruned by
   staleness, and the house rules (the network file) say which stimulus
   opens which bar.
3. **Duplication is a price, priced.** Colonies pay for redundancy among
   foragers; in money terms this family already measured the analogue — a
   claim race is roughly 2× the single call ($0.0994 for the two-hunk
   bridge edit, 2026-11-08). The colony reading adds the discrimination:
   tolerate duplication when the item is cheap, respond to the threshold
   when it is dear — budgets are where that line is drawn, per trace.

Design references (named, not pinned): stigmergy, P.-P. Grassé; response
thresholds and self-organised task allocation, Bonabeau, Dorigo et al.;
colony task allocation under fluctuating stimulus, Deborah Gordon; ant
colony optimisation as the algorithmic form, Dorigo & Stützle. The discipline
the README's analogy sentence sets holds here: the colony is a design
reference for the unbuilt layers, not biology cosplay — every mapping above
lands on a named rule the SPEC could carry or already carries.

## The org layer, in build order

Cheap to expensive; 1–4 implement issue #30's items in this order, each
keeping the boundary rule from the lenov analysis (2026-11-07): **MCP moves
payloads; the page moves people; the desk moves answers.**

1. **Grants on the desk** *(built: decision 0010, `run --grants`)*. Who may
   answer which item, from a grant list (a JSON file today, the org's
   directory later), validated at the desk's POST;
   anyone outside gets a refusal the desk logs the way it logs the person's
   refusals on stderr today. The token stays the root; the org shape rides
   on it. No new identity system. *(issue #30 item 2)*
2. **Durable run state.** Open asks, grants and answers persist beside the
   run (the run folder, never the vault/manuscript); a restarted run re-arms
   the table with the same key and owner, and answers ride the same stderr
   contract for the new run. *(item 1)*
3. **Presence.** A page-pending event on the MCP job pattern (the M3 surface
   already emits per-second job notifications): item id + page URL + a
   token-scoped handle; the *contents* of an answer never cross MCP. Hosts
   act: badge, ping, Slack webhook. *(item 3)*
4. **Quiescence.** "The whole run settled" as the gate trigger. The traced
   approximation first (no open desk items, no pending wakes — the ids
   header makes two reads of one run agree which event is which), the Flow
   answer second. *(item 4)*
5. **Decision kinds** *(choose-between-options built: decision 0010)*. What
   #30 does not cover: choose-between-options and
   delegate/escalate are payload and page surface first; parameter changes
   (budget, worker choice, timeout) want small protocol verbs with the same
   recording conventions. Keep the list closed — every new kind is a
   vocabulary cost for every engine.
6. **Worker claiming, measured before built.** A step that n workers may
   take: start as honest races (two workers, `replaces` keeps the first
   valid answer — speculative work the protocol already dedupes), with
   budgets bounding the spend. The family measures before deciding: the
   bridge run of 2026-11-08 cost $0.0994 for a two-hunk edit — three model
   calls, the writer carrying both hunks in one call (~$0.04 flat per
   call, so per-item cost falls with batch size). A claim race is
   ~2× the writer call; whether the second-best rewrite is worth it is a
   measured question, not a felt one.

## Where this sits in the open questions

The **Flow** open question ("do neurons run in one program, as separate
local processes, or across machines?") is answered in the org form: across
machines, via MCP, with the run record as the shared state — and the
whitepaper's "not a wire protocol between parties" stays true: the parties
share a *run*, not a bus. The **Composition** open question (how a composed
neuron's learning shows outside) is untouched: learning stays planned.

## Deployment shape

An org runs a hosted engine beside its site: agent workers connect through
MCP (their harnesses, Claude, scheduled workers), every human decision point
lands on one role-aware, persistent page, budgets bound what any worker may
spend, and network files are the standing policy per workflow. The fences
from the MCP-exposure analysis apply unchanged: file arguments inside an
allowlist, no `https` (and no unguarded `http`) from internet-reachable
runs, per-token budget and concurrency caps.

## Decisions wanted

1. Frame the next SPEC pass as this pass — the four #30 items get their
   shapes from sections 1–4 above, in this order.
2. The first spike: **grants on the desk** in the Rust engine (a grant list
   file, validation at the desk's POST, tests beside desk.rs), proven on a
   real timbre-style run as the bridge run was.
3. This note's home is `docs/`; it stays a concept note until a decision
   record (0010) is written.
