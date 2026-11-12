import { runNetwork, type Event, type Injection } from './engine.js';
import { type Script } from './virtual.js';
type Rule = {
    when: Record<string, unknown>;
    inject: {
        after: number;
        source: string;
        payload: unknown;
        replaces?: string;
    };
};
export type Fixture = {
    fixture: 0;
    description: string;
    network?: string;
    notes?: boolean;
    finished?: boolean;
    streamFails?: boolean;
    pending?: string;
    pingEvery?: number;
    refuses?: boolean;
    inputs: Injection[];
    script?: Script;
    observer?: Rule[];
    expect: Event[];
    expectWithout?: Event[];
    independent?: boolean;
};
export type Result = {
    name: string;
    status: 'pass' | 'fail' | 'pending';
    detail: string[];
};
type Engine = typeof runNetwork;
/** Ids become #1, #2, ... by first appearance, scanning id, trace, replyTo, cause (and a note's trigger) of each event in turn (8.3). */
export declare function normalise(events: Event[]): Event[];
/** The first difference between expected and actual events (notes and `finished` events only if asked), or undefined. `via` absent equals "emit"; an error's or unknown's message is a prefix. */
export declare function compare(expected: Event[], actual: Event[], notes: boolean, finished?: boolean): string | undefined;
export declare const fixtureDirs: (dir: string) => string[];
/** The events one run of the fixture in `dir` produces (all of them, notes included). The live stream of the run (SPEC 9.1) must equal them, event for event:
 *  the consumer here keeps a copy, then scribbles on what it was given and, with `streamFails`, throws, so an engine that lets the consumer touch the run fails. */
export declare function events(engine: Engine, dir: string, f: Fixture, withObserver: boolean): Promise<Event[]>;
/** Runs the fixtures under `dir` (those whose path contains `only`, if given). */
export declare function conform(dir: string, options?: {
    only?: string;
    engine?: Engine;
    bail?: boolean;
}): Promise<Result[]>;
/** The lines the CLI prints: `pass NAME`, `fail NAME` with indented detail, `pending NAME: reason`, then a summary. */
export declare function report(results: Result[]): string[];
export {};
