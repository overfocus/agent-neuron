# agent-neuron

> **Experimental, concept stage.** This README describes the idea; the code
> implements only the first example so far.

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
  strengthen and weaken over time.
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
- **a shell script**, wrapped so its arguments, output and exit code become
  signals;
- **a program waiting for a human response**, wrapped so a person is a
  neuron like any other.

If those three can be connected and exchange signals through the same
interface, the wrapper idea works.

## Open questions

These are deliberately undecided; experiments should inform them.

- What makes a connection strengthen or weaken: how often it is used,
  whether its answers were useful, or a rule set on the wrapper?
- What is the difference between a passive and an active channel?

## Status

The first example works: shell, HTTP and human adapters, neurons with
bidirectional links, and a network that broadcasts or targets signals
(`src/`). Link strength, rewiring, ignore conditions, loop limits and
passive/active channels are not built yet.

## Run the first example

```sh
echo "approve" | npm run example
```

A shell script summarises some text, then its two linked neurons, a local
HTTP API and a human on stdin, each receive the summary. Every signal is
printed as it travels, then the human's reply. Nothing leaves localhost.

## Development

Node 22 or later.

```sh
npm install
npm run check   # typecheck, test, build
```

## License

[MIT](LICENSE)
