import type { Desk, Host } from './engine.js';
type Reply = Record<string, unknown>;
type Entry = {
    when?: unknown;
    after?: number;
    reply: Reply;
    ignoreTerm?: boolean;
    leaves?: boolean;
    onCancel?: {
        after?: number;
        reply: Reply;
    };
};
type Start = {
    ready?: number | false;
    maxInflight?: number;
    first?: unknown;
    readyV?: number;
    pong?: number | false;
    end?: {
        after?: number;
        how?: 'exit' | 'closeStdout' | 'error';
    };
    ignoreTerm?: boolean;
    leaves?: boolean;
    stderr?: string;
};
export type Script = Record<string, {
    calls?: Entry[];
    starts?: Start[];
}>;
export declare class VirtualHost implements Host {
    private script;
    t: number;
    private seq;
    private timers;
    private procs;
    private pids;
    private used;
    private starts;
    constructor(script: Script);
    now: () => number;
    setTimeout: (fn: () => void, ms: number) => NodeJS.Timeout;
    clearTimeout: (t: NodeJS.Timeout | undefined) => void;
    setInterval: (fn: () => void, ms: number) => NodeJS.Timeout;
    clearInterval: (t: NodeJS.Timeout | undefined) => void;
    /** Runs the next wake-up: the earliest, and among equals the one set first (SPEC 6.2 rule 1). */
    step: () => boolean;
    launch: (node: {
        spec: {
            run?: string[];
            serve?: string[];
        };
        path: string;
    }) => never;
    killGroup: (pid: number | undefined, signal: NodeJS.Signals) => void | undefined;
    alive: (pid: number | undefined) => boolean;
    /**
     * The scripted person (CONFORMANCE 5.5): the desk a virtual run gives the engine for `human` neurons. Each question takes the
     * first unused `calls` entry of its instance path whose `when` equals the input, like a program; the reply comes `after` ms
     * later on the virtual clock, set when the question is asked, so it goes before the call's timeout at the same instant.
     * `{"answer": "text"}` replies with the text, `{"quiet": true}` with nothing (No), `{"hang": true}` never. A withdrawn
     * question's pending reply is cancelled.
     */
    desk(): Desk;
    /** The first unused entry whose `when` equals the program's input (none: matches anything). A miss is the fixture's fault. */
    take(path: string, input: unknown): Entry;
}
export {};
