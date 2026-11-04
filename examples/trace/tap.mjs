// Listens to a run's network and reports each thing it does as one trace event (format: docs/TRACE.md).
// Shared by the recorder (examples/trace/record.mjs) and the network view's server, so both see the same events.
//   const { listeners, fail } = tap((event) => ..., neurons);   // pass `listeners` as the demo's `listeners`
// `neurons` (the declared ones, as in the trace header) lets it tell a held input's drop from a running call's: `phase`.
// `t` is whole milliseconds since tap() was called, read from a clock that never goes back.

import { start, step } from './replay.mjs';

const fields = (s) => ({
  id: s.id, from: s.from, to: s.to ?? [], trace: s.trace,
  ...(s.replyTo !== undefined && { replyTo: s.replyTo }),
  ...(s.cause !== undefined && { cause: s.cause }),
  hops: s.hops,
});

/** An answer that says nothing: the neuron chose not to act. */
export const isQuiet = (s) => s.replyTo !== undefined && s.payload.type === 'text' && s.payload.text === '';

export function tap(emit, neurons = []) {
  const walk = start({ neurons });
  const t0 = performance.now();
  const at = () => Math.round(performance.now() - t0);
  // Every event is folded into the replay state before the next one, so a drop sees what was held when it happened.
  const out = (event) => (emit(event), step(walk, event));
  const listeners = {
    onSignal: (s) => {
      const type = s.replyTo === undefined ? 'signal' : isQuiet(s) ? 'quiet' : 'answer';
      out({ t: at(), type, ...fields(s), ...(type !== 'quiet' && { payload: s.payload }) });
    },
    // A replaced input that is still held was waiting; otherwise its call was running (the library reports no phase).
    onDrop: (s, neuron, reason) => {
      const phase = walk.held[neuron]?.some((h) => h.id === s.id) ? 'waiting' : 'running';
      out({ t: at(), type: 'drop', ...fields(s), neuron, reason, ...(reason === 'replaced' && { phase }), payload: s.payload });
    },
    onError: (e, s, neuron) => out({ t: at(), type: 'error', ...fields(s), neuron, message: String(e?.message ?? e), payload: s.payload }),
  };
  // The run itself failed: there is no neuron to blame.
  const fail = (e) => emit({ t: at(), type: 'fail', message: String(e?.message ?? e) });
  return { listeners, fail };
}
