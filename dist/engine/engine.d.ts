import { type ChildProcessWithoutNullStreams as Proc } from 'node:child_process';
import { type NeuronSpec } from './check.js';
export type Units = Record<string, number>;
export type Injection = {
    at: number;
    source: string;
    payload: unknown;
    replaces?: string;
    budget?: Units;
};
export type Event = {
    type: string;
    t: number;
    [field: string]: unknown;
};
export type Trace = {
    format: 'agent-neuron-trace';
    version: 1;
    demo: string;
    description: string;
    library: string;
    engine: string;
    budget?: Units;
    neurons: {
        id: string;
        kind: string;
        description: string;
        options?: Record<string, number>;
        cost?: Units;
    }[];
    links: {
        from: string;
        to: string;
    }[];
    events: Event[];
    summary: Record<string, unknown>;
};
/**
 * A question to a person (SPEC 3.4): a `human` neuron's input, and where it comes from. `id` is the crossing signal's id.
 * `t` is the engine's clock (ms since the run started, as in the trace).
 */
export type Question = {
    id: string;
    neuron: string;
    path: string;
    description: string;
    payload: unknown;
    t: number;
};
/**
 * Where a `human` neuron's questions go: the command line's page (`./page.js`), or a test's scripted person. `answer` ends the call: an
 * empty string is quiet, any other string is the text payload it replies with (SPEC 3.4); a second answer, or one after `withdraw`, is
 * ignored. `withdraw` says the call ended without an answer (stale, timed out, or the run was cut), so the question can be taken down.
 */
export interface Desk {
    ask(q: Question, answer: (text: string) => void): void;
    withdraw(id: string): void;
}
/** Time and processes: the seam the conformance fixtures use (SPEC 8). `realHost` is what runs; `src/engine/virtual.ts` is the other. */
export interface Host {
    now(): number;
    setTimeout(fn: () => void, ms: number): NodeJS.Timeout;
    clearTimeout(t: NodeJS.Timeout | undefined): void;
    setInterval(fn: () => void, ms: number): NodeJS.Timeout;
    clearInterval(t: NodeJS.Timeout | undefined): void;
    launch(node: {
        spec: NeuronSpec;
        dir: string;
        path: string;
    }): Proc;
    killGroup(pid: number | undefined, signal: NodeJS.Signals): void;
    alive(pid: number | undefined): boolean;
    /** Virtual only: runs the next due wake-up (same instant: the order they were set); false when none is left. */
    step?(): boolean;
}
/** Signals every running program's process group: for a CLI that is itself being stopped. */
export declare function killGroups(signal: NodeJS.Signals): void;
/** Reads injections, one JSON object per line: `at` (ms), `source`, `payload`, optional `replaces`. Throws on a bad line. */
export declare function readInjections(file: string): Injection[];
/**
 * Runs the network in `file` with the producer's `injections` and returns its trace.
 * Throws if the file does not check, uses a kind this engine lacks, or an injection names no source.
 * `inner` receives the events inside composed neurons (by instance path): an operator's tool, never in the trace.
 * `pingEvery` (ms, default 10000) is how often an idle serve neuron is pinged.
 * `result` receives the full answers of the neurons with no outgoing links (the trace cuts long payloads).
 * `host` replaces time and processes (a virtual clock and scripted programs, SPEC 8); one virtual run at a time.
 * `observer` is called with each outer event; the injections it returns (`at` is a delay in ms) enter later (SPEC 9).
 * `onEvent` is the live stream (SPEC 9.1): each event of the trace, in written order, as it happens; its failure never touches the run.
 * `desk` takes the questions of `human` neurons (SPEC 3.4) and answers them; without one, a file with a `human` neuron is refused.
 */
export type Observer = (e: Event) => Injection[] | void;
export declare function runNetwork(file: string, injections: Injection[], options?: {
    pingEvery?: number;
    inner?: (path: string, e: Event) => void;
    result?: (neuron: string, t: number, payload: unknown) => void;
    host?: Host;
    observer?: Observer;
    onEvent?: (e: Event) => unknown;
    desk?: Desk;
}): Promise<Trace>;
/** The trace as text, one neuron, link and event per line (as the recorder writes it). */
export declare const formatTrace: (t: Trace) => string;
