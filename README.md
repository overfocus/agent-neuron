# agent-neuron

> **Experimental, concept stage.** This README describes the idea; the code
> implements a few small demos so far (see [docs/DEMOS.md](docs/DEMOS.md)).

An **agent-neuron** is a universal wrapper for any piece of software: an AI
agent, a script, an executable, a plain function, or something else entirely.
The wrapper adapts whatever is inside it, whether that is an API, a command
line or a stream of input and output, to one common interface, so that any
neuron can connect to any other.

Neurons connect **many to many**. Together they form a network that can
rewire itself, strengthening the connections that matter and weakening the
ones that don't.

This is an experiment, for experimenters and technologists and anyone
interested in new ideas about how agents can be connected.

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
- **Adaptive.** The network can rewire itself, and connections can
  strengthen and weaken over time. Strengths are meant to carry over from one
  run to the next, so the network keeps what it learns.
- **Simple at the core.** The neuron interface is most valuable the simpler
  it is: wrapping something should stay a few lines, whatever the network
  learns to do around it.
- **Configured at the wrapper.** How a neuron listens, when it stops and what
  counts as done are set on each neuron's wrapper, not by a central
  controller.

Where it could be useful, ranked honestly by fit: [docs/USE-CASES.md](docs/USE-CASES.md).

The network file format, the neuron contract and the routing rule are
specified in [docs/SPEC.md](docs/SPEC.md) (version 0, draft).

## Why "neuron"

The name is a loose analogy, not a model of biology. A neuron here stands for
adaptive connective tissue between arbitrary functions and agents: the part
that decides what is connected to what, how strongly, and what flows between
them.

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
chain. An answer from `send` or `emit` has `replyTo`, the id of the signal it
answers, and shares that signal's `trace`. Outside code that passes an answer
on gives the next `send` or `emit` `{ cause: answer }`: the new signal records
`cause` (the answer's id) and joins its trace. Nothing passes answers on by
itself yet, and the network does not store signals; keep a map by id if you
need to look one up.

## Many inputs, one neuron

When a neuron has many incoming connections, how it responds is an option
set per neuron. By default it acts on each input as it arrives. Two more ways
are expected: combining all inputs into one picture before acting, and
weighing senders differently. One is built:

**Threshold.** `new Neuron(id, adapter, { threshold: 2, within: 5000 })` acts
only when two distinct senders have each sent it an input on the same trace
(inputs in different traces never combine). The adapter then runs once, and
`context.inputs` holds the inputs that met the threshold, the one that
crossed it last. Details:

- Each sender counts once; its newer input replaces its older one (the older
  is reported as `onDrop(..., 'replaced')`).
- Held inputs get no answer of their own, and no drop once consumed, even if
  the run is aborted or fails. The run's answer is for the input that crossed
  the threshold.
- A running call is cut when any input it acted on is replaced by a newer emit
  (same sender, same `replaces` key), not only the one that crossed: its adapter
  is aborted and `onDrop(crossingInput, id, 'replaced')` is reported once. The
  other inputs report nothing. The trace tells it from a held input's drop by
  `phase` (`running` or `waiting`) in new recordings (the committed traces predate it).
- An input still held after `within` ms (default 10000) is dropped with
  `onDrop(..., 'expired')`. An input whose `emit` is replaced (same `replaces`
  key, newer emit) is dropped as `'replaced'` and stops counting.
- It applies to `emit`. `send` is a direct exchange, like `Neuron.query`, so it
  skips the threshold and runs the adapter at once, without `context.inputs`.
- `idle()` does not wait for held inputs: they are waiting, not working, and
  waiting 10 s for them would help nobody. Their timers do not keep the process
  alive, so a program that exits first never sees the `'expired'` drop.
- Once a trace has acted, a later input on it starts a new hold.
- Without `threshold` nothing is held and no timer is made.

Run `npm run demo:suggest` to watch one.

## Loops and finishing

Connections can form loops, so something has to stop a network from running
forever, and something has to say when it has an answer. Both are configured
per wrapper: each neuron carries its own limits and its own idea of done,
rather than the network having one global rule.

The first limit is built: every signal carries `hops`, its place in its chain
(0 for a root, one more than the signal it answers or follows through
`replyTo` or `cause`). A neuron created with `new Neuron(id, adapter, { maxHops: 3 })`
refuses a signal with more than 3 hops: `emit` reports it through
`onDrop(signal, id, 'hops')` and the adapter never runs, and a waiting `send`
leaves that neuron's answer out. Without `maxHops` there is no limit.
A refused signal is still reported to `onSignal` (refusal is per neuron, and on a broadcast others may accept it), and `send` leaves the refusing neuron's answer out without an `onDrop`. `Neuron.query` is a direct ask and skips it. Hops only count along a trace:
code that passes an answer on without `{ cause }` starts a new trace at 0, so
forward with `cause` if you want the limit to see the loop. The idiom: an answerer passes its answer on with `network.emit(answer.from, answer.payload, { to: answer.to, cause: answer })`. Run
`npm run demo:loop` to watch one being cut.

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
- **Learning stays inside.** Connections that strengthen inside a composed
  neuron stay inside it. What shows outside is the result: the composed
  neuron as a whole becomes more capable.
- **Each use learns on its own.** The same composed network used in two
  places becomes two neurons that learn separately.

The first working example is `npm run demo:composed`: a Rust program
`music` with a tiny inner network (tracks, artist and lyrics neurons) that
the outer network sees as one neuron. The boundary is sealed by
construction: `music` is a separate process, its inner handlers can only
return text, and the outer network has no way to name an inner neuron (a
test checks that no inner id appears outside). The trace exists inside
only: the outer signals are one request and one answer, while
`music --inspect` run on its own prints the inner signals. Running it
standalone is looking inside, not the outer network reaching in. It does
not show learning, which does not exist yet: each call is its own process,
so "each use learns on its own" holds only because nothing is kept.

## Run a network file

A network can be a file instead of code: neurons, one-way links, thresholds,
and each neuron's program as a command. The reference engine (`src/engine/`,
also importable as `agent-neuron/engine`) reads it, with no caller code:

