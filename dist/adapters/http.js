import { json, text } from '../signal.js';
export class HttpError extends Error {
    status;
    constructor(status) {
        super(`request failed with status ${status}`);
        this.status = status;
        this.name = 'HttpError';
    }
}
/** POSTs the payload to a URL; the response body is the answer. */
export class HttpAdapter {
    url;
    constructor(url) {
        this.url = url;
    }
    async handle(payload, _signal, context) {
        const res = await fetch(this.url, {
            method: 'POST',
            headers: { 'content-type': payload.type === 'json' ? 'application/json' : 'text/plain' },
            body: payload.type === 'json' ? JSON.stringify(payload.data) : payload.text,
            ...(context && { signal: context.abort }),
        });
        if (!res.ok)
            throw new HttpError(res.status);
        const isJson = (res.headers.get('content-type') ?? '').includes('application/json');
        return isJson ? json(await res.json()) : text(await res.text());
    }
}
//# sourceMappingURL=http.js.map