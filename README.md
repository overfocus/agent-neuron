# agent-neuron

> **Experimental, concept stage.** This README describes the idea; the code
> implements two small demos so far (see [docs/DEMOS.md](docs/DEMOS.md)).

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
  strengthen and weaken over time. Strengths carry over from one run to the
  next, so the network keeps what it learns.
- **Configured at the wrapper.** How a neuron listens, when it stops and what
  counts as done are set on each neuron's wrapper, not by a central
  controller.

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
set per neuron, for example:

- act on each input as it arrives;
- wait until enough inputs agree or add up, like a threshold;
- combine all of its inputs into one picture before acting.

## Loops and finishing

Connections can form loops, so something has to stop a network from running
forever, and something has to say when it has an answer. Both are configured
per wrapper: each neuron carries its own limits and its own idea of done,
rather than the network having one global rule.

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
the same sender, and adapters stop when it is dropped. Link strength,
rewiring, ignore conditions, loop limits and passive/active channels are not
built yet. Listeners for `emit` work (`onSignal`, `onError`, `onDrop`) are
called safely: if one throws, the error is swallowed. Every demo is listed in [docs/DEMOS.md](docs/DEMOS.md).

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

## Development

Node 22 or later.

```sh
npm install
npm run check   # typecheck, test, build
```

## License

[MIT](LICENSE)