```sh
npm run engine -- check spec/examples/alerts.network.json
npm run engine -- run spec/examples/alerts.network.json \
  --inputs spec/examples/alerts.inputs.jsonl --trace storm.json
npm run demo:network        # the alert storm from its file, summarised from the trace
```

`check` validates the file against the schema and reports what a schema cannot
(unknown ids with a "did you mean", a missing composed file, a cycle without
`maxHops`), with line and column. `run` injects the timed inputs into the
file's sources (one JSON object per line: `at` in ms, `source`, `payload`,
optional `replaces`), runs every neuron as a process, forwards answers along
links, and writes a [trace](docs/TRACE.md) that the replay module and the
network view read. A neuron is any program: `spec/examples/neurons/` has the
same `hypothesis` neuron in JavaScript and in Python, and
`alerts-python.network.json` swaps one for the other. A neuron can also be one
long-lived process speaking JSON lines (`"serve"`, level 1): the engine starts it
once, matches replies to calls, sends it `cancel` for stale work, pings it, and
restarts it with backoff if it dies. `hypothesis_serve.py` is that neuron in 29
lines of standard-library Python, and `alerts-python-serve.network.json` runs the
storm with it (`npm run demo:network -- spec/examples/alerts-python-serve.network.json`).
This engine runs `source`, level 0 (`run`), level 1 (`serve`) and composed
(`network`) neurons; the rest of the
[specification](docs/SPEC.md) (version 0, draft) and the gaps are in its
Appendix C.

### Conformance fixtures

A second engine (the Rust one, later) must give the same events as this one on
the same inputs. The shared test is [`spec/fixtures/`](spec/fixtures/): 148 (138 run, 10 pending)
small fixtures, each a network file, scripted inputs, scripted neuron replies with
latencies on a virtual clock, and the exact events expected, ids normalised. No
process starts and no clock is read, so a run is exactly repeatable.

```sh
npm run engine -- conform spec/fixtures   # pass or fail per fixture; non-zero exit on a failure
```

The format and the command-line contract any engine implements are in
[docs/CONFORMANCE.md](docs/CONFORMANCE.md). Ten fixtures under `budget/` are marked
pending: they are the plan for the next specification version.

## The first example

Three neurons wrapping three very different things:

- **a website with an API**, wrapped so its requests and responses become
  signals;
- **a shell script**, wrapped so its input and output become signals (a
  failed exit is reported as an error);
- **a program waiting for a human response**, wrapped so a person is a
  neuron like any other.

