// Types for replay.mjs, so the tests can import it under strict TypeScript.
export type TraceEvent = {
  t: number;
  type: 'signal' | 'answer' | 'quiet' | 'drop' | 'error' | 'fail';
  id?: string;
  from?: string;
  to?: string[];
  trace?: string;
  replyTo?: string;
  cause?: string;
  hops?: number;
  /** How a `signal` was sent; absent means `emit`. */
  via?: 'emit' | 'send';
  neuron?: string;
  reason?: 'replaced' | 'expired' | 'hops';
  message?: string;
  payload?: { type: 'text'; text: string; truncated?: boolean } | { type: 'json'; data?: unknown; truncated?: boolean; preview?: string };
};
export type TraceNeuron = { id: string; kind: string; description: string; options?: { threshold?: number; within?: number; maxHops?: number } };
export type TraceLink = { from: string; to: string; strength?: number };
export type Trace = {
  format: string;
  version: number;
  demo: string;
  description: string;
  library: string;
  parameters?: Record<string, unknown>;
  neurons: TraceNeuron[];
  links: TraceLink[];
  events: TraceEvent[];
  summary: Record<string, unknown>;
};
export type NeuronCounts = { sent: number; answers: number; quiet: number; errors: number; acted: number; dropped: Record<string, number> };
export type ReplayState = {
  t: number;
  neurons: Record<string, NeuronCounts>;
  edges: Record<string, Record<string, number>>;
  held: Record<string, { id: string; from: string; trace: string }[]>;
  thresholds: Record<string, number>;
  maxHops: Record<string, number>;
  weights: Record<string, Record<string, number>>;
};
export function start(trace: Pick<Trace, 'neurons'> & { links?: TraceLink[] }): ReplayState;
export function step(state: ReplayState, event: TraceEvent): string[];
export function replay(trace: Pick<Trace, 'neurons' | 'events'> & { links?: TraceLink[] }, t?: number): ReplayState;
