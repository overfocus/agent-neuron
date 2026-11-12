import type { Adapter, AdapterContext } from './adapter.js';
import { type Payload, type Signal } from './signal.js';
/** Per-neuron settings. Everything is optional; a neuron with none behaves as it always did. */
export type NeuronOptions = {
    /** Refuse any signal with more hops than this (see `hops` on `Signal`). Default: no limit. */
    maxHops?: number;
    /**
     * Act only once this many distinct senders have each sent an input on one trace (see `Network.emit`).
     * The adapter then runs once, with those inputs in `context.inputs`. Default: act on every input.
     */
    threshold?: number;
    /** With `threshold`: how many ms an input is held waiting for the others. Default 10000. */
    within?: number;
};
/** A wrapper around anything, reachable through one interface. */
export declare class Neuron {
    readonly id: string;
    readonly adapter: Adapter;
    readonly options: NeuronOptions;
    readonly links: Set<Neuron>;
    constructor(id: string, adapter: Adapter, options?: NeuronOptions);
    /** Link both ways. */
    connect(other: Neuron): void;
    /** Ask this neuron directly and get its answer. */
    query(payload: Payload): Promise<Payload>;
    receive(signal: Signal, context?: AdapterContext): Promise<Payload>;
}
