import type { Payload, Signal } from './signal.js';
/**
 * Extra, optional input to `handle`. `abort` fires when the work is no longer wanted; stop and reject.
 * `inputs`: for a neuron with a `threshold`, the held inputs that met it (the signal being handled is the last).
 */
export type AdapterContext = {
    abort: AbortSignal;
    inputs?: Signal[];
};
/** The one thing that differs between neurons: how to turn a payload into an answer. */
export interface Adapter {
    handle(payload: Payload, signal: Signal, context?: AdapterContext): Promise<Payload>;
}