If those three can be connected and exchange signals through the same
interface, the wrapper idea works.

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

It needs three things the first example does not have. Two are built: a send
that does not wait for answers (`emit`) and a way to drop work made stale by
a newer keystroke (`replaces`), shown in a terminal demo,
`npm run demo:typing`. Still missing: a page to watch it happen on. It is
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

The first example works: shell, HTTP and human adapters, neurons with
bidirectional links, and a network that broadcasts or targets signals
(`src/`). A send that does not wait for answers (`emit`) can replace older work from
the same sender, and adapters stop when it is dropped. A neuron can wait for a `threshold` of distinct senders on one trace (`npm run demo:suggest`); its running call is cut when any input it acted on is replaced. An alert storm demo (`npm run demo:alerts`) puts threshold, stale-work dropping, wrapped scripts and a composed diagnosis neuron together. Link strength,
rewiring, ignore conditions, limits other than `maxHops`, and passive/active channels are not
built yet. Every signal carries a `trace` shared by everything it leads to;
answers record the signal they answer (`replyTo`), and `send` and `emit` take
an optional `cause` to continue a trace. Nothing passes answers on by itself,
and the network keeps no store of past signals. Listeners for `emit` work (`onSignal`, `onError`, `onDrop`) are
called safely: if one throws, the error is swallowed. A small Rust program also runs as a neuron through the shell adapter (`npm run demo:rust`), showing the wrapper is not tied to one language. A local page draws any of three demos live as they run (`npm run view`). Every demo is listed in [docs/DEMOS.md](docs/DEMOS.md).

## Run the first example

```sh
echo "approve" | npm run example
```

A shell script summarises some text, then its two linked neurons, a local
HTTP API and a human on stdin, each receive the summary. Every signal is
printed as it travels, then the human's reply. Nothing leaves localhost.

## Run the typing demo

```sh
npm run demo:typing
```

A search box types a query one letter at a time; three neurons with
different latencies answer or stay quiet, and work made stale by the next
letter is aborted and printed as dropped. Answers do not repeat the prefix:
the demo finds it through `replyTo`. Made-up catalogue, no network.

## Watch a demo

```sh
npm run view
```

Builds, then prints a `http://127.0.0.1:<port>/` address (a free port, this machine only). Open it, pick
the alert storm, typing or suggest demo and press Run: the network is drawn
from its traffic as it happens, with answers, dropped work and held inputs
labelled, and a log of the same events beside or below it. Stop the server with Ctrl+C. No
framework, no outside requests.

## Record a trace

```sh
npm run record -- alerts      # also: typing, suggest; writes traces/<demo>.json
```

Runs a demo and writes every signal, answer, drop and error, with its time, as
one JSON file that can be replayed later without running anything. The alert
storm is also recorded with `--threshold 1` and `3`. Format and replay:
[docs/TRACE.md](docs/TRACE.md).

## Run the Rust demo

```sh
npm run demo:rust
```

The typing demo again, with the tracks neuron replaced by a small Rust
program (`rust/catalogue`, standard library only) wrapped by the shell
adapter. The output shows `tracks (rust)` answering the same queries, and
stale work still dropped. Needs a Rust toolchain (`cargo` on your PATH).

## Run the composed demo

```sh
npm run demo:composed
```

The search box sends every keystroke to one neuron, `music`: a Rust program
(`rust/music`, standard library only) wrapped by the shell adapter. Inside
it is its own network of tracks, artist and lyrics neurons. The output
shows answers combining all three and stale work dropped (the inner lyrics neuron takes 250 ms once it acts, as in the typing demo), then the inside
view: one standalone `music --inspect` run printing the inner trace. Needs
a Rust toolchain.

## Run the alert storm demo

```sh
npm run demo:alerts
```

Forty invented alerts arrive in about six seconds. Two shell scripts and a
JavaScript function look at each one; a stand-in for a model runs only when
two of them agree, and a composed neuron answers with one diagnosis. The
output ends with how many expensive calls were avoided and how much stale
work was dropped; the numbers are the same on an idle machine.

## Development

Node 22 or later, and a Rust toolchain (`cargo` on PATH, typically rustup's `~/.cargo/bin`): `npm run check` and `npm test` build and run the Rust example.

```sh
npm install
npm run check   # typecheck, Rust test and build, test, build
```

## License

[MIT](LICENSE)
