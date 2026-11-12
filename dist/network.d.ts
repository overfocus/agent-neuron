import { Neuron } from './neuron.js';
import { type Payload, type Signal } from './signal.js';
export type NetworkOptions = {
    /** Called for every signal: each send or emit, and each answer as it arrives. */
    onSignal?: (signal: Signal) => void;
    /** Called when a neuron fails while handling an `emit`. Never called for work that was replaced. */
    onError?: (error: unknown, signal: Signal, neuronId: string) => void;
    /** Called when a neuron's work for `signal` was dropped: 'replaced' by a newer emit, or refused for too many 'hops' (its `maxHops`),
     * or held by a neuron with a `threshold` that was not met within its `within`: 'expired'. */
    onDrop?: (signal: Signal, neuronId: string, reason: 'replaced' | 'hops' | 'expired') => void;
};
export type SendOptions = {
    /** Only these neurons (must be linked). Default: every linked neuron. */
    to?: string[];
    /** A signal that led to this one (usually an answer being passed on): the new signal joins its trace and records its id as `cause`. */
    cause?: Signal;
};
export type EmitOptions = SendOptions & {
    /**
     * A key. A newer emit from the same sender with the same key aborts the
     * older one's in-flight work, and the older work's answers are dropped.
     */
    replaces?: string;
};
export declare class Network {
    private readonly options;
    private readonly neurons;
    private readonly pending;
    private readonly held;
    private readonly replaceable;
    constructor(options?: NetworkOptions);
    add(neuron: Neuron): Neuron;
    connect(a: string, b: string): void;
    /**
     * Send from one neuron to everything it is linked to, or only to `to`
     * (which must be linked). Delivers in link order, one at a time, and
     * returns each answer as a signal from the neuron that gave it. A target
     * whose `maxHops` the signal exceeds is skipped: its answer is left out,
     * with no `onDrop` (the signal itself is still reported to `onSignal`).
     * `threshold` does not apply to `send`, which is a direct exchange: every
     * target handles the one signal at once, with no `context.inputs`.
     */
    send(fromId: string, payload: Payload, { to, cause }?: SendOptions): Promise<Signal[]>;
    /**
     * Like `send`, but returns the outgoing signal at once and does not wait.
     * Targets work in parallel; answers arrive through `onSignal`, failures
     * through `onError`. With `replaces`, a newer emit under the same key from
     * the same sender aborts the older one's work (adapters get an abort
     * signal) and nothing more is reported for it except `onDrop`. A target
     * whose `maxHops` the signal exceeds never runs: `onDrop` with 'hops'.
     *
     * A target with a `threshold` holds each input until enough distinct
     * senders have sent one on the same trace: a sender's newer input replaces
     * its older one ('replaced'), the sum of senders reaching the threshold runs
     * the adapter once with those inputs in `context.inputs` and answers the
     * last of them (the others get no answer and no drop, even if that run is aborted or fails), and an input still held
     * after the neuron's `within` is dropped as 'expired'. A held input whose
     * emit is replaced is dropped as 'replaced'. Held inputs are waiting, not
     * working: `idle` does not wait for them, and their timers never keep the
     * process alive, so a program that ends first never sees the 'expired'.
     * A running call is cut when any input it acted on is replaced, not only the
     * crossing one: one `onDrop` 'replaced' is reported, for the crossing input.
     * A later input on a trace that already ran starts a new hold.
     */
    emit(fromId: string, payload: Payload, { to, replaces, cause }?: EmitOptions): Signal;
    /** Resolves once all emitted work has finished or been dropped, including work started meanwhile. */
    idle(): Promise<void>;
    private run;
    /** Hold `signal` for `target`'s threshold. Returns the held inputs to act on if it is met, otherwise undefined. */
    private hold;
    /**
     * Listeners called for `emit` work (onSignal, onError, onDrop) cannot take
     * the network down: whatever they throw is swallowed, since nothing is
     * waiting to catch it. Catch errors inside your own listener if you care.
     */
    private notify;
    private resolve;
    private get;
}
