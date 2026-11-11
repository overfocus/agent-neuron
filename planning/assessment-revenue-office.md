# Assessment: the Office — an agent-neuron product around kevinferron.com

**Status: planning assessment, no code (2026-11-10).** A network of neurons run as a
marketing and engineering team whose goal is revenue for the portfolio site's
practice. The family procedure applies: after the numbered go below, the Decisions
section records Kevin's answers dated and attributed, and that commit lands on main
before any `feat/`.

## The verified problem

The site (kevinferron.com, the `overfocus/portfolio` repo) sells a local practice's
offers — small-business websites at $49/mo, Elite AI Enablement via scoping calls,
custom development, WISP security setup — through 21 business-type pages and a
concierge funnel. Its growth work runs wholly at human cadence today: channel posts
that a tool builds by hand (`docs/social.md`), analytics read in the GA4 console
(`docs/analytics.md`), improvement tickets whenever Kevin gets to them. The
mechanisms an office needs are already specified on the site as data:

- **The gauge set exists as an event dictionary**: `generate_lead` with
  `lead_source`/`lead_type`/`value` (website 588, scoping call 150, concierge 500,
  general 150, partner 300 — `data/leadValues.ts`), `purchase`
  (`transaction_id`, $49/mo website plan), `begin_checkout`, `cta_click` per
  campaign id, and the concierge/pain funnel events. Lead values are already
  Kevin's own expectations, not prices.
- **The effect machinery exists**: `npm run social -- new` builds channel links by
  the utm rules (lowercase, hyphens, ≤40 chars, campaign `<offer>-<initiative>`),
  `data/socialCampaigns.ts` is the registry, and the x-post / linkedin-post /
  facebook-post skills post them. The Google Business Profile already has a
  per-trade Update precedent (2026-11-05: four trades, `gbp`/`local`,
  `websites-trades-202611`).
- **The ship flow is a desk**: `npm run ship -- --approved "<his words>"`, draft
  gates for copy (DRAFT flags), `data/updates.ts` announce entries before every
  merge, and the site's own check suite (`check:home`, `check:layout`,
  `check:for`, `npm run verify`) is the gate an engineering team's PR must pass
  before anything merges. "Never push or ship without Kevin's go" is already the
  house rule; the desk makes it a mechanism.

So the gap is not machinery — it is that nothing schedules the senses, the
briefs and the hands between Kevin's decisions. That is the network's job, and every
mechanism the SPEC has (detectors, corroboration thresholds, staleness, budgets,
effects, human holds, durable asks, learned strength) has an org analog. The office
needs no outside data vendor: its inputs are the gauges and channels the practice
already owns — GA4, the enquiry ledger, the Google Business Profile, the
newsletter — and acquisition experiments are proposed inside the network itself.

## The roster (roles for the neurons)

Each role is a neuron or a small composed set. The network file is the org chart;
the two engines pattern (the 0013 `companies` shape) holds: a marketing graph peer
and an engineering graph peer are the teams, and one program peer (the front desk)
hosts every effect. Waves tell when a role goes live; W1 is the invented-data
skeleton.

**The page (human, 0008)**

- **Principal** — Kevin, the run's one desk. Every publish, ship and spend ask
  lands here; every accepted/rejected answer is the learning signal; won or lost
  engagements are recorded by a desk answer, never by a scraper. A lost-engagement
  answer exists so the ledger can record a real revenue number, not a proxy.

**Marketing team (graph peer `marketing`)**

- **Marketing manager** (Wave 1; model): the team's router — turns the gauge
  digest plus scout findings into at most one brief per cycle. Threshold-guarded
  like the alerts demo: a model call happens only when two or more independent
  inputs agree (a channel's movement in the digest plus a scout finding, say).
  Emits briefs; never posts.
- **Gauge clerk** (Wave 1 sensory, stub in W1; live GA4 pull in Wave 2; script):
  pulls `generate_lead`, `purchase`, `begin_checkout`, `cta_click` and the funnel
  events by channel and campaign once per cycle. Emits the digest. A newer window
  replaces an older one (staleness lives here). Ids and counts only — the site's
  own no-PII rule is the neuron's rule.
- **Acquirer** (Wave 2; model): the office's growth strategist — turns each
  digest into at most one customer-acquisition experiment proposal (a new
  business type for the page factory to draft, a GBP or newsletter move, a
  partner push), grounded only in the gauges and channels the office already
  reads, each carrying the metric it claims, for the desk's approval before any
  brief routes. No outside data vendor and no outside senses: if an experiment
  needs a fact, the page factory's own sourced-facts discipline covers it.
- **Channel writer** (Wave 1; model): turns a brief into per-channel post drafts
  that follow the utm rules verbatim — links are built with the site's social
  tool, never hand-assembled. Drafts only; it has no keys.
- **Social operator** (Wave 3 for live-fire, Wave 1 recorded: effect neurons, one
  per channel): X, LinkedIn, Substack, GBP. Sealed credentials; publishes only
  behind a desk approval, one ask per post, carrying the campaign link. Substack's
  effect is a draft-stage send (the editorial handoff), because the send to
  subscribers is Kevin's call.

**Engineering team (graph peer `eng`)**

- **Engineering manager** (Wave 1; model): takes the managers' link (marketing
  manager ↦ eng manager) and turns each brief or accepted proposal into a task
  spec stated in the site's own terms (worktree, files, checks, the claimed
  metric); holds the backlog; refuses a task the house rules forbid (first-screen
  dial changes, prices in the bento/announce, renamed offer spell-outs).
