import { request, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import * as lib from '../src/index.js';
import { INNER_IDS } from '../examples/alerts-demo.mjs';
import { startViewServer } from '../examples/view/server.mjs';

describe('network view server', () => {
  let server: Server;
  let base: string;
  beforeAll(async () => {
    server = await startViewServer(lib);
    base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  });
  afterAll(() => void server.close());

  it('serves the page, refuses unknown demos and foreign host names', async () => {
    expect((await fetch(`${base}/`)).status).toBe(200);
    expect((await fetch(`${base}/events?demo=nope`)).status).toBe(404);
    const status = await new Promise((resolve) => {
      const req = request(`${base}/`, { headers: { Host: 'example.test' } }, (res) => (res.resume(), resolve(res.statusCode)));
      req.end();
    });
    expect(status).toBe(403);
    for (const headers of [{ 'Sec-Fetch-Site': 'cross-site' }, { 'Sec-Fetch-Site': 'same-site' }, { Origin: 'http://example.test' }]) {
      expect((await fetch(`${base}/events?demo=typing`, { headers })).status).toBe(403);
    }
  });

  it('streams the alerts run: diagnosis is one node and its inner neurons never appear', async () => {
    const body = await (await fetch(`${base}/events?demo=alerts`)).text();
    const events = body.split('\n\n').filter(Boolean).map((chunk) => JSON.parse(chunk.replace(/^data: /, '')));
    expect(events[0]).toMatchObject({ type: 'start', demo: 'alerts' });
    const first = events.find((e) => e.type === 'signal');
    expect(first.text).toMatch(/^id: 1, at: \d+, service: /); // an object reads as key: value pairs
    expect(events.at(-1)).toMatchObject({ type: 'done' });
    const seen = new Set<string>();
    for (const e of events) {
      if (['signal', 'answer', 'quiet'].includes(e.type)) [e.from, ...e.to].forEach((id) => seen.add(id));
      if (e.type === 'drop' || e.type === 'error') seen.add(e.neuron);
    }
    expect(seen.has('diagnosis')).toBe(true);
    expect(INNER_IDS.filter((id) => seen.has(id))).toEqual([]);
    expect(events.some((e) => e.type === 'drop' && e.reason === 'replaced')).toBe(true);
  }, 30_000);
});
