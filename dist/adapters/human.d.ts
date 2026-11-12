import type { Readable, Writable } from 'node:stream';
import type { Adapter, AdapterContext } from '../adapter.js';
import { type Payload, type Signal } from '../signal.js';
/** Shows the payload as a question and answers with the next line typed. */
export declare class HumanAdapter implements Adapter {
    readonly input: Readable;
    readonly output: Writable;
    constructor(input?: Readable, output?: Writable);
    handle(payload: Payload, _signal?: Signal, context?: AdapterContext): Promise<Payload>;
}
