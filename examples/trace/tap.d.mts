// Types for tap.mjs, so the tests can import it under strict TypeScript.
import type { Signal } from '../../src/index.js';
import type { TraceEvent, TraceNeuron } from './replay.mjs';
export function tap(
  emit: (event: TraceEvent) => void,
  neurons?: Pick<TraceNeuron, 'id' | 'options'>[],
): {
  listeners: {
    onSignal: (s: Signal) => void;
    onDrop: (s: Signal, neuron: string, reason: 'replaced' | 'expired' | 'hops') => void;
    onError: (e: unknown, s: Signal, neuron: string) => void;
  };
  fail: (e: unknown) => void;
};
