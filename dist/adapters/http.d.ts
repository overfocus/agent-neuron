import type { Adapter, AdapterContext } from '../adapter.js';
import { type Payload, type Signal } from '../signal.js';
export declare class HttpError extends Error {
    readonly status: number;
    constructor(status: number);
}
/** POSTs the payload to a URL; the response body is the answer. */
export declare class HttpAdapter implements Adapter {
    readonly url: string;
    constructor(url: string);
    handle(payload: Payload, _signal?: Signal, context?: AdapterContext): Promise<Payload>;
}
