// The people page (decision 0008): the desk `engine run` gives `human` neurons. Its three answers, its gate, and what it serves.
import http from 'node:http';
import { afterEach, describe, expect, it } from 'vitest';
import type { Question } from '../src/engine/index.js';
import { approval, openPage } from '../src/engine/page.js';

type Res = { status: number; body: string; headers: http.IncomingHttpHeaders };
const request = (port: number, o: { method?: string; path: string; host?: string; headers?: Record<string, string>; body?: string }) =>
  new Promise<Res>((resolve, reject) => {
    const r = http.request({ host: '127.0.0.1', port, method: o.method ?? 'GET', path: o.path, headers: { host: o.host ?? `127.0.0.1:${port}`, ...o.headers } }, (res) => {
      let body = '';
      res.on('data', (c) => (body += c));
      res.on('end', () => resolve({ status: res.statusCode!, body, headers: res.headers }));
    });
    r.on('error', reject);
    r.end(o.body);
  });

const q = (id: string, payload: unknown): Question => ({ id, neuron: 'reviewer', path: 'reviewer', description: 'Approve, edit or no.', payload, t: 0 });
let close: (() => Promise<void>) | undefined;
afterEach(async () => { await close?.(); close = undefined; });

async function page() {
  const p = await openPage({ title: 'voice.network.json' });
  close = p.close;
  const port = Number(new URL(p.url).port);
  const answers: Record<string, string> = {};
  const ask = (id: string, payload: unknown) => p.desk.ask(q(id, payload), (text) => (answers[id] = text));
  const api = (body: object, headers: Record<string, string> = {}) =>
    request(port, { method: 'POST', path: '/api/answer', headers: { origin: `http://127.0.0.1:${port}`, 'x-neuron-token': p.token, 'content-type': 'application/json', ...headers }, body: JSON.stringify(body) });
  return { ...p, port, answers, ask, api };
}

describe('the three answers', () => {
  it('Approve sends the proposal, Edit sends the person\'s text, No sends empty (quiet)', async () => {
    const p = await page();
    p.ask('a', { draft: 'The tool reads files.', proposal: 'rigswig reads every settings file.' });
    p.ask('b', 'Publish the release notes?');
    p.ask('c', { draft: 'x', proposal: 'y' });
    expect((await p.api({ id: 'a', kind: 'approve' })).status).toBe(200);
    expect((await p.api({ id: 'b', kind: 'edit', text: 'Publish them on Monday.' })).status).toBe(200);
    expect((await p.api({ id: 'c', kind: 'no' })).status).toBe(200);
    expect(p.answers).toEqual({ a: 'rigswig reads every settings file.', b: 'Publish them on Monday.', c: '' });
    expect(p.items().map((i) => [i.id, i.state, i.kind])).toEqual([['a', 'answered', 'approve'], ['b', 'answered', 'edit'], ['c', 'answered', 'no']]);
  });

  it('refuses a second answer, an answer to a withdrawn question, an empty edit and an unknown kind', async () => {
    const p = await page();
    p.ask('a', 'x');
    p.ask('b', 'y');
    await p.api({ id: 'a', kind: 'approve' });
    expect((await p.api({ id: 'a', kind: 'no' })).status).toBe(409);
    p.desk.withdraw('b');
    const w = await p.api({ id: 'b', kind: 'approve' });
    expect(w.status).toBe(409);
    expect(w.body).toMatch(/withdrawn/);
    p.ask('c', 'z');
    expect((await p.api({ id: 'c', kind: 'edit', text: '   ' })).status).toBe(400);
    expect((await p.api({ id: 'c', kind: 'maybe' })).status).toBe(400);
    expect((await p.api({ id: 'nope', kind: 'no' })).status).toBe(404);
    expect(p.answers).toEqual({ a: 'x' });
  });

  it('approval: a text proposal, else the text, else the payload as JSON text', () => {
    expect(approval({ draft: 'a', proposal: 'b' })).toBe('b');
    expect(approval('just text')).toBe('just text');
    expect(approval({ n: 1 })).toBe('{\n  "n": 1\n}');
  });
});

describe('the gate', () => {
  it('serves the page only with the token, the exact Host and no cross-site request, with a nonce CSP and no outside source', async () => {
    const p = await page();
    expect((await request(p.port, { path: '/' })).status).toBe(403);
    expect((await request(p.port, { path: `/?t=${p.token.replace(/.$/, (c) => (c === '0' ? '1' : '0'))}` })).status).toBe(403);
    expect((await request(p.port, { path: `/?t=${p.token}`, host: `evil.example:${p.port}` })).status).toBe(421);
    expect((await request(p.port, { path: `/?t=${p.token}`, headers: { 'sec-fetch-site': 'cross-site' } })).status).toBe(403);
    expect((await request(p.port, { path: `/?t=${p.token}`, headers: { origin: 'http://evil.example' } })).status).toBe(403);
    expect((await request(p.port, { method: 'POST', path: `/?t=${p.token}` })).status).toBe(405);
    const ok = await request(p.port, { path: `/?t=${p.token}` });
    expect(ok.status).toBe(200);
    const csp = String(ok.headers['content-security-policy']);
    const nonce = /script-src 'nonce-([0-9a-f]{32})'/.exec(csp)![1]!;
    expect(csp).toMatch(/^default-src 'none';/);
    expect(csp).not.toMatch(/unsafe|https?:/);
    expect(ok.body).toContain(`<script nonce="${nonce}">`);
    expect(ok.body).not.toMatch(/__NONCE__|__TOKEN__|__TITLE__|innerHTML/);
    expect(ok.headers['x-content-type-options']).toBe('nosniff');
  });

  it('answers need the token in the header, this Origin, JSON and a capped body; the API answers nothing without the token', async () => {
    const p = await page();
    p.ask('a', 'x');
    expect((await request(p.port, { path: '/api/items' })).status).toBe(403);
    expect((await request(p.port, { path: `/api/items?t=${p.token}` })).status).toBe(403); // the API takes the header only
    const items = await request(p.port, { path: '/api/items', headers: { 'x-neuron-token': p.token } });
    expect(JSON.parse(items.body).items).toHaveLength(1);
    expect((await p.api({ id: 'a', kind: 'approve' }, { origin: '' })).status).toBe(403);
    expect((await p.api({ id: 'a', kind: 'approve' }, { origin: 'http://evil.example' })).status).toBe(403);
    expect((await p.api({ id: 'a', kind: 'approve' }, { 'x-neuron-token': 'x'.repeat(32) })).status).toBe(403);
    expect((await p.api({ id: 'a', kind: 'approve' }, { 'content-type': 'text/plain' })).status).toBe(415);
    expect((await p.api({ id: 'a', kind: 'edit', text: 'x'.repeat(200 * 1024) })).status).toBe(413);
    expect(p.answers).toEqual({});
  });

  it('serves the two fonts with the token, and nothing else under /fonts/', async () => {
    const p = await page();
    const font = await request(p.port, { path: `/fonts/pixelify.ttf?t=${p.token}` });
    expect(font.status).toBe(200);
    expect(font.headers['content-type']).toBe('font/ttf');
    expect((await request(p.port, { path: '/fonts/pixelify.ttf' })).status).toBe(403);
    expect((await request(p.port, { path: `/fonts/..%2Fpage.ts?t=${p.token}` })).status).toBe(404);
    expect((await request(p.port, { path: `/fonts/README.md?t=${p.token}` })).status).toBe(404);
  });
});
