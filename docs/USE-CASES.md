# Use cases

Where an agent-neuron swarm could be useful, imagined from four angles:
AI agents working together, operations, the physical world, and real-time
interfaces. Every name and every piece of data here is invented.

They are ranked by how well they fit the concept, not by how big the market
is. Each one says honestly where a central router, a pipeline or an existing
tool would do as well.

## What the best fits have in common

- **Unrelated systems** that no single controller understands, wrapped
  behind one interface.
- **A stream where newer input makes older work stale**, so dropping stale
  work matters.
- **A person whose approvals are a natural learning signal** for which
  connections should strengthen.
- **Something worth sealing** inside a composed neuron: credentials, a
  supplier's internals, private deliberation.

The weakest fits are fixed pipelines under another name, or problems whose
real difficulty is something the concept does not address, such as
unreliable connections.

Most of the strong fits need connection strength, threshold voting (a neuron
acts only when enough inputs agree) or loop limits to show the adaptive part
of the idea. Without them they can show wrapping, broadcasting and dropping
stale work, but not learning.

## Strong fit

### 1. Live writing assistant

Helpers react to a writer's text as it changes: a style-rules script, a
house term list, a composed citation neuron (a reference index, a format
checker and a small model), and a model that rewrites only after a pause and
a full sentence. The editor broadcasts every edit; each helper's wrapper
decides whether to act; a newer edit replaces the older one, so slow rewrites
of old text are dropped. The citation neuron's lookups stay sealed: outside,
one suggestion comes out. Suggestions the writer accepts strengthen the
connection to that helper.

*Why a swarm:* a continuous stream, helpers that gate themselves, and work
that goes stale. A router would rebuild all of that. Built from what exists
today. The document itself belongs in an editor's own data structure, not in
the swarm.

### 2. Alert storm triage

Forty alerts arrive in ninety seconds. Detector neurons (metrics APIs, a log
search script, a deploy-history script) look first; a model forms a
hypothesis only when two or more detectors flag the same trace, which caps
model calls. A newer reading replaces stale analysis. A composed diagnosis
neuron keeps its read-only credentials and its reasoning inside. A detector
whose flags came before confirmed incidents counts for more.

*Why a swarm:* sources that arrive out of order suit broadcasting with each
neuron deciding for itself. *Risk:* a learned weight could mute the one
source that matters in a new kind of failure.

*Built, except the learned weight:* `npm run demo:alerts` runs this with
invented data (see [DEMOS.md](DEMOS.md)).

### 3. Building fault triage

An alarm in an office building is diagnosed by heating, power and lift
scripts, a model that reads vibration logs, and a technician. The alarm is
broadcast; a follow-up question goes to one system only; the trace links the
whole diagnosis. A supplier's chiller plant is a composed neuron whose
internals stay sealed. The technician's confirmation of the right answer is
the strength signal.

*Why a swarm:* these systems are different kinds of software that no single
controller understands. This is "wrap anything" at its strongest.

### 4. Game villagers who gossip

In a browser game, each villager is a neuron (a small model or a rules
script), and rumours spread along friendships as the player acts. The
player's actions are broadcast to villagers within earshot; gossip goes to
friends; walking away drops a half-written reply. The player talks to a
composed guild, never to its members, so the guild's politics stay sealed.
Two villagers whose rumours keep travelling between them become friends: a
strengthened connection.

*Why a swarm:* the graph is the game. *Limits:* the game loop still owns the
world state, and model latency rules out anything per frame.

### 5. Canary deploy guard

A new release goes to a small share of traffic, and a swarm recommends
promoting it or rolling it back. Metric neurons replace each reading window
with the next, so verdicts are never made on stale numbers. A person must
approve a promotion. A composed deploy neuron is the only one holding deploy
credentials and offers just two actions.

*Why a swarm:* sources can be swapped or added without changing a central
engine, and the sealed deploy neuron limits the damage. *Risk:* an automated
rollback that loops; this needs loop limits.

## Moderate fit

### 6. Checks on every push

Each push to a branch is checked by scripts, a composed test suite that
hides its retries, a gated model review and a person. A newer push cancels
the tests and reviews of older ones. *Catch:* existing CI tools already do
most of this.

### 7. Greenhouse climate

Sensors broadcast readings; vents, heaters and misters decide for themselves
whether to act ("act when two sensors agree"); each bay is a composed neuron.
*Catch:* an industrial controller already does this better, and safety
cut-offs must stay in firmware.

### 8. On-call escalation

People are neurons. A page goes to the primary; with no answer it is
broadcast to others; if the alert clears, pending pages are dropped so nobody
is woken for nothing. *Catch:* paging tools already do this, and escalation
must follow rules, never learned routing that could stop paging someone.

### 9. Whiteboard helpers

Help appears where people linger on a shared board: a diagram tidier, a
label suggester, a way to ask a teammate. *Catch:* only the help layer fits;
live cursor traffic belongs on ordinary pub/sub.

### 10. A reusable review panel

Two departments send drafts to their own copy of one composed review panel,
and each copy learns on its own. *Catch:* it shows composition and separate
learning, but inside it is a fan-out under another name.

## Weak fit

### 11. Warehouse robots

Robots report blocked aisles and hand off tasks; each robot is a composed
neuron. *Why not:* it needs neurons running across machines, which the
concept does not cover yet, and fleet managers already do this centrally.

### 12. Cost spike watchdog

A spending jump is explained and a fix proposed for a person to approve.
*Why not:* a scheduled job does it, and spending data changes too slowly for
dropping stale work to matter.

### 13. Live captions in several languages

Partial transcripts are broadcast to translators and replaced by final ones.
*Why not:* a fixed pipeline does this with less delay and cost.

### 14. Remote flood sensors

River gauges with patchy radio links warn a ranger. *Why not:* the real
problem is unreliable links (store-and-forward, duplicates), which the
concept does not address.