- **Site engineer** (Wave 1 with a scripted builder; Wave 3 harness-wrapped):
  one task, one worktree (`wt new`), implement, run the called-for checks plus
  `verify`, open the PR, report the diff and receipt. Never merges: merge is the
  ship, and the ship is an effect.
- **Page factory** (Wave 2; script + model): executes the site's data-only
  "add a business type" recipe end to end in draft state — registry entry,
  keywords, tailoring, facts with sources, page copy, checks. Everything it
  writes is a draft for Kevin, per the site's own convention.
- **Attributor** (Wave 2; script + model): for each shipped change, before/after
  on the metric its task claimed, over a fixed window; plus the site checks'
  regressions. Writes the one-line learning note that rides back to the marketing
  manager ("channel up, brief source, metric moved"). Acceptances strengthen the
  brief-to-manager and manager-to-channel connections (ground, 0012); losses
  weaken them; the desk's answers are the only signal.

**Front desk (program peer, the effects host)**

- **Dispatcher** — the desk peer itself. Routes asks and holds, carries the
  durable open asks (0015: a held question survives a crash and re-arms), and
  holds no opinions. Also home of the **budget warden** function (0006): per-cycle
  action caps (posts per channel per week), and a standing rule that no effect
  fires between cycles except the desk's own answers.
- **Ledger** — the journal every ask/effect/answer and lead lands in; the weekly
  revenue note is its summary of desk answers, with the GA4 lead values always
  labelled a proxy, never revenue.

**Roster totals**: 12 roles, 11 neurons plus the desk; 4 model neurons, 3 script
sensory/effect sets, 2 harness-adjacent, 1 human. W1 runs on invented gauges and
recorded effects exactly as the demos do: headless, outcome asserted, same seed,
two takes identical.

## The revenue definition

For a consultancy the unit of revenue is a signed engagement, which only Kevin
knows. So: the fast gauges (enquiries by lead_type, funnel events, purchases of
the $49/mo plan — `purchase` events) drive every brief, and the desk's
won/engagement answers are the only entries the ledger may call revenue. A cycle's
score is therefore two numbers: proxy leads gained and engagements won. The
marketing manager optimizes the first; learning is validated on the second.

## Waves and gates

- **W1 — the office, invented data.** Roster skeleton in the demos' harness shape:
  gauge fixtures, recorded effects, no outside senses, two graph peers plus the
  desk peer, daily-cycle run shape. Gate: headless run asserts the outcome (a seeded
  day produces exactly one brief, its asks, and a recorded post per the utm
  rules); the engine's `check` accepts the network file; two runs on one seed
  are identical.
- **W2 — the senses go live.** Gauge clerk pulls GA4 (read-only) once per cycle;
  the acquirer proposes its first acquisition experiments from real gauges;
  attributor runs on the last shipped site change. Gate: a real digest lands at
  the desk; the trace shows the digest's numbers agreeing with the GA4 console
  one morning; one accepted experiment arrives at the desk with the metric it
  claims; no effect neuron holds a live key yet.
- **W3 — the hands go live.** Social operator publishes its first approved post
  per channel (GBP first); page-factory drafts arrive at the desk as site
  worktrees through the site's real flow (worktree → checks → verify → Kevin's
  `--approved` ship); strength learning switches on for acceptances only.
  Gate: every live post traceable ask → approval → campaign link → GA4 first-touch;
  a drafted type page ships only through the site's own gates; the ledger's first
  weekly revenue note exists.
- **Non-goals (until separately decided)**: no paid ads (organic only through
  W3); no cold outreach to listed businesses — inbound only, matching the site's
  own "an assistant drafts, a person replies" rule; the office touches only the
  portfolio site, never a client's; no engine or SPEC change (inputs are wrapper
  neurons, effects exist, humans exist, budgets exist, durable asks exist);
  no separate brain above the office — the cycle is the fixed shape, and the
  managers route.

## Constraint map

- **The site's gates are the office's ship gates**: `npm run verify` is the
  receipt; the announce discipline (`data/updates.ts` entries ≤ ~70 characters,
  plain words, no prices, newest-first) binds any merge the site engineer
  produces; the offer's name is a constant, never a literal; copy is draft-gated.
  The engineering manager's refusal list encodes these so bad tasks die at triage.
- **The utm rules are law for both teams**: `?from=` is internal-only, `utm_*` is
  Kevin's sharing only; the social operator's links come from
  `data/socialCampaigns.ts` via the social tool. A post whose link fails
  `check:social` is a malformed ask, not a fix-up-at-publish opportunity.
