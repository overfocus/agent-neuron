// The people page (decision 0008): the desk `engine run` gives the engine for `human` neurons (SPEC 3.4). One page per run, on
// 127.0.0.1 only; each question is an item a person answers with Approve, Edit or No.
//
// The gate, before any route: 127.0.0.1 only; a request target that does not parse is a 400; `Host` must be exactly ours (no DNS
// rebinding); an `Origin`, if sent, must be exactly ours, and an answer must send one; `Sec-Fetch-Site: cross-site` is refused; the
// 128-bit token (query `t` for the page and its fonts, the `x-neuron-token` header for the API) is compared in constant time; methods
// are allowed per route; an answer's body is JSON, at most MAX_BODY bytes. Every response carries nosniff, no-referrer and no-store;
// the page has a nonce CSP and fetches nothing from anywhere but its own origin. Errors never echo the token or the request.
import { randomBytes, timingSafeEqual } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import type { Desk, Question } from './engine.js';
import { PAGE } from './page-html.js';

const MAX_BODY = 128 * 1024;
const FONTS: Record<string, string> = { 'josefin.ttf': 'JosefinSans-wght.ttf', 'pixelify.ttf': 'PixelifySans-wght.ttf' };
const FONT_DIR = new URL('../../assets/fonts/', import.meta.url);

/** One question on the page, and what became of it. */
export type Item = Question & { state: 'waiting' | 'answered' | 'withdrawn'; kind?: 'approve' | 'edit' | 'no'; answer?: string };

/**
 * What Approve sends (decision 0008, SPEC Appendix D): a payload's `proposal` when it is text, the payload itself when it is text,
 * and otherwise the payload as JSON text.
 */
export function approval(payload: unknown): string {
  if (typeof payload === 'string') return payload;
  const p = payload as { proposal?: unknown } | null;
  if (p && typeof p === 'object' && typeof p.proposal === 'string') return p.proposal;
  return JSON.stringify(payload, null, 2);
}

const same = (a: string | undefined, b: string) => typeof a === 'string' && a.length === b.length && timingSafeEqual(Buffer.from(a), Buffer.from(b));

