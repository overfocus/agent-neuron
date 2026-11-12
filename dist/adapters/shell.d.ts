import type { Adapter, AdapterContext } from '../adapter.js';
import { type Payload, type Signal } from '../signal.js';
export declare class ShellError extends Error {
    readonly exitCode: number | null;
    readonly stderr: string;
    constructor(exitCode: number | null, stderr: string);
}
/** Runs a command with the payload on stdin; stdout is the answer. No shell is involved. */
export declare class ShellAdapter implements Adapter {
    readonly command: string;
    readonly args: string[];
    constructor(command: string, args?: string[]);
    handle(payload: Payload, _signal?: Signal, context?: AdapterContext): Promise<Payload>;
}
