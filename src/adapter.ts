import type { Payload, Signal } from './signal.js';

/** Extra, optional input to `handle`. `abort` fires when the work is no longer wanted; stop and reject. */
export type AdapterContext = { abort: AbortSignal };

/** The one thing that differs between neurons: how to turn a payload into an answer. */
export interface Adapter {
  handle(payload: Payload, signal: Signal, context?: AdapterContext): Promise<Payload>;
}