/** Starts the page. Resolves the desk to give `runNetwork`, the address to show the person, and `close`. */
export async function openPage(options: { title?: string; port?: number; token?: string } = {}): Promise<{ desk: Desk; url: string; token: string; items: () => Item[]; close: () => Promise<void> }> {
  const token = options.token ?? randomBytes(16).toString('hex');
  const items = new Map<string, Item>();
  const answers = new Map<string, (text: string) => void>();
  const desk: Desk = {
    ask(q, answer) {
      items.set(q.id, { ...q, state: 'waiting' });
      answers.set(q.id, answer);
    },
    withdraw(id) {
      const it = items.get(id);
      if (it?.state === 'waiting') it.state = 'withdrawn';
      answers.delete(id);
    },
  };
  let origin = '';
  const headers = { 'x-content-type-options': 'nosniff', 'referrer-policy': 'no-referrer', 'cache-control': 'no-store' };
  const send = (res: ServerResponse, code: number, type: string, body: string | Buffer, extra: Record<string, string> = {}) => {
    res.writeHead(code, { ...headers, 'content-type': type, ...extra });
    res.end(body);
  };
  const json = (res: ServerResponse, code: number, value: unknown) => send(res, code, 'application/json; charset=utf-8', JSON.stringify(value));
  const text = (res: ServerResponse, code: number, msg: string) => send(res, code, 'text/plain; charset=utf-8', `${msg}\n`);

  const readBody = (req: IncomingMessage) =>
    new Promise<string | null>((resolve) => {
      const chunks: Buffer[] = [];
      let n = 0;
      req.on('data', (c: Buffer) => {
        n += c.length;
        if (n > MAX_BODY) chunks.length = 0;
        else chunks.push(c);
      });
      req.on('end', () => resolve(n > MAX_BODY ? null : Buffer.concat(chunks).toString('utf8')));
      req.on('error', () => resolve(null));
    });

  const server = createServer(async (req, res) => {
    let url: URL;
    try { url = new URL(req.url ?? '/', origin); } catch { return text(res, 400, 'bad request'); }
    const h = req.headers;
    if (h.host !== origin.slice(7) && h.host !== `localhost:${origin.split(':')[2]}`) return text(res, 421, 'wrong host');
    if (h.origin !== undefined && h.origin !== origin && h.origin !== `http://localhost:${origin.split(':')[2]}`) return text(res, 403, 'cross-origin request refused');
    if (h['sec-fetch-site'] === 'cross-site') return text(res, 403, 'cross-site request refused');
    const header = Array.isArray(h['x-neuron-token']) ? h['x-neuron-token'][0] : h['x-neuron-token'];
    const authed = same(url.searchParams.get('t') ?? header, token);
    const route = url.pathname;

    if (route === '/') {
      if (req.method !== 'GET') return text(res, 405, 'GET only');
      if (!authed) return text(res, 403, 'forbidden: open the address the engine printed');
      const nonce = randomBytes(16).toString('hex');
      const page = PAGE.replaceAll('__NONCE__', nonce).replaceAll('__TOKEN__', token).replace('__TITLE__', JSON.stringify(options.title ?? 'a run').replace(/</g, '\\u003c'));
      return send(res, 200, 'text/html; charset=utf-8', page, {
        'content-security-policy': `default-src 'none'; script-src 'nonce-${nonce}'; style-src 'nonce-${nonce}'; font-src 'self'; connect-src 'self'; img-src data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'`,
        'x-frame-options': 'DENY',
      });
    }
    if (route.startsWith('/fonts/') && Object.hasOwn(FONTS, route.slice(7))) {
      if (req.method !== 'GET') return text(res, 405, 'GET only');
      if (!authed) return text(res, 403, 'forbidden');
      return send(res, 200, 'font/ttf', readFileSync(new URL(FONTS[route.slice(7)]!, FONT_DIR)));
    }
    if (route === '/api/items') {
      if (req.method !== 'GET') return json(res, 405, { error: 'GET only' });
      if (!header || !same(header, token)) return json(res, 403, { error: 'forbidden' });
      return json(res, 200, { items: [...items.values()] });
    }
    if (route === '/api/answer') {
      if (req.method !== 'POST') return json(res, 405, { error: 'POST only' });
      if (h.origin === undefined) return json(res, 403, { error: 'an answer needs this page\'s Origin' });
      if (!header || !same(header, token)) return json(res, 403, { error: 'forbidden' });
      if (!String(h['content-type'] ?? '').startsWith('application/json')) return json(res, 415, { error: 'JSON only' });
      const raw = await readBody(req);
      if (raw === null) return json(res, 413, { error: `an answer is at most ${MAX_BODY} bytes` });
      let body: { id?: unknown; kind?: unknown; text?: unknown };
      try { body = JSON.parse(raw) as typeof body; } catch { return json(res, 400, { error: 'not JSON' }); }
      const it = typeof body.id === 'string' ? items.get(body.id) : undefined;
      if (!it) return json(res, 404, { error: 'no such question' });
      if (it.state !== 'waiting') return json(res, 409, { error: it.state === 'withdrawn' ? 'this question was withdrawn: a newer version arrived, the time ran out, or the run was cut' : 'already answered' });
      let reply: string;
      if (body.kind === 'approve') reply = approval(it.payload);
      else if (body.kind === 'no') reply = '';
      else if (body.kind === 'edit' && typeof body.text === 'string' && body.text.trim()) reply = body.text;
      else return json(res, 400, { error: 'kind is approve, edit (with non-empty text) or no' });
      it.state = 'answered';
      it.kind = body.kind;
      it.answer = reply;
      answers.get(it.id)?.(reply);
      answers.delete(it.id);
      return json(res, 200, { item: it });
    }
    return text(res, 404, 'not found');
  });

  await new Promise<void>((resolve, reject) => {
    server.once('error', reject);
    server.listen(options.port ?? 0, '127.0.0.1', () => resolve());
  });
  const port = (server.address() as { port: number }).port;
  origin = `http://127.0.0.1:${port}`;
  return {
    desk,
    url: `${origin}/?t=${token}`,
    token,
    items: () => [...items.values()],
    close: () => new Promise<void>((resolve) => {
      server.closeAllConnections();
      server.close(() => resolve());
    }),
  };
}
