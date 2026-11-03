import type { Payload, Signal } from './signal.js';

/** The one thing that differs between neurons: how to turn a payload into an answer. */
export interface Adapter {
  handle(payload: Payload, signal: Signal): Promise<Payload>;
}
