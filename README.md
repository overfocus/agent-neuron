# agent-neuron

<img src="assets/brand/icon-256.png" width="92" align="right" alt="agent-neuron's mark: a neon-green neuron, a signal riding its axon toward the terminal bouton, on a black rounded square">

Any program (a shell script, a Python file, a model call) goes inside the same wrapper, a JSON stdin/stdout contract, so they can be wired together in any shape. Instead of a central controller deciding what runs, each wrapper carries its own rules: how many inputs must agree before it acts, when work has gone stale and gets cut, how much it may spend.

This is an experiment at concept stage: a specification (version 0.1, draft), its conformance fixtures, one engine that runs it (in Rust, `rust-engine/`) and a set of demos. Until [decision 0023](docs/decisions/0023-one-engine.md) there were three engines (a TypeScript reference with the library it grew from, a protocol test in Python, and the Rust one); the Rust engine is now the only one. Nothing is on npm yet.

The idea as a protocol, for agent and framework developers: [docs/WHITEPAPER.md](docs/WHITEPAPER.md) (a draft).

## A team decides how its agents run

The strongest pattern in coding agents right now is one developer with a harness. The failure mode when a
team shares that harness is well known: plausible agent contributions that no rule was in a position to
stop (slop grenades), and architectural choices made silently inside a diff (invisible lock-ins). Both
come from the rules travelling as private prompt context. agent-neuron makes the coordination layer a
network whose rules are data the whole team can see, argue with and version in git:

- the work lands as **desk items** — every ask rides whole, on a page several people share, each person's
  items filtered to their role;
- **`threshold` decides quorum** (two reviewers agree on the direction), **a human gate decides the
  irreversible step** (a merge as an `effect`: never cut, `unknown` on crash), and **budgets bound what
  any agent may spend**;
- every vote, accept and refusal lands as one auditable line, with the trace beside it showing which rule
  fired — the decision record no harness keeps;
- **agents join through MCP** (`neuron-engine mcp`: `check_network`, `run_network`, runs as jobs,
  live resources), **people answer on the page**. Specs and guidelines ride as data the network names,
  so the team's policy is a diffable file in git — and a spec change is itself a decided contribution
  on the same surface.

It runs today as a demo, with no model anywhere:

```sh
npm run demo:team
```

Two agent contributions sit on real branches of a scratch git repo. Two reviewers vote in their browser
windows; the quorum settles the direction; a lead gates the merge; the merge is a real git commit with a
tag, and the ledger of who decided what is on every page. Two scripted paths (`--headless`,
`--headless --tie`) show the whole flow — quorum and tiebreak — without browsers. The full positioning,
and the four protocol gaps this surface is driving (grants at the desk, durable holds, presence,
quiescence): [docs/MULTIPLAYER.md](docs/MULTIPLAYER.md).

## What's built, what's planned

