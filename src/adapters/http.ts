import type { Adapter, AdapterContext } from '../adapter.js';
import { json, text, type Payload, type Signal } from '../signal.js';

export class HttpError extends Error {
  constructor(readonly status: number) {
    super(`request failed with status ${status}`);
    this.name = 'HttpError';
  }
}

/** POSTs the payload to a URL; the response body is the answer. */
export class HttpAdapter implements Adapter {
  constructor(readonly url: string) {}

  async handle(payload: Payload, _signal?: Signal, context?: AdapterContext): Promise<Payload> {
    const res = await fetch(this.url, {
      method: 'POST',
      headers: { 'content-type': payload.type === 'json' ? 'application/json' : 'text/plain' },
      body: payload.type === 'json' ? JSON.stringify(payload.data) : payload.text,
      ...(context && { signal: context.abort }),
    });
    if (!res.ok) throw new HttpError(res.status);
    const isJson = (res.headers.get('content-type') ?? '').includes('application/json');
    return isJson ? json(await res.json()) : text(await res.text());
  }
}
