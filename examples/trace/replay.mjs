// Folds a trace's events (docs/TRACE.md) into derived state. Pure: no Node or browser APIs, no imports, so it can be
// copied into a bundle as is. Any reader can replay a trace file with it, whole or one event at a time.
//
//   replay(trace, t)  state after every event with e.t <= t (default: all of them)
//   start(trace)      an empty state with the trace's declared neurons
//   step(state, e)    folds in one event (mutates `state`) and returns the ids of the threshold neurons that just acted
//
// State is plain JSON:
//   t                 the time of the last event folded in
//   neurons[id]       { sent, answers, quiet, errors, acted, dropped: { [reason]: n } }
//   edges[from][to]   signals that crossed from -> to (requests and answers alike)
//   held[id]          the inputs a threshold neuron is waiting on: [{ id, from, trace }]
// Held inputs follow `emit`: an input is held until `threshold` distinct senders have sent one on its trace (then all
// of that trace's inputs leave and the neuron has `acted` once) or until it is dropped. Each sender counts for its
// link's `strength` toward the threshold (default 1). A `signal` is taken to be an `emit` unless it says `via: 'send'`
// (`send` is not held); an input whose `hops` exceed the receiver's declared `maxHops` is refused, so not held either.

const fresh = () => ({ sent: 0, answers: 0, quiet: 0, errors: 0, acted: 0, dropped: {} });

export function start(trace) {
  const state = { t: 0, neurons: {}, edges: {}, held: {}, thresholds: {}, maxHops: {}, weights: {} };
  for (const n of trace.neurons ?? []) {
    state.neurons[n.id] = fresh();
    if (n.options?.maxHops !== undefined) state.maxHops[n.id] = n.options.maxHops;
    if (n.options?.threshold !== undefined) {
      state.thresholds[n.id] = n.options.threshold;
      state.held[n.id] = [];
    }
  }
  // weights[to][from]: how much `from` counts toward `to`'s threshold. Only links that state a strength are listed.
  for (const l of trace.links ?? []) if (l.strength !== undefined) (state.weights[l.to] ??= {})[l.from] = l.strength;
  return state;
}

const neuron = (state, id) => (state.neurons[id] ??= fresh());

export function step(state, e) {
  const acted = [];
  state.t = e.t;
  if (e.type === 'signal' || e.type === 'answer' || e.type === 'quiet') {
    const from = neuron(state, e.from);
    if (e.type === 'signal') from.sent++;
    else if (e.type === 'answer') from.answers++;
    else from.quiet++;
    const out = (state.edges[e.from] ??= {});
    for (const to of e.to) {
      neuron(state, to);
      out[to] = (out[to] ?? 0) + 1;
      const refused = state.maxHops[to] !== undefined && e.hops > state.maxHops[to];
      if (e.type === 'signal' && e.via !== 'send' && !refused && state.thresholds[to] !== undefined && hold(state, to, e)) acted.push(to);
    }
  } else if (e.type === 'drop') {
    const n = neuron(state, e.neuron);
    n.dropped[e.reason] = (n.dropped[e.reason] ?? 0) + 1;
    if (state.held[e.neuron]) state.held[e.neuron] = state.held[e.neuron].filter((h) => h.id !== e.id);
  } else if (e.type === 'error') {
    neuron(state, e.neuron).errors++;
  }
  return acted;
}

// Holds input `e` for `id`; true if that met the threshold, which releases the trace's inputs.
function hold(state, id, e) {
  const held = [...state.held[id], { id: e.id, from: e.from, trace: e.trace }];
  const mine = held.filter((h) => h.trace === e.trace);
  const total = [...new Set(mine.map((h) => h.from))].reduce((sum, from) => sum + (state.weights[id]?.[from] ?? 1), 0);
  if (total + 1e-9 < state.thresholds[id]) { // the tolerance is for fractional strengths
    state.held[id] = held;
    return false;
  }
  state.held[id] = held.filter((h) => h.trace !== e.trace);
  neuron(state, id).acted++;
  return true;
}

export function replay(trace, t = Infinity) {
  const state = start(trace);
  for (const e of trace.events) {
    if (e.t > t) break; // events are in time order
    step(state, e);
  }
  return state;
}
