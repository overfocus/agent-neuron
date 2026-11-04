// Listens to a run's network and reports each thing it does as one trace event (format: docs/TRACE.md).
// Shared by the recorder (examples/trace/record.mjs) and the network view's server, so both see the same events.
//   const { listeners, fail } = tap((event) => ...);   // pass `listeners` as the demo's `listeners`
// `t` is whole milliseconds since tap() was called, read from a clock that never goes back.

const fields = (s) => ({
  id: s.id, from: s.from, to: s.to ?? [], trace: s.trace,
  ...(s.replyTo !== undefined && { replyTo: s.replyTo }),
  ...(s.cause !== undefined && { cause: s.cause }),
  hops: s.hops,
});

/** An answer that says nothing: the neuron chose not to act. */
export const isQuiet = (s) => s.replyTo !== undefined && s.payload.type === 'text' && s.payload.text === '';

export function tap(emit) {
  const t0 = performance.now();
  const at = () => Math.round(performance.now() - t0);
  const listeners = {
    onSignal: (s) => {
      const type = s.replyTo === undefined ? 'signal' : isQuiet(s) ? 'quiet' : 'answer';
      emit({ t: at(), type, ...fields(s), ...(type !== 'quiet' && { payload: s.payload }) });
    },
    onDrop: (s, neuron, reason) => emit({ t: at(), type: 'drop', ...fields(s), neuron, reason, payload: s.payload }),
    onError: (e, s, neuron) => emit({ t: at(), type: 'error', ...fields(s), neuron, message: String(e?.message ?? e), payload: s.payload }),
  };
  // The run itself failed: there is no neuron to blame.
  const fail = (e) => emit({ t: at(), type: 'fail', message: String(e?.message ?? e) });
  return { listeners, fail };
}
