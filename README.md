# agent-neuron

> **Experimental, concept stage.** This README describes the idea; the code
> does not implement it yet.

An **agent-neuron** is a universal wrapper for any piece of software: an AI
agent, a script, an executable, a plain function, or something else entirely.
The wrapper adapts whatever is inside it, whether that is an API, a command
line or a stream of input and output, to one common interface, so that any
neuron can connect to any other.

Neurons connect **many to many**. Together they form a network that can
rewire itself, strengthening the connections that matter and weakening the
ones that don't.

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

## Why "neuron"

The name is a loose analogy, not a model of biology. A neuron here stands for
adaptive connective tissue between arbitrary functions and agents: the part
that decides what is connected to what, how strongly, and what flows between
them.

## What the wrapper gives what's inside it

- **Query and answer:** a simple way to ask a neuron something and get an
  answer back.
- **Channels, passive and active:** a neuron can broadcast to every neuron,
  to the neurons that answer, or to a specific set of neurons it targets.

## Many inputs, one neuron

When a neuron has many incoming connections, how it responds is an option
set per neuron, for example:

- act on each input as it arrives;
- wait until enough inputs agree or add up, like a threshold;
- combine all of its inputs into one picture before acting.

## Open questions

The concept is still being worked out. These are not decided yet:

- What travels along a connection: free text, a structured payload with a
  declared shape, or a signal with a strength while the content lives
  elsewhere?
- When a neuron fires, who receives it by default: every connected neuron,
  the ones it chooses, or whoever subscribed to that kind of signal?
- With loops possible, what stops a network from running forever: a limit
  on hops or time, a resting period after firing, neurons that damp
  activity, a cost budget?
- How does anyone know the network has finished: an output neuron fires,
  activity dies out, or an outside observer decides?
- What makes a connection strengthen or weaken: how often it is used,
  whether its answers were useful, or a rule set by the developer?
- What is the difference between a passive and an active channel?
- What is the smallest example that makes the idea click?
- Who is this README for: developers looking for a library, researchers and
  tinkerers, or readers of "the neuron concept" as an idea?

## Status

Nothing beyond project scaffolding exists yet. The first build will be the
smallest example that shows the idea working.

## Development

Node 22 or later.

```sh
npm install
npm run check   # typecheck, test, build
```

## License

[MIT](LICENSE)
