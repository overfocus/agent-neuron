/** What a signal carries: free text or JSON. */
export type Payload = {
    type: 'text';
    text: string;
} | {
    type: 'json';
    data: unknown;
};
/**
 * One message on the network. `to` lists the neurons it was delivered to (for a broadcast, every linked neuron).
 * `trace` is shared by every signal in one chain. `replyTo` is the id of the signal this one answers;
 * `cause` is the id of the signal that made outside code send this one (see `cause` on `send`/`emit`).
 */
export type Signal = {
    id: string;
    from: string;
    to?: string[];
    payload: Payload;
    trace: string;
    replyTo?: string;
    cause?: string;
    /** Links in this signal's chain: 0 for a root, one more than the signal it answers or follows (`replyTo`, `cause`). */
    hops: number;
};
/**
 * Options for a new signal. `replyTo`: the signal it answers. `cause`: the
 * signal that led to it. With either, it joins that signal's trace; with
 * neither, it starts a new trace.
 */
export type SignalOptions = {
    to?: string[];
    replyTo?: Signal;
    cause?: Signal;
};
export declare const text: (value: string) => Payload;
export declare const json: (data: unknown) => Payload;
/** JSON if the string parses, otherwise text. */
export declare function parsePayload(raw: string): Payload;
/** The wire form of a payload: the text itself, or the JSON stringified. */
export declare const serialize: (payload: Payload) => string;
export declare function newSignal(from: string, payload: Payload, { to, replyTo, cause }?: SignalOptions): Signal;
