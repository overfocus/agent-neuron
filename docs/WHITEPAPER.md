> **Draft.** This is a first draft for its author, Kevin Ferron, to rewrite. It was assembled from the
> repository's own files (at commit `4043967`) and from a positioning note whose sources were read on
> 2026-11-05. Places where the drafter was unsure are marked *(unsure)*. Nothing here is a release
> announcement.

# agent-neuron: firing rules on the wrapper, not in a controller

Kevin Ferron. Specification version 0.1 (draft). Repository: [github.com/overfocus/agent-neuron](https://github.com/overfocus/agent-neuron), MIT licence.

## 1. Abstract

When scripts, models and people are wired together, the hard part is not the wiring but deciding when
each one should act, when its work has gone stale, how much it may spend, and what to record when a side
effect's outcome is unknown. In the tools I looked at, that logic sits in a central workflow or in caller
code, where it is tied to that code's language and can only be tested through it. agent-neuron moves it onto each
component's wrapper: a network file declares, per neuron, how many distinct senders must agree, how long
an input may wait, what makes work stale, how far a chain may loop, what a call may cost and whether a
call has an effect, and an engine enforces those rules around any program that reads JSON on stdin and
writes JSON on stdout. The specification is a draft at version 0.1, with 193 conformance fixtures that
compare exact event lists on a virtual clock, a TypeScript reference engine, and a Python engine written
from the text that passes the fixtures and found where the text says less than they do. I present it as
a small, checkable protocol for the agent and developer community to test, break and implement again.

## 2. The problem

Wiring is easy: a shell script, a model call, an API lookup and a person answering a prompt can each take
input and produce output, and any queue or framework will connect them. The coordination around the
wiring is hard, and four questions come up every time.

**When to act, given many inputs.** A costly component (a model call, say) should often wait until
several cheaper signals agree: count inputs per piece of work, count each sender once, give up on an
input that waits too long for a partner.

**When work is stale, and how to stop it.** A newer input often makes older work pointless: the next
keystroke, the next reading, the next alert for the same service. Dropping work that has not started is
easy. Stopping work that is already running, and discarding its late answer, is the part that tends to be
left out, and it is where the waste is when the running work is the costly call.

**How much may be spent.** A chain that fans out or loops can spend without limit unless something outside
the components keeps the books, refuses a call before it starts, and does not let a crash or a cut give
the money back.

**What to record when an effect's outcome is unknown.** If a call that sends a message crashes, nobody
knows whether the message went. Recording that as an ordinary error tells every tool that retries on
error that doing it again is safe.

None of these is about the component itself; they are about its place in the graph. That is the thesis:
**the rule for when a component acts belongs on that component's wrapper, declared next to it and
enforced by an engine, not written into a central controller.**

## 3. Where that logic lives today

Several existing tools cover part of this ground well.

**Workflow orchestrators.** Open Workflow Specification (formerly Serverless Workflow) is a declarative
workflow language with a `listen` task that can wait for `all`, `any` or `one` of a set of events, with an
`until` condition, correlation and timeouts, and it ships a conformance kit and SDKs in several
languages [[1]](#ref-1): real portability and testability. But the rules live in a central workflow
document, and as far as I could find it does not count *distinct senders* with an expiry per input, or
cancel a running task because a newer keyed event arrived.

**Durable execution.** Temporal delivers activity cancellation through heartbeats, and an activity may
accept or ignore it [[2]](#ref-2); when to request a cancel is application code.

**Flow runtimes.** Node-RED's Join node, in manual mode, sends a combined message after a configured
count of messages or a timeout [[3]](#ref-3); its flows are stored as JSON for import and export
[[4]](#ref-4). The count is of messages, not of distinct senders, and I found no published format
specification or conformance suite. Apache NiFi expires queued items per connection and applies back
pressure by count or size [[5]](#ref-5), which is staleness by age, not by supersession of running work.

**Agent protocols.** MCP lets a caller send a cancellation notification, and the receiver may ignore it
[[6]](#ref-6). A2A defines a `CancelTask` operation and a `CANCELED` task state [[7]](#ref-7). Both are
caller-requested. A 2026 preprint comparing five agent interoperability protocols reports that voting is
absent from all of them [[8]](#ref-8) (preprint; I read the abstract only). Oracle's Open Agent Specification,
which aims to make agents portable across frameworks, leaves the order among parallel branches
unguaranteed [[9]](#ref-9).

So the pieces exist. What I did not find is the combination: a quorum of distinct senders, cancellation
of running work derived from a key on newer input, a fail-closed budget per trace and an honest unknown
outcome for side effects, all declared per node in a file, enforced by an engine and checked by comparing whole event lists.
My argument (not a survey) is that when this logic lives in a controller, it is tied to the controller's
language, and it can only be tested through that code's own tests: there is no file to read the rules
from and no shared test a second implementation must pass.

## 4. The protocol

The specification ([SPEC.md](SPEC.md), version 0.1, draft, RFC 2119 words) has four parts: a network
file format (SPEC 2), a neuron contract (SPEC 3), engine semantics (SPEC 4 to 6, 10, 11) and a trace
format (SPEC 7, [TRACE.md](TRACE.md)). The reasons for each rule are in [decisions/](decisions/).

### 4.1 The neuron contract

A neuron is a program. The engine writes JSON to its stdin and reads JSON from its stdout; the program
links no library (SPEC 0, 3). A process is the boundary that lets any language join without bindings and
makes a cancel a kill (decision 0001 section 5).

- **Level 0** (`"run": [...]`): one process per call. It prints one JSON value (an answer) or nothing
  (quiet); a non-zero exit, a kill, non-JSON or over 1 MiB of output is a failure with a code (SPEC 3.1,
  6.1). Above `threshold` 1 it receives `{"payload": P, "inputs": [P1, P2, ...]}` (SPEC 3.1).
- **Level 1** (`"serve": [...]`): one long-lived process speaking JSON lines, with a handshake,
  `maxInflight` slots, `cancel`, `ping` and restart with backoff (SPEC 3.2). From SPEC 3.2:

```
> {"v":1,"type":"hello","neuron":"judge"}
< {"v":1,"type":"ready","maxInflight":2}
> {"type":"signal","re":1,"payload":{"n":1}}
> {"type":"cancel","re":1}                       (re 1 went stale)
< {"type":"answer","re":1,"payload":{"n":9}}     (discarded: re 1 is cancelled)
```

- **The environment is empty** except `PATH` and the variable names listed in `env`; names only, never
  values (SPEC 2.4, section 0).
- **Programs never see** ids, the trace, their cost or whether they are an effect (SPEC 1, section 0), so
  a program cannot skip the rules.

### 4.2 The network file and routing

One closed JSON object, checked against `spec/network.schema.json`. Values are literals. Each neuron has
an `id`, a `description`, one kind (`source`, `run`, `serve`, `http`, `human`, `network`) and at most
eight options; links are one-way `{"from", "to"}` (SPEC 2.2 to 2.5, 2.8). An engine must offer a `check`
that reports errors with line and column, including a cycle where no neuron has `maxHops` (SPEC 2.7).

A producer injects a payload into a `source`, optionally with a `replaces` key; that starts a new trace
(SPEC 4.1). A reply goes back to the sender. After an answer that is not stale, the engine forwards it
along the neuron's outgoing links, inheriting the key (SPEC 4.3, 4.4). Forwarding along links is the only
way one neuron reaches another; nothing in the middle decides.

### 4.3 The firing rules

**Threshold and within.** Inputs are grouped by (neuron, trace). A sender counts once; its newer input
replaces its held one. When `threshold` distinct senders have an input held, the neuron acts once with
all of them. Each held input expires after its own `within` (default 10000 ms) (SPEC 5.2; fixtures
`threshold/same-sender-counts-once`, `threshold/every-held-input-has-its-own-timer`).

**Replaces.** The producer sets the key, because only it knows what makes an input stale (SPEC 4.1;
decision 0002). A newer signal with the same (sender, key) makes older work stale: waiting work is
dropped, running work is cancelled and its late answer discarded (SPEC 5.4; fixtures
`staleness/running-call-is-cut`, `staleness/level1-cut-late-reply-is-discarded`). Any replaced input cuts
a running threshold call, not only the one that crossed (SPEC 5.4; decision 0001 section 3; fixture
`staleness/17-earlier-input-cuts-a-running-call`). Staleness does not travel down a chain (fixture
`staleness/does-not-travel-down-a-chain`).

**maxHops and timeout.** An input above `maxHops` is refused, never run (SPEC 5.3; fixture
`hops/loop-ends-at-maxhops`). A call longer than `timeout` (default 30000 ms) fails; a reply due at the
timeout instant wins (SPEC 6.2 rule 7).

**Budgets that fail closed.** A network may declare a `budget` of named whole-number units and each neuron
a `cost`; under a budget every non-source neuron must declare one, or `check` refuses the file (SPEC
10.1; decision 0006 A). Each trace has its own pool, which a producer may lower but never raise (SPEC
10.2). A call reserves its cost when it starts, or is refused with an `error` of code `budget` if the pool
cannot cover every unit (SPEC 10.3). A started call commits its reservation on every outcome, a cut or a
crash included, so failing never runs for free (SPEC 10.4; decision 0006 decision 1; fixture
`budget/a-cut-call-commits-its-reservation`). In 0.1 every commit is the reservation, labelled
`"basis": "estimated"` (SPEC 10.4).

```json
"budget": { "credits": 12 },
{ "id": "hypothesis", "run": ["node", "neurons/hypothesis.mjs", "500"],
  "threshold": 2, "within": 500, "cost": { "credits": 5 } }
```

(Excerpt from `spec/examples/alerts-budget.network.json`.)

**Effects and the unknown outcome.** A neuron declared `"effect": true` does something in the world. A
started effect call is never cut by staleness: its answer is recorded, its cost committed, and only its
forward is suppressed, marked `"forwarded": false` (SPEC 11.2; fixture
`effect/the-answer-of-a-stale-effect-call-is-recorded-but-not-forwarded`). If it crashes, hangs or times
out, the engine reports `unknown` in place of `error` (SPEC 11.3; fixture
`effect/a-crashed-effect-call-is-unknown-and-commits-its-cost`); a budget refusal, a failed start and a
level 1 `error` reply stay errors (SPEC 6.1). The engine does not retry; SPEC 11.4 describes a
reconciliation pattern, which fixture `observer/an-observer-reconciles-an-unknown-effect` runs.

### 4.4 Sealing

A `network` neuron makes a whole network file one neuron (SPEC 2.6). Each call gets a new inner trace, so
threshold groups and staleness never cross calls (fixtures `composed/threshold-groups-never-mix-calls`,
`composed/staleness-is-scoped-to-a-call`). Inner ids, signals and errors never appear outside (fixture
`composed/inside-notes-stay-inside`). Its `cost` caps its inside (SPEC 10.5), and it may not contain an
effect (SPEC 11.5). Sealing is between neurons, not against whoever holds the file (SPEC 2.6; decision 0007
decision 17).

### 4.5 Event order and the trace

Events have one total order, so two engines produce the same list (SPEC 6.2): work due at one instant runs
in the order it was scheduled, a signal comes before the drops it causes, a reply before its forward. The
trace is `agent-neuron-trace` version 1, whose readers ignore what they do not know, so every 0.1 addition
is an optional field or a new event type (TRACE.md; decision 0003). Events are `signal`, `answer`,
`quiet`, `drop`, `error`, `unknown`, `finished` (once per trace) and `note` (SPEC 7.2, 6.3). The same events
are offered live as a stream that fails open (SPEC 9.1; fixture `stream/a-failing-consumer-changes-nothing`).

## 5. Conformance

### 5.1 The fixtures

An engine conforms when it passes every fixture in `spec/fixtures/` that is not pending (SPEC 8). There
are 193, none pending, in 13 areas: budget 26, failures 29, level1 20, composed 19, routing 18, staleness
18, threshold 17, effect 15, ordering 13, observer 6, runend 6, hops 4, stream 2 (counted from the
directory at commit `4043967`; `npm run engine -- conform spec/fixtures` reports 193 passed, 0 failed,
0 pending).

Each fixture is a network file plus a `fixture.json`: timed injections, a script that stands in for
every program (which reply, after how many virtual milliseconds) and the exact expected events
(CONFORMANCE.md 2, 5). No process starts and no clock is read; ids are normalised by first appearance and
events are compared one by one, every field, `t` included (CONFORMANCE.md 3, 4). Every run also checks
that the live stream equals the trace (CONFORMANCE.md 3, item 6). A fixture changes only in the same
commit as the text it pins (SPEC 8). This one pins that a newer input cuts a running call
(`spec/fixtures/staleness/running-call-is-cut/fixture.json`, shortened):

```json
{ "fixture": 0,
  "inputs": [ {"at":0,"source":"s","payload":1,"replaces":"k"},
              {"at":40,"source":"s","payload":2,"replaces":"k"} ],
  "script": { "a": {"calls":[{"after":100,"reply":{"answer":"old"}},
                             {"after":10,"reply":{"answer":"new"}}]} },
  "expect": [
    {"type":"signal","t":0,"id":"#1",...},
    {"type":"signal","t":40,"id":"#2",...},
    {"type":"drop","t":40,"id":"#1",...,"neuron":"a","reason":"replaced","phase":"running"},
    {"type":"answer","t":50,"id":"#3","from":"a",...,"replyTo":"#2"} ] }
```

### 5.2 Fixtures that bite

A test suite can pass and still miss bugs. `test/conformance.test.ts` builds 81 copies of the reference
engine, each with one deliberate bug, and requires every one of them to fail at least one fixture.
Examples from that list: threshold counting the same sender twice, `maxHops` off by one, a cut call
releasing its reservation instead of committing it, one pool shared by every trace, a stale input cutting
an effect call, a failed effect call recorded as an error, and the stream delivered out of order. (The
count is from the test file; decision 0004 still says 33, from when the suite was smaller.)

### 5.3 What "an agent-neuron engine" means

An engine is an agent-neuron engine for version 0.1 if it passes every active fixture through the
command-line contract `<engine> conform <dir>` (SPEC 8; CONFORMANCE.md 8), not by sharing code or by a
claim in a README. The fixtures test what an engine does with an outcome, not how it parses bytes; real
process behaviour is each engine's own tests' business (CONFORMANCE.md introduction, 9).

The reference engine is TypeScript, in `src/engine/`. An earlier Rust engine, written from the text in a
time-boxed experiment, was stopped as a no-go against criteria agreed in advance; its differential runs
still led to clearer text and new fixtures for timeout ties, signal names in error messages and the kill
grace (decision 0005).

### 5.4 A second engine

A second engine, in Python, has been written from SPEC.md, CONFORMANCE.md, the fixtures and the decision
records, without reading or porting `src/engine/` (branch `feat/python-engine`, commit `364086d`, not yet
merged; [conformance-python.md](conformance-python.md) and `py/agent_neuron/`). It uses the Python
standard library only (3.11 or later) and is 1,551 lines by `wc -l`, comments and blank lines included.

**Result:** 193 of 193 fixtures pass, identically on repeated runs. Its first complete run passed 177; the
16 failures were its own bugs and places where the fixtures pin more than the text says, and none was fixed
by loosening the runner (conformance-python.md, "Result"). After it passed, its author used the TypeScript
engine only as a black box on 10 extra probe fixtures (`py/probes/`), which both engines pass; the probes
found two Python bugs that no fixture covers.

**Scope:** it runs fixtures on a virtual clock with scripted programs. It starts no real process, has no
`check`, `run`, trace file or `--stream`, and refuses `http` and `human` neurons. So it tests the engine
semantics of SPEC 4 to 11, not the byte-level contract of SPEC 3 (conformance-python.md, "What this engine
does not do").

**What it found** is the point of a second engine: the places where the fixtures pin behaviour the text
does not state. Most important first (conformance-python.md, "Ambiguities and gaps"):

1. A level 1 restart that falls due while only a kill grace keeps the run open (SPEC 3.2, 6.2 rule 6;
   CONFORMANCE 9 lists the race as not covered, yet a fixture pins it).
2. A call queued behind a crashed level 1 process must keep the run open across the restart (6.2 rule 6);
   no fixture covers it, and the Python engine first got it wrong.
3. Rule 7 for a composed neuron: the text says an exit reply due at the instant of the composed neuron's
   `timeout` wins, but **both engines** let the timeout win (6.2 rule 7, CONFORMANCE 4.1). Either the
   engines or the sentence are wrong; no fixture decides it.
4. The `budget` error message: the fixtures pin `budget: needs 1 calls, 0 left`, more than the prefix
   SPEC 6.1 fixes (CONFORMANCE 3, item 4).
5. Where `finished` goes relative to the delivery effects of the signal that ended the trace (6.2 rules 2
   and 8).
6. Whether the failed-start count `f` in the restart delay includes the start that just failed (3.2).
7. When the hang timer of a cancelled level 1 slot starts (3.2).
8. A trailing space after `signal S:` that a prefix comparison turns into a rule (6.1).

None of these is fixed in the specification yet. Item 3 is the clearest case for a third engine, or a
decision.

## 6. Worked example: the alert storm

The example is invented end to end (USE-CASES.md case 2; DEMOS.md). Forty alerts for five services
arrive over 5.6 seconds. Three cheap detectors look at each: `logs` and `deploys` are shell scripts,
`metrics` is JavaScript. A neuron called `hypothesis` stands in for a model call with a 500 ms timer,
and should run only when two detectors flag the same alert. A composed neuron, `diagnosis`, finds a root
cause and a deploy to look at. **No model is called;** the stand-in is a timer (README; DEMOS.md).

### Before: the rule in a controller

Without engine rules, the caller keeps the books: a group per alert, a timer per held flag, the running
calls and the inputs each was built from, and cancel logic keyed by detector and service. The sketch below
is illustrative, written for this paper and never run; it is not code from the repository.

```js
// Illustrative only. Not from the repository, not run.
const held = new Map();     // alertId -> Map(detector -> { flag, timer })
const running = [];         // { inputs: [{ detector, service, alertId }], abort }
const latest = new Map();   // `${detector}|${service}` -> alertId of the newest flag

function onFlag(detector, flag) {
  const key = `${detector}|${flag.service}`;
  const older = latest.get(key);
  latest.set(key, flag.alertId);
  if (older !== undefined && older !== flag.alertId) {
    // stale: cut any running call built from the older flag, drop the older held flag
    for (const c of running) if (c.inputs.some(i => i.detector === detector && i.alertId === older)) c.abort.abort();
    const g = held.get(older); if (g?.has(detector)) { clearTimeout(g.get(detector).timer); g.delete(detector); }
  }
  const group = held.get(flag.alertId) ?? new Map();
  if (group.has(detector)) clearTimeout(group.get(detector).timer);       // a sender counts once
  const timer = setTimeout(() => group.delete(detector), 500);           // within
  group.set(detector, { flag, timer });
  held.set(flag.alertId, group);
  if (group.size < 2) return;                                            // threshold
  for (const e of group.values()) clearTimeout(e.timer);
  held.delete(flag.alertId);
  startHypothesis([...group.values()].map(e => e.flag));                 // must also ignore late answers once aborted
}
```

The repository's own library version of the demo still forwards answers in a listener and keeps its
counts in maps; decision 0002 lists each piece of that caller code and where it went in the network file.

### After: the rule on the wrapper

In the network file the same behaviour is two options on one neuron (`spec/examples/alerts.network.json`):

```json
{ "id": "hypothesis", "run": ["node", "neurons/hypothesis.mjs", "500"],
  "threshold": 2, "within": 500 }
```

and a key on each input, set by the producer, here the alert's service (`spec/examples/alerts.inputs.jsonl`):

```json
{"at":150,"source":"alerts","payload":{"id":2,"service":"gateway","severity":"crit",...},"replaces":"gateway"}
```

The detectors' flags inherit the key when they are forwarded (SPEC 4.4), so a newer flag from the same
detector for the same service makes the older one stale, and a running `hypothesis` call built from it is
cut (SPEC 5.4). The `hypothesis` program itself is a few lines and knows none of this
(`spec/examples/neurons/hypothesis.mjs`).

### The numbers

From the committed recording `traces/alerts.json` (its `summary`, threshold 2): 40 alerts produced 52
detector flags; `hypothesis` started 17 calls instead of 40; 10 of them were cut as stale and 7
finished; 11 waiting flags were replaced by newer ones and 7 expired waiting for a partner; there were 7
diagnoses. The same storm recorded with other thresholds (`traces/alerts-threshold-1.json`,
`alerts-threshold-3.json`) gives 52 calls at threshold 1 (one per flag; 31 cut, 21 finished, 16
diagnoses) and 0 calls at threshold 3 (21 flags expired, 31 replaced while waiting).

Three cautions, which I would rather state than have a reader find:

- **The alerts are invented, and the waste is built in.** Alerts for one service come either 150 ms apart
  or 900 ms or more apart, against a 500 ms stand-in call, so bursts are designed to make calls go stale
  (`examples/alerts-demo.mjs` comments; DEMOS.md).
- **The verdicts match by construction.** The invented topology maps every service to `gateway` as its
  root (`examples/alerts/topology.json`), so all 7 diagnoses in the recording name `gateway` and deploy
  `dep-8b21`. The example shows how many costly calls run and how many are cut, not diagnostic quality.
- **The numbers depend on timing.** The committed traces were recorded with the TypeScript library demo
  (`npm run record -- alerts`, TRACE.md), and TRACE.md says the numbers are what that run produced, not
  constants. Two runs of the engine version (`npm run demo:network`) on one machine on 2026-11-05 gave
  52 flags, 17 calls, 10 cut, then 50 flags, 16 calls, 9 cut; the README says counts vary with load.

## 7. Where it sits

agent-neuron is not another way for agents to talk to each other. That layer is taken. MCP connects an
application to tools and data over JSON-RPC [[10]](#ref-10). A2A lets opaque agents exchange tasks and messages,
described by an Agent Card [[7]](#ref-7). ANP gives agents decentralised identity and discovery on the open
internet [[11]](#ref-11). MCP and A2A now sit under the Linux Foundation [[12]](#ref-12) [[13]](#ref-13), and both have
conformance suites [[14]](#ref-14) [[15]](#ref-15).

What I specify sits one level in: inside a single engine, the rules for **when** a wrapped program runs.

Pieces of this exist elsewhere:

| Project | What it already covers | What I did not find in it |
|---|---|---|
| Open Workflow Specification | `listen` for all or any of a set of events, correlation, timeouts, a conformance kit [[1]](#ref-1) | a quorum of distinct senders with expiry per input; per-node rules (it is a central orchestrator) |
| Node-RED | Join after a message count or a timeout [[3]](#ref-3); flows stored as JSON [[4]](#ref-4) | distinct senders; a published format spec or conformance suite |
| CloudEvents | an `expirytime` extension; consumers may discard expired events [[16]](#ref-16) | cancellation of running work |
| Apache NiFi | per-connection expiry of queued items, back pressure [[5]](#ref-5) | supersession of running work by a keyed newer input |
| MCP, A2A, Temporal | cancellation requested by a caller; in MCP the receiver may ignore it [[6]](#ref-6) [[7]](#ref-7) [[2]](#ref-2) | cancellation derived from a key on newer input |
| Cosmonapse | an agent runtime and envelope spec described as "modeled on a nervous system" [[17]](#ref-17), with cost rolled up in envelopes, deadlines and a `STOP` that cancels a whole trace | declared per-node firing rules; reserve-and-commit budget rules; a conformance suite (none found) |
| OpenTelemetry | spans with links, events and status; GenAI agent conventions, marked as in development [[18]](#ref-18) [[19]](#ref-19) | it records runs; it does not decide them |

MCP Tasks have the nearest cousin of `unknown` I found: a cancelled task stays cancelled even if
execution later completes or fails [[20]](#ref-20). That keeps the record stable; it does not say the
effect is unknown. Alertmanager groups alerts over time windows and inhibits one alert while another
fires [[21]](#ref-21), which overlaps the alert storm's goal but has no rule that several sources must
agree.

I have not found a specification that combines these rules and checks them by comparing exact event
lists. I may have missed one, and I would like to hear about it.

The gap is also narrower than it looks: agent-neuron is a single-engine, single-host specification with
no identity, authentication, discovery or transport between machines (SPEC 3.3, Appendix B). It does not
compete with MCP, A2A or ANP at their layer.

### 7.1 Composition with MCP and A2A (proposal)

Everything in this subsection is a proposal. The specification says nothing about MCP, A2A or
OpenTelemetry today, and none of it is built.

- **A neuron that wraps an MCP tool.** A level 1 adapter that is an MCP client: a `signal` becomes a
  `tools/call`, the result the `answer`, an `isError` result an `error`, and a `cancel` MCP's cancellation
  notification (or the task cancel for a task-augmented call) [[6]](#ref-6) [[20]](#ref-20). Because an
  MCP receiver may ignore a cancel [[6]](#ref-6), the network's author should declare a tool that changes
  the world `effect: true`. Budgets need no MCP change: the engine meters the declared `cost`.
- **A neuron that wraps an A2A agent.** Also a level 1 adapter: send a message, map a completed task to
  `answer` and a failed or rejected one to `error`, and call `CancelTask` on `cancel` [[7]](#ref-7). A2A's
  input-required state has no mapping, because a neuron cannot ask its caller a question in the middle
  of a call; that is an open question.
- **A network exposed as an A2A agent.** An engine could publish an Agent Card for a network file's
  `entry` and `exit`. SPEC 2.6 already says that a boundary that must hold against another party is an
  `http` neuron to that party's own engine, which fits A2A's opaque agents.

### 7.2 OpenTelemetry as a non-normative export (proposal)

I would keep `agent-neuron-trace` version 1 as the normative record, because conformance compares exact
event lists with virtual times and normalised ids, and the OpenTelemetry GenAI agent conventions are
still marked as in development [[19]](#ref-19). A non-normative export could map one agent-neuron trace to
one OpenTelemetry trace and one call to one span; a threshold call's span would carry span links to each
input it consumed, since links exist to tie a span to one or more causes [[18]](#ref-18); drops,
expiries and hops refusals would be span events, and `reserve` and `commit` span attributes. `unknown`
has no OpenTelemetry equivalent; I would export it as status Error plus an attribute such as
`agent_neuron.outcome = unknown` *(unsure: this is an open design question)*.

## 8. Non-goals and what is not built

- **Link strength and learning.** The name suggests connections that strengthen. They are planned and
  not specified: the library's `weight()` always returns 1 (`src/network.ts`), the network file has no
  strength field (SPEC 2.5, Appendix B), and nothing is kept between runs (README).
- **`http` and `human` neurons** are specified (SPEC 3.3, 3.4) but not run: the reference engine's
  `check` warns and `run` refuses (SPEC Appendix C). The `http` failure row has no fixture (decision 0004).
- **No model is called in any demo.** Every "model" is a timed stand-in (README). A neuron can hold a
  model, with the API key named in `env` (SPEC section 0); that is a property of the design, not something
  the demos show.
- **POSIX only.** The engine runs on macOS and Linux (README "Development"; CONTRIBUTING.md). Windows is
  out of version 0.1 (SPEC Appendix B).
- **One engine on `main`**, in TypeScript. The Python engine (section 5.4) runs fixtures only and is not
  merged; the Rust attempt was stopped (decision 0005). The npm package is private and unpublished
  (`package.json`).
- **Not in version 0.1:** usage reported by a neuron (so a commit could be below the reservation), an
  owed ledger, spend per source or per time window, retries, an engine-held `unknown` that blocks a key
  until reconciled, effects inside a composed file, passive and active channels, authentication for
  `http` (SPEC 10.8, Appendix B; decisions 0006 and 0007). A trace explains what was spent; it does not
  prove it to anyone else (SPEC 10.8).
- **Not a wire protocol between parties** (section 7).

## 9. Status, versioning and how to take part

**Status.** Specification version 0.1, draft; it "may change in any way until it is called 1" (SPEC,
status line). A change is one commit to SPEC.md, the schema and the fixtures together (CONTRIBUTING.md).

**Versions.** The network file's `version` is `0`, since every 0.1 field is an addition (decision 0006,
builder's choice 5). The level 1 protocol's `v` is `1`, bumped only by a change an old peer would misread
(SPEC 3.2). The trace format is `agent-neuron-trace` version 1 (TRACE.md).

**How to take part.**

- **Implement an engine** in any language against the fixtures; `<engine> conform <dir>` is the contract
  (CONFORMANCE.md 8). Decision 0005 and conformance-python.md record what two attempts found.
- **Open an issue on any ambiguity in the specification**, at
  [github.com/overfocus/agent-neuron/issues](https://github.com/overfocus/agent-neuron/issues). Two earlier
  issues changed the spec: #16 (`phase` on drops) and #17 (any replaced input cuts a running threshold
  call) (decisions 0001 and 0003). A fix to the text comes with a fixture, and a rule an engine could break
  quietly comes with a mutant (CONTRIBUTING.md).
- **Write a neuron.** SPEC section 0 is the quick start; `spec/examples/neurons/hypothesis_serve.py` is a
  level 1 neuron in standard-library Python.
- **Propose a field** only with a decision record that says what it costs against the field budget
  (SPEC 2.8; CONTRIBUTING.md).

**Licence.** MIT (`LICENSE`).

## 10. My view

(Kevin to write)

## References

Sources outside this repository, numbered in order of first use. All were read on 2026-11-05, as recorded
in the positioning note this draft was built from. Quotations are under 15 words, one per source at most.

1. <a id="ref-1"></a>Open Workflow Specification (formerly Serverless Workflow): README and dsl-reference.md. CNCF Sandbox project. Kind: spec, repo. <https://github.com/open-workflow-specification/specification>. Read 2026-11-05.
2. <a id="ref-2"></a>Activity Execution. Temporal Technologies. Kind: vendor docs. <https://docs.temporal.io/activity-execution>. Read 2026-11-05.
3. <a id="ref-3"></a>Node-RED Join node help (17-split.html). OpenJS Foundation (Node-RED project). Kind: docs (in repo). <https://github.com/node-red/node-red/blob/master/packages/node_modules/%40node-red/nodes/locales/en-US/sequence/17-split.html>. Read 2026-11-05.
4. <a id="ref-4"></a>About Node-RED. OpenJS Foundation (Node-RED project). Kind: docs. <https://nodered.org/about/>. Read 2026-11-05.
5. <a id="ref-5"></a>Apache NiFi User Guide. Apache Software Foundation. Kind: docs (first 100k characters read). <https://nifi.apache.org/docs/nifi-docs/html/user-guide.html>. Read 2026-11-05.
6. <a id="ref-6"></a>Model Context Protocol, Cancellation (revision 2025-11-25). Model Context Protocol project. Kind: spec. <https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/cancellation>. Read 2026-11-05.
7. <a id="ref-7"></a>Agent2Agent (A2A) Protocol Specification v1.0.0. A2A project (Linux Foundation). Kind: spec (first 100k characters read). <https://a2a-protocol.org/latest/specification/>. Read 2026-11-05.
8. <a id="ref-8"></a>Kang and Diponegoro, arXiv 2606.31498. arXiv. Kind: preprint (abstract only read). <https://arxiv.org/abs/2606.31498>. Read 2026-11-05.
9. <a id="ref-9"></a>Open Agent Specification, language specification 26.3.1 (parallelism section). Oracle. Kind: spec, repo. <https://github.com/oracle/agent-spec>. Read 2026-11-05.
10. <a id="ref-10"></a>Model Context Protocol, Specification (latest revision, 2026-07-28). Model Context Protocol project (Agentic AI Foundation, Linux Foundation). Kind: spec. <https://modelcontextprotocol.io/specification/latest>. Read 2026-11-05.
11. <a id="ref-11"></a>Agent Network Protocol (ANP 1.2). ANP open-source technical community. Kind: docs. <https://agent-network-protocol.com>. Read 2026-11-05.
12. <a id="ref-12"></a>Linux Foundation Launches the Agent2Agent Protocol Project (2025-06-23). Linux Foundation. Kind: press release. <https://www.linuxfoundation.org/press/linux-foundation-launches-the-agent2agent-protocol-project-to-enable-secure-intelligent-communication-between-ai-agents>. Read 2026-11-05.
13. <a id="ref-13"></a>Linux Foundation Announces the Formation of the Agentic AI Foundation (2025-12-09). Linux Foundation. Kind: press release. <https://www.linuxfoundation.org/press/linux-foundation-announces-the-formation-of-the-agentic-ai-foundation>. Read 2026-11-05.
14. <a id="ref-14"></a>modelcontextprotocol/conformance. Model Context Protocol project. Kind: repo. <https://github.com/modelcontextprotocol/conformance>. Read 2026-11-05.
15. <a id="ref-15"></a>a2aproject/a2a-tck. A2A project. Kind: repo. <https://github.com/a2aproject/a2a-tck>. Read 2026-11-05.
16. <a id="ref-16"></a>CloudEvents expirytime extension. CloudEvents (CNCF). Kind: spec. <https://github.com/cloudevents/spec/blob/main/cloudevents/extensions/expirytime.md>. Read 2026-11-05.
17. <a id="ref-17"></a>Cosmonapse/cosmonapse-core: README and design/ENVELOPE_SPEC.md. Cosmonapse. Kind: repo. <https://github.com/Cosmonapse/cosmonapse-core>. Read 2026-11-05.
18. <a id="ref-18"></a>OpenTelemetry, Traces (concepts). OpenTelemetry (CNCF). Kind: docs. <https://opentelemetry.io/docs/concepts/signals/traces/>. Read 2026-11-05.
19. <a id="ref-19"></a>OpenTelemetry semantic conventions for generative AI (agent spans, MCP, A2A). OpenTelemetry (CNCF). Kind: spec, repo (Development status). <https://github.com/open-telemetry/semantic-conventions-genai>. Read 2026-11-05.
20. <a id="ref-20"></a>Model Context Protocol, Tasks (revision 2025-11-25, experimental there). Model Context Protocol project. Kind: spec. <https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/tasks>. Read 2026-11-05.
21. <a id="ref-21"></a>Alertmanager configuration. Prometheus (CNCF). Kind: docs. <https://prometheus.io/docs/alerting/latest/configuration/>. Read 2026-11-05.

Files in this repository cited above (commit `4043967`): [SPEC.md](SPEC.md) (version 0.1),
[CONFORMANCE.md](CONFORMANCE.md), [TRACE.md](TRACE.md), [DEMOS.md](DEMOS.md), [USE-CASES.md](USE-CASES.md),
[decisions/](decisions/) 0001 to 0007, `spec/network.schema.json`, `spec/examples/`, `spec/fixtures/`,
`test/conformance.test.ts`, `traces/alerts.json`, `traces/alerts-threshold-1.json`,
`traces/alerts-threshold-3.json`, `examples/alerts-demo.mjs`, `examples/alerts/topology.json`,
[README.md](../README.md), [CONTRIBUTING.md](../CONTRIBUTING.md).