| Built | Planned |
|---|---|
| A shared decision surface: desk items for several people, quorum by `threshold`, a lead's gate on an `effect`, the merge a real git commit (`npm run demo:team`) | Grants validated at the desk's POST, a `human` hold that survives a restart, presence, run quiescence ([issue #30](https://github.com/overfocus/agent-neuron/issues/30)) |
| Neurons that wrap scripts and executables (`run`, `serve`), HTTP APIs (`http`) and a person (`human`, answered on a local page: `npm run demo:human`) | A network that rewires itself; durable trails between runs ([issue #30](https://github.com/overfocus/agent-neuron/issues/30) item 1) |
| Many-to-many links, signals to every link or to named neurons, each signal tracing its chain; trail strength as engine state: `ground` — deposits kept per place, evaporation, a seeded one-link routing for replies that name no `to` ([decision 0012](docs/decisions/0012-the-ground.md)) | Ignore conditions, passive and active channels |
| Per-neuron `threshold` (N distinct senders must agree), `within`, `replaces` (a newer input cuts stale work) and `maxHops` (loops stop) | Limits other than `maxHops` |
| Network files: a JSON format, a closed schema, a validator and an engine that runs them (`neuron-engine check`, `neuron-engine run`) | A `human` wait that survives a restart |
| Per-trace budgets, effects that are never cut, a live event stream | A neuron reporting its own spend |
| The engine, in Rust (`rust-engine/`): real processes, `http` neurons, `human` neurons on the desk, peers on other machines, the MCP server `neuron-engine mcp` | `https` (needs a dependency policy) and packaging of the engine binary for npm |
| Composed neurons: a whole network behind one wrapper, sealed from outside | Anything that learns: today each call is its own process and nothing is kept |
| 303 conformance fixtures any other engine must pass (decisions 0012, 0016's `may`, 0019's neuron file, 0021's `maxInflight` and 0022's `args` and `each` included); recorded traces | A real model: no model is called anywhere, every "model" in the demos is a timed stand-in |

## Try it

You need a Rust toolchain (`cargo`; the engine uses the standard library only, so the build fetches nothing) and Node 22 or later (the demo's neurons are small Node and shell programs; there is nothing to `npm install`). It makes no network requests. Run it on macOS or Linux.

```sh
git clone https://github.com/overfocus/agent-neuron && cd agent-neuron
npm run demo:network
```

It builds the engine first (`cargo build --release --manifest-path rust-engine/Cargo.toml`; the binary is `rust-engine/target/release/neuron-engine`). Output (a run takes about 7 seconds):

```
Alert storm from alerts.network.json: 40 alerts over 5.6 s, run by the engine.
40 alerts -> 36 flags -> 10 hypothesis calls instead of 40
3 calls cut short as stale, 9 waiting flags replaced, 7 gave up waiting
7 diagnoses; the last: {"alert":35,"service":"checkout","detectors":["deploys","logs"],"root":"gateway","deploy":"dep-8b21","minutes":14}
324 events in the trace; no inner id of diagnosis in it: true
```

A live run starts real processes on the real clock, so the counts move from run to run and with what else the machine is doing (a busy machine cuts more detectors short, so fewer flags and fewer calls); the seven diagnoses do not move. The fixed numbers quoted elsewhere (52 flags, 17 calls) come from a recorded trace, `traces/alerts.json`, made on an idle machine with the TypeScript library's version of this demo before decision 0023.

That is a made-up storm of 40 alerts for five services. Cheap detectors flag alerts, a slow neuron stands in for a model call (a 500 ms timer) and runs only when two detectors agree, and a composed neuron answers with a diagnosis. The summary says how many slow calls ran instead of 40.

### The network file

The whole storm is this file, `spec/examples/alerts.network.json`. The programs it names are in `spec/examples/neurons/` and `examples/alerts/`.

```json
{
  "$schema": "../network.schema.json",
  "version": 0,
  "name": "alerts",
  "description": "Alert storm triage: cheap detectors flag alerts, a slow neuron runs only when two detectors agree, a composed neuron finds the root cause.",
  "neurons": [
    { "id": "alerts", "description": "Where alerts enter. The producer tags each with a replaces key (its service).", "source": true },
    { "id": "logs", "description": "Flags an alert when the log has error lines for its service.", "run": ["sh", "../../examples/alerts/detect-logs.sh", "../../examples/alerts/logs.txt"] },
    { "id": "deploys", "description": "Flags an alert when its service was deployed in the last 30 minutes.", "run": ["sh", "../../examples/alerts/detect-deploys.sh", "../../examples/alerts/deploys.txt"] },
    { "id": "metrics", "description": "Flags a critical alert when the service's error rate is above 5%.", "run": ["node", "neurons/metrics.mjs"] },
    { "id": "hypothesis", "description": "A 500 ms stand-in for a model call; acts once two distinct detectors flag one alert.", "run": ["node", "neurons/hypothesis.mjs", "500"], "threshold": 2, "within": 500 },
    { "id": "diagnosis", "description": "Composed: finds the root cause and the deploy to look at.", "network": "diagnosis.network.json" }
  ],
  "links": [
    { "from": "alerts", "to": "logs" },
    { "from": "alerts", "to": "deploys" },
    { "from": "alerts", "to": "metrics" },
    { "from": "logs", "to": "hypothesis" },
    { "from": "deploys", "to": "hypothesis" },
    { "from": "metrics", "to": "hypothesis" },
    { "from": "hypothesis", "to": "diagnosis" }
  ]
}
```

`hypothesis` has `threshold: 2` and `within: 500`: it acts only when two distinct senders (here `logs`, `deploys` and `metrics` are the candidates) have each flagged the same alert, and a flag that waits longer than 500 ms for a partner is dropped. `replaces` is not in the file: the producer puts it on each input (here, the alert's service), and a newer input with the same key cuts older work for that key that is still waiting or running, which is where the "cut short as stale" and "replaced" counts come from. Edit the file, run `rust-engine/target/release/neuron-engine check spec/examples/alerts.network.json` to validate it, and run it again.

## How it compares

- **Unix pipes.** A level-0 neuron is a filter: a program that reads JSON on stdin and writes JSON on stdout. The difference is that neurons form a graph (many in, many out, loops allowed and bounded), not a line.
- **Node-RED and similar flow files.** The shape is similar: a JSON file lists nodes and links. The difference I am trying for is that each node carries its own firing rules (threshold, staleness, budget, effect), and that those rules have a written specification and conformance fixtures, so another engine can be checked against the first. That happened three times: a TypeScript reference, a Python protocol test written from the text alone, and the Rust engine all passed the same fixtures, until [decision 0023](docs/decisions/0023-one-engine.md) kept only the Rust engine. It passes all 277.
- **Agent frameworks.** They usually put a controller or planner in the middle. Here nothing in the middle decides; and no model is required, since a neuron can be any program. And the top-level deciding can belong to the team: quorum, gates and budgets are neuron fields, so three people agreeing on a merge is the same graph that triages forty alerts by itself (`npm run demo:team`).

I have not benchmarked any of these against it, and this one is far smaller and younger than all of them.

## The idea

- **Wrap anything.** A neuron can hold an LLM-powered agent, plain code with
  no AI in it, an external program, or a human answering through the same
  interface. The wrapper is an adapter: it translates between the neuron
  network and whatever the thing inside expects.
- **No model required.** The network runs on a deterministic engine, so
  neurons can execute without a model. AI is something a neuron *can* hold,
  not something the network depends on.
- **Many-to-many connections.** Every connection can be bidirectional. A
  neuron can also be told to ignore input from a connection when other
  conditions hold.
- **Adaptive (the trail is built; habits between runs are planned).** The
  ground ([decision 0012](docs/decisions/0012-the-ground.md)) keeps strength
  per place, evaporates it, and routes replies that name no `to` by a seeded
  draw — the colony demo runs on it. What is still planned: carry-over
  between runs, so a network brings its trails with it, and a network that
  rewires itself.
- **Simple at the core.** The neuron interface is most valuable the simpler
  it is: wrapping something should stay a few lines, whatever the network
  learns to do around it.
- **Configured at the wrapper.** How a neuron listens, when it stops and what
  counts as done are set on each neuron's wrapper, not by a central
  controller.

Where it could be useful, ranked honestly by fit: [docs/USE-CASES.md](docs/USE-CASES.md).

The network file format, the neuron contract and the routing rule are
specified in [docs/SPEC.md](docs/SPEC.md) (version 0.1, draft).

## Why "neuron"

The name is a loose analogy, not a model of biology. A neuron here stands for
connective tissue between arbitrary functions and agents: the part that
decides what is connected to what and what flows between them. The in-run
part of how strongly things are connected is built (`ground`,
[decision 0012](docs/decisions/0012-the-ground.md)); the part that keeps
habits between runs is still planned.

## What the wrapper gives what's inside it

- **Query and answer:** a simple way to ask a neuron something and get an
  answer back.
- **Channels, passive and active:** a neuron can broadcast to every neuron,
  or target one neuron or a set of neurons.

## Signals

Signals start as **free text or JSON**. More data types are expected later;
which ones, and how a wrapper converts between them and what its contents
expect, is one of the things to experiment with.

Every signal has an `id` and a `trace`, the id of the first signal of its
chain. An answer has `replyTo`, the id of the signal it answers, and shares
that signal's `trace`. When the engine forwards an answer along a link, the
new signal records `cause` (the answer's id) and stays on the trace. The
engine keeps no store of signals between runs; the [trace](docs/TRACE.md) is
the record.

## Many inputs, one neuron

When a neuron has many incoming connections, how it responds is an option
set per neuron. By default it acts on each input as it arrives. Two more ways
are expected: combining all inputs into one picture before acting, and
weighing senders differently. One is built:

**Threshold.** A neuron with `"threshold": 2, "within": 5000` acts
only when two distinct senders have each sent it an input on the same trace
(inputs in different traces never combine). Its program then runs once, with
the inputs that met the threshold. Details ([SPEC section 5](docs/SPEC.md)):

- Each sender counts once; its newer input replaces its older one (the older
  is reported as a drop, reason `replaced`).
- Held inputs get no answer of their own. The call's answer is for the input
  that crossed the threshold.
- A running call is cut when any input it acted on is replaced by a newer one
  (same sender, same `replaces` key), not only the one that crossed. The trace
  tells it from a held input's drop by `phase` (`running` or `waiting`).
- An input still held after `within` ms (default 10000) is dropped as `expired`.
- Once a trace has acted, a later input on it starts a new hold.
- Without `threshold` nothing is held and no timer is made.

`hypothesis` in the alert storm is one: run `npm run demo:network` to watch it.

## Loops and finishing

Connections can form loops, so something has to stop a network from running
forever, and something has to say when it has an answer. Both are configured
per wrapper: each neuron carries its own limits and its own idea of done,
rather than the network having one global rule.

The first limit is built: every signal carries `hops`, its place in its chain
(0 for a root, one more than the signal it answers or follows). A neuron with
`"maxHops": 3` refuses a signal with more than 3 hops: its program never
runs and the trace records a drop, reason `hops`. Without `maxHops` there is
no limit, and `check` refuses a cycle in which no neuron has one. Hops count
along a trace, two for each neuron on the path (the reply, then the forward):
[SPEC 4.4](docs/SPEC.md), with fixtures under `spec/fixtures/hops/`.

## Composition

A network of neurons is itself a piece of software, so it can be wrapped as
a neuron. Neurons compose: a neuron can contain a whole network, and from
outside it looks and behaves like any other neuron.

The wrapper of a composed neuron is a **boundary**. It works like a force
field: the neurons inside it cannot be reached from outside. Other neurons
connect to the composed neuron, never to what is inside it, and everything
that crosses the boundary goes through the composed neuron's own interface.

- **Sealed both ways.** Neurons inside cannot reach out past the boundary
  either; whatever leaves goes out through the composed neuron.
- **Traceable from inside only.** A signal can be traced through the
  neurons inside, but only from within the composed neuron. From outside,
  the trace shows a signal going in and an answer coming out.
- **Learning stays inside (planned).** The intent is that connections that
  strengthen inside a composed neuron stay inside it, and what shows outside
  is the result.
- **Each use learns on its own (planned).** The same composed network used in
  two places would become two neurons that learn separately.

The working example is `diagnosis` in the alert storm (`npm run demo:network`):
a network file of its own, `spec/examples/diagnosis.network.json`, that the outer
network names as one neuron (`"network": "diagnosis.network.json"`). The
boundary is sealed by the engine: the outer file has no way to name an inner
neuron, and the outer trace holds one request and one answer (the demo's last
line checks that no inner id appears in it). A whole network can also sit on
another engine and be one neuron to this one (`npm run demo:p2p`). It does
not show learning, which does not exist yet: nothing is kept between runs,
so "each use learns on its own" holds only because nothing is kept.

## Run a network file

A network is a file: neurons, one-way links, thresholds,
and each neuron's program as a command. The engine (`rust-engine/`, the
`neuron-engine` binary) reads it, with no caller code:

```sh
cargo build --release --manifest-path rust-engine/Cargo.toml    # or: npm run engine:build
alias neuron-engine=rust-engine/target/release/neuron-engine
neuron-engine check spec/examples/alerts.network.json
neuron-engine run spec/examples/alerts.network.json \
  --inputs spec/examples/alerts.inputs.jsonl --trace storm.json
npm run demo:network        # the alert storm from its file, summarised from the trace
```

`check` validates the file against the schema and reports what a schema cannot
(unknown ids with a "did you mean", a missing composed file, a cycle without
`maxHops`, a budget or cost that is not a whole number or names an unknown unit), with line and column. `run` injects the timed inputs into the
file's sources (one JSON object per line: `at` in ms, `source`, `payload`,
optional `replaces`, optional `budget` to lower that trace's budget), runs every neuron as a process, forwards answers along
links, and writes a [trace](docs/TRACE.md) that the replay module
(`examples/trace/replay.mjs`) reads. A neuron is any program: `spec/examples/neurons/` has the
same `hypothesis` neuron in JavaScript and in Python, and
`alerts-python.network.json` swaps one for the other. A neuron can also be one
long-lived process speaking JSON lines (`"serve"`, level 1): the engine starts it
once, matches replies to calls, sends it `cancel` for stale work, pings it, and
restarts it with backoff if it dies. `hypothesis_serve.py` is that neuron in 29
lines of standard-library Python, and `alerts-python-serve.network.json` runs the
storm with it (`npm run demo:network -- spec/examples/alerts-python-serve.network.json`).
The engine runs every kind of neuron: `source`, level 0 (`run`), level 1 (`serve`),
`http`, `human` and composed (`network`). What it adds to the
[specification](docs/SPEC.md) (version 0.1, draft), and what it leaves out, are in its
Appendix C.

### Budgets

A network file may declare a `budget` (named units, whole numbers) and give a neuron a
`cost`. Each trace gets its own pool: a call reserves its cost when it starts, a call that the
pool cannot cover is refused with an `error` of code `budget`, and every call that started consumes
what it reserved, whether it answered, was cut or crashed. A composed neuron's cost caps its inside.
Under a budget every neuron but a source declares a cost (0 is allowed), so nothing spends unseen. The trace shows it all (`reserve` and `commit` notes with the time the engine measured, and one
`finished` event per trace with what is left), and a producer may lower a trace's budget when it
injects, never raise it. A network with no budget works as before, bar the `finished` events. See
[SPEC section 10](docs/SPEC.md) and [decision 0006](docs/decisions/0006-budgets.md).

```sh
npm run demo:network -- spec/examples/alerts-budget.network.json   # the storm with 12 credits an alert
```

### Effects and the live stream

A neuron that does something in the world is declared `"effect": true`. A newer input never cuts a
call that has started (nothing is cancelled, its answer is recorded, its cost committed), it only keeps
that answer from being forwarded (`"forwarded": false` on the `answer` event); and if the call crashes, hangs
or times out, the trace says `unknown`, not `error`, because nobody can tell whether the effect
happened. The engine reconciles nothing: a producer or an observer injects the id from the payload into a
declared source (SPEC 11.4). Every event is also offered as it happens, in written order,
as JSON lines (`run --stream <path>`); a consumer that fails never changes the run
(SPEC 9.1). See [SPEC section 11](docs/SPEC.md) and [decision 0007](docs/decisions/0007-effects-and-the-live-stream.md).

```sh
# a notification that a newer build update does not cancel, and a crash recorded as unknown
SENT_LOG=/tmp/sent.log neuron-engine run spec/examples/notify.network.json \
  --inputs spec/examples/notify.inputs.jsonl --stream notify.jsonl
npm run demo:human    # a person approves, edits or refuses each proposed rewrite on a local page
```

### Conformance fixtures

Any further engine must give the same events as this one on
the same inputs. The shared test is [`spec/fixtures/`](spec/fixtures/): 303
small fixtures, each a network file, scripted inputs, scripted neuron replies with
latencies on a virtual clock, and the exact events expected, ids normalised. No
process starts and no clock is read, so a run is exactly repeatable.

```sh
neuron-engine conform spec/fixtures   # pass or fail per fixture; non-zero exit on a failure
neuron-engine conform spec/probes     # ten more, kept apart from the contract
npm run conform                       # builds, then both
```

The format and the command-line contract any engine implements are in
[docs/CONFORMANCE.md](docs/CONFORMANCE.md). None is pending; the fixtures under `budget/` pin the budget
of version 0.1 and the `finished` event, `effect/` the effect option and the `unknown` outcome, `stream/` the live stream, and
`ground/` the trail layer ([decision 0012](docs/decisions/0012-the-ground.md): deposits, ageing, the seeded draw), and
`neuronfile/` a neuron described in a file of its own, which the network that names it may only narrow ([decision 0019](docs/decisions/0019-the-neuron-file.md)), and
`may/` what a neuron's program is declared to touch ([decision 0016](docs/decisions/0016-the-self-describing-neuron.md): checked by every engine; the Rust engine holds a program to it on macOS when a run asks with `--confine`, [decision 0018](docs/decisions/0018-confining-a-program-to-its-may.md)).

#### The engines there were

The fixtures were first checked against a TypeScript reference engine. A first Rust attempt was tried and stopped
([decision 0005](docs/decisions/0005-rust-engine-experiment.md)). An engine in Python, written from the specification
and the fixtures alone with the standard library only, then tested whether the specification is a protocol and not
just the TypeScript code: it passed every fixture, found places where the fixtures pin more than the text says, and
left the ten probes now in [`spec/probes/`](spec/probes/). The Rust engine restarted from the text under the same
terms ([decision 0009](docs/decisions/0009-rust-engine.md)) and passed them too. Each contract step was then built
three times, and [decision 0023](docs/decisions/0023-one-engine.md) deleted the TypeScript library, the TypeScript
engine and the Python engine: the commit before its build has them, with `docs/conformance-python.md`, the Python
engine's account. What that costs (three engines no longer cross-check each hand-written expectation, and the mutant
test that measured the fixtures is gone) is said plainly in the decision.

#### The engine, in Rust

[`rust-engine/`](rust-engine/) is the engine the fixtures were asking for: written from [SPEC](docs/SPEC.md), [CONFORMANCE](docs/CONFORMANCE.md), the network schema and the fixtures alone (the TypeScript engine of the day was used as a black box), in Rust with the standard library only (a one-package `Cargo.lock`, nothing fetched). It runs a real network end to end: the process layer (level 0 `run`, level 1 `serve`, pings, restart backoff, SIGTERM then SIGKILL after the kill grace with the 6.3 notes), real `http` neurons (`https` is refused honestly — no TLS in a std-only crate), `human` neurons answered on the desk of Appendix D ([decision 0008](docs/decisions/0008-human-neurons-on-a-page.md)), budgets, effects and the live stream. It carries the family's MCP surface: `neuron-engine mcp` speaks JSON-RPC 2.0 over stdio with `check_network`, `describe_network` (the shape of a network file tree, composed networks inside, for drawing it before any traffic), `run_network`, `start_run`/`poll_run` (a run as a job, with live `run://{id}` resources and notifications) and `read_trace`, and serves SPEC, CONFORMANCE, TRACE and the schema as embedded `spec://` resources. `conform`, the fixtures and human networks are deliberately unexposed there: the desk is the person's surface, not an MCP surface. Every trace it writes declares its ids scheme in the header (`{"signal":"run-numbered","trace":"run-numbered"}` — ids are opaque strings, compared, never parsed, per [TRACE.md](docs/TRACE.md)). P0 of the peer layer is built ([decision 0013](docs/decisions/0013-p2p.md)): `run --peers <file>` names the machines that execute some of the network's `run` neurons, and `neuron-engine peer` is the worker. A dispatched call runs the same level 0 contract on the peer and comes back through the engine's queue, so the run is the run a local one is — same answers, same events, wall-clock stamps aside. It is the seam, not yet the protocol: placement covers `run` neurons and whole networks, and cancel, `serve`, the ground and quiescence are P1–P4 in [docs/P2P.md](docs/P2P.md). A peer can also *be* one neuron whose inside is a network (`peer --network <file>`), which is the composability Kevin asked for: an outer run places a `network` node on a peer and composes with it exactly as with a composed neuron planted locally, down to the `inner` error and the sealing — and the hosted graph can itself place its nodes on other peers. `npm run demo:p2p` launches four peer engines and a driver from this repo and prints the answer, the trace and where the run lives: `classify` on one engine forwards to `review` on the next, whose `wrap` runs on a third and whose `audit` — a whole network — runs on a fourth.

```sh
cargo test --release --manifest-path rust-engine/Cargo.toml                  # the engine's own tests (npm run engine:test runs them one at a time)
cargo run --manifest-path rust-engine/Cargo.toml -- conform spec/fixtures    # 303 passed, 0 failed, 0 pending
cargo run --manifest-path rust-engine/Cargo.toml -- conform spec/probes      # 10 passed, 0 failed, 0 pending
cargo run --manifest-path rust-engine/Cargo.toml -- mcp                      # the MCP server on stdio
cargo run --manifest-path rust-engine/Cargo.toml -- run net.json --inputs in.jsonl --peers peers.json  # some calls run on another machine (P0, decision 0013)
cargo run --manifest-path rust-engine/Cargo.toml -- peer --listen 127.0.0.1:7301 --token <hex>   # the worker that side of the wire
npm run demo:p2p                                                             # four engines, three graphs, one answer: the composability demo
```

Gates, standing rules (rustfmt at 140, clippy `-D warnings`) and the size tables: [rust-engine/README.md](rust-engine/README.md). The direction record — why a second engine, what complete means against the first attempt's no-go, and the gates rerun on merged `main` — is [decision 0009](docs/decisions/0009-rust-engine.md). The voice-rewrite tool timbre runs this engine as one built binary: the dependency is a binary, not a package.

## The first example (history)

The project began as a TypeScript library with one example: three neurons wrapping three very different things.

- **a website with an API**, wrapped so its requests and responses become
  signals;
- **a shell script**, wrapped so its input and output become signals (a
  failed exit is reported as an error);
- **a program waiting for a human response**, wrapped so a person is a
  neuron like any other.

If those three can be connected and exchange signals through the same
interface, the wrapper idea works. It did. The library and that example went with
[decision 0023](docs/decisions/0023-one-engine.md); in a network file the three are the `http`, `run` and `human`
kinds of neuron.

## A second example (planned)

A search box on a music site, run as a network of neurons in real time:

- **the search box** is a neuron. As someone types, it broadcasts each
  keystroke, or the query so far, to every neuron it is connected to;
- **music search** acts once there are a few characters and returns tracks;
- **artist** acts only when the text matches an artist it knows;
- **lyrics** acts when the text looks like a line from a song rather than a
  name;
- **concerts** listens to the artist neuron, not to the box, and acts once
  an artist has been named;
- **links** turns any result into links for each streaming service.

Every neuron hears every keystroke; each one's wrapper decides whether the
input is worth acting on. Nothing in the middle decides for them. The example
will use a made-up catalogue.

It needs three things the first example did not have. Two are built: answers
forwarded along links without anyone waiting for them, and a way to drop work made stale by
a newer keystroke (`replaces`). Still missing: a page to watch it happen on. It is
also a test bed for the open questions below: keystrokes are a candidate for passive traffic, the concerts neuron is a
chain inside the network rather than routing by outside code, and the
results a person clicks are a candidate signal for strengthening a
connection.

## Open questions

All of these are up for debate; experiments should inform them.

**Connections**

- What counts as a connection being useful, and who decides: any traffic
  across it, an answer that came back without an error, or a rating from the
  sender, the receiver, or an observer (possibly a human)?
- What does strength change: whether a broadcast reaches a weak link at all,
  how much a link's input counts toward a threshold, or the order links are
  tried in?
- Is strength one number per pair of neurons, one per direction, or each
  neuron's private rating of its neighbours?
- What happens to a weak connection: it loses priority, it is muted and can
  recover, or it is cut and the network tries new ones?
- Where do strengths live between runs: in a file that can be read and
  edited, or in each neuron's wrapper configuration, so a neuron brings its
  habits with it?

**Channels**

- What is a passive channel, as opposed to an active one: a send that does
  not wait for an answer, something the receiver pulls or subscribes to, or
  a signal that is noted without making the neuron act?
- Is passive or active set on the connection, on the neuron, or by the
  sender for each signal?
- Does passive traffic count toward strength and loop limits?

**Composition**

- How does a composed neuron's learning show outside: only in better
  answers, or also as something its neighbours can see, such as a stronger
  connection to it or a capability it declares?

**Flow**

- When a neuron answers, who decides where the answer goes next: code
  outside the network, the neuron's wrapper (forwarding to some or all of its
  connections), or only back to whoever asked?
- When one neuron reaches its idea of done, does the whole run stop, does
  only that neuron go quiet, or is the caller told and left to decide?
- Do neurons run in one program, as separate local processes, or across
  machines?

## Status

The specification is version 0.1, a draft, with 303 conformance fixtures, and one engine runs it: the Rust engine in
`rust-engine/` ([decision 0023](docs/decisions/0023-one-engine.md)). Built: network files with a closed schema; neurons
of every kind (`run`, `serve`, `http`, `human`, composed, and a neuron named by its own file); a `threshold` of distinct
senders on one trace, with a running call cut when any input it acted on is replaced; stale-work dropping by `replaces`;
`maxHops`; budgets; effects; `may` and confinement on macOS; `maxInflight`; the live stream; peers; the MCP server. The
trail layer (`ground`, [decision 0012](docs/decisions/0012-the-ground.md)) is built; carry-over between runs, rewiring,
ignore conditions, limits other than `maxHops`, and passive/active channels are not built yet. Every signal carries a
`trace` shared by everything it leads to; answers record the signal they answer (`replyTo`), and a forwarded signal its
`cause`. An alert storm (`npm run demo:network`) puts threshold, stale-work dropping, wrapped scripts and a composed
diagnosis neuron together. The team demo (`npm run demo:team`) runs the engine's desk as a shared decision surface for
three people over real git branches, the merge among them an effect — the multiplayer protocol pieces around it (grants
at the desk's POST, durable holds, presence, quiescence) are
[issue #30](https://github.com/overfocus/agent-neuron/issues/30)'s SPEC gaps, positioned in
[docs/MULTIPLAYER.md](docs/MULTIPLAYER.md). Every demo is listed in [docs/DEMOS.md](docs/DEMOS.md). The decision-surface demos
series — `quorum`, `backlog`, `spend`, `refunds`, `oncall` and `colony` ([decision 0012](docs/decisions/0012-the-ground.md)'s
first user: the ants' returns routed by the engine's ground) — lives in
[agent-neuron-demos](https://github.com/overfocus/agent-neuron-demos), watched in 3D by
[agent-neuron-viz](https://github.com/overfocus/agent-neuron-viz), and rendered from
their own recorded runs as social videos in that repo's `video/`.

## Run the demos

```sh
npm run demo:network                    # the alert storm from its network file (about 7 s)
npm run demo:network -- spec/examples/alerts-budget.network.json   # the same with a budget; also alerts-python and alerts-python-serve (need python3)
npm run demo:human                      # a person in the loop: open the address it prints on stderr and answer the three items
npm run demo:team                       # the team decision on three pages; `-- --headless` and `-- --headless --tie` need no browser
npm run demo:p2p                        # four peer engines and a driver on this machine
```

Each builds the engine first. What each shows: [docs/DEMOS.md](docs/DEMOS.md).

## Development

A Rust toolchain (`cargo` on your PATH) for the engine, and Node 22 or later for `npm run` and the demos; `package.json` has no dependencies, so there is nothing to install. The engine is POSIX only (macOS or Linux) and refuses `https` (no TLS in a std-only crate). Python 3 is optional, for the Python variants of the alert storm and some of the engine's tests.

```sh
npm run check   # cargo fmt --check, clippy -D warnings, the engine's tests, a release build, the fixtures and the probes
```

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[MIT](LICENSE)