- **The engine seams are already built**: graph peer vs program peer (0013,
  the `companies` pattern), effects and the live stream (0007), human neurons on
  a page (0008), budgets (0006), durable deposit (0015, accepted P0+P1). Nothing
  here requires a SPEC change; if a webhook-input neuron needs one field, that
  starts its own decision record rather than joining this scope.
- **Secrets stay sealed**: channel credentials live inside their effect neurons'
  wrappers only; the GA4 read lives inside the gauge clerk only; no neuron sees
  another's keys. A cancel is a kill, which the operators must survive (stateless
  drafts, effects re-derived from the ledger).

## Ground truth (the gauges, as the site defines them)

| Gauge | Source | Value |
|---|---|---|
| Enquiry by type | GA4 `generate_lead` + the enquiry inbox | website 588, scoping call 150, concierge 500, general 150, partner 300 |
| Plan purchase | GA4 `purchase` | $49/mo websites, `transaction_id` once each |
| Channel movement | GA4 `cta_click`/`select_promotion` by campaign id | proxy for brief priority |
| Funnel depth | `concierge_pick`/`_continue`, `pain_continue`/`_skip`, `inquire_bar_open` | where briefs should push |
| Engagement won/lost | the principal's desk answer | the only "revenue" the ledger records |

## Pending decisions (recommendation first — answer by number)

1. **Where the office lives** — Rec: a new private repo `overfocus/office`
   following the demos' harness shape (it spawns the engine the same way), with
   W1's fixtures hand-made there. Alternative: ship W1 inside agent-neuron-demos
   as a demo and graduate later.
2. **First live effect** — Rec: the GBP Update post (contained, has precedent, and
   its effect is an edit on an owned surface). X and LinkedIn stay drafts-only
   until two live GBP posts have gone ask → approval → publish → traced cleanly.
3. **First live sense** — Rec: the GA4 daily pull through the Data API
   (read-only, the site's own property). Scout's monitors come second, inside a
   daily credit budget.
4. **Cadence** — Rec: a fixed daily cycle (digest → briefs → asks at the desk;
   Kevin answers when he answers; 0008 semantics). Monitor events feed the next
   cycle, they never wake a run.
5. **Learning shape** — Rec: strength learning on desk acceptances only, wired on
   the brief→manager and manager→channel links; no learned escalation routing,
   ever (escalation stays a rule, 0008).
6. **Paid spend** — Rec: out of scope through W3; revisit after the first weekly
   revenue note exists (that note is the instrument paid spend would ride).

## Non-obvious risks

- **A static org chart is a fixed pipeline under another name.** The adaptive
  machinery (corroboration before model calls, staleness on gauges, learned
  strength) earns its place on the marketing side, where signals are genuinely
  noisy; the engineering side stays deliberately mechanical.
- **Loop limits are mandatory**: brief → eng task → deploy → gauge movement →
  brief is a feedback loop; the budget warden's per-cycle caps and the
  one-brief-per-cycle rule are the loop limit, not a nicety.
- **A learned weight could mute the one channel that matters** (the alerts use
  case's own named risk): strength floors per channel — a channel below a floor
  keeps a guaranteed brief share — so a slow Substack can never be starved to zero
  by its own learning.
- **The office must not become its own content channel about itself.** Its briefs
  serve the practice's offers; the Labs and demos marketing the repos is Kevin's
  separate editorial voice until he says otherwise.

## Decisions

**Accepted (Kevin, 2026-11-10, go with the recs): every recommendation above.** Each
pending decision takes the answer its recommendation names:

1. The office is the private repo `overfocus/office` (local
   `~/Documents/projects/office`), built on the demos' harness shape, its W1
   fixtures hand-made there.
2. The first live effect is the GBP Update post; X and LinkedIn stay drafts-only
   until two live GBP posts have gone ask, approval, publish and traced cleanly.
3. The first live sense is the GA4 daily pull, read-only; the scout's monitors
   come second, inside a daily credit budget.
4. Cadence is a fixed daily cycle; a monitor event feeds the next cycle and never
   wakes a run.
5. Learning is strength on desk acceptances only; no learned escalation routing,
   ever.
6. Paid spend is out of scope through W3, revisited after the first weekly
   revenue note.

The roster, the revenue definition, the wave gates and the non-goals stand as
written; no deviation at acceptance.

**Amended (Kevin, 2026-11-10, mid-build steer): the office carries no Firecrawl
and no outside data vendor at all.** Decision 3's second half (the scout's
monitors, inside a daily credit budget) is dropped; the roster's scout becomes
the acquirer (see the roster above), and the network itself proposes the ways to
acquire customers for the website offer — experiments grounded in the gauges and
channels the practice already owns (GA4, the enquiry ledger, the Google Business
Profile, the newsletter), each approved at the desk with the metric it claims
before any brief routes. W1 in the office repo is unaffected: no outside sense
was ever built. The W2 gate's second live sense is the acquirer, not a monitor.
