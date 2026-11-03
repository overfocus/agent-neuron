import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { PassThrough } from 'node:stream';
import { afterEach, describe, expect, it } from 'vitest';
import {
  HttpAdapter,
  HumanAdapter,
  Network,
  Neuron,
  ShellAdapter,
  ShellError,
  json,
  parsePayload,
  text,
  type Adapter,
  type Signal,
} from '../src/index.js';

describe('shell neuron', () => {
  it('feeds a json signal to stdin and returns stdout as json', async () => {
    const n = new Neuron('s', new ShellAdapter('cat'));
    expect(await n.query(json({ a: 1 }))).toEqual(json({ a: 1 }));
  });

  it('returns text when stdout is not JSON', async () => {
    const n = new Neuron('s', new ShellAdapter('sh', ['-c', 'echo hello there']));
    expect(await n.query(text('x'))).toEqual(text('hello there'));
  });

  it('rejects with the exit code', async () => {
    const n = new Neuron('s', new ShellAdapter('sh', ['-c', 'echo oops >&2; exit 3']));
    const err = await n.query(text('x')).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ShellError);
    expect(err).toMatchObject({ exitCode: 3, stderr: 'oops\n' });
  });
});

describe('api neuron', () => {
  let server: Server;
  afterEach(() => new Promise((done) => server.close(done)));

  async function serve(status: number, type: string, body: string): Promise<string> {
    server = createServer((_req, res) => {
      res.writeHead(status, { 'content-type': type });
      res.end(body);
    });
    await new Promise<void>((ok) => server.listen(0, '127.0.0.1', ok));
    return `http://127.0.0.1:${(server.address() as AddressInfo).port}/`;
  }

  it('returns json when the server sends JSON', async () => {
    const n = new Neuron('api', new HttpAdapter(await serve(200, 'application/json', '{"ok":true}')));
    expect(await n.query(text('hi'))).toEqual(json({ ok: true }));
  });

  it('returns text otherwise', async () => {
    const n = new Neuron('api', new HttpAdapter(await serve(200, 'text/plain', 'pong')));
    expect(await n.query(json({ ping: 1 }))).toEqual(text('pong'));
  });

  it('rejects with the status on a non-2xx response', async () => {
    const n = new Neuron('api', new HttpAdapter(await serve(500, 'text/plain', 'no')));
    await expect(n.query(text('hi'))).rejects.toMatchObject({ status: 500 });
  });
});

describe('human neuron', () => {
  it('writes the question and returns the next line as text', async () => {
    const input = new PassThrough();
    const output = new PassThrough();
    const n = new Neuron('h', new HumanAdapter(input, output));
    const answer = n.query(text('Approve?'));
    expect(String(await new Promise((r) => output.once('data', r)))).toContain('Approve?');
    input.write('yes please\n');
    expect(await answer).toEqual(text('yes please'));
  });
});

describe('network', () => {
  const echo = (tag: string): Adapter => ({ handle: async (p) => text(`${tag}:${JSON.stringify(p)}`) });
  function build() {
    const seen: Signal[] = [];
    const net = new Network({ onSignal: (s) => seen.push(s) });
    net.add(new Neuron('a', new ShellAdapter('cat')));
    net.add(new Neuron('b', echo('b')));
    net.add(new Neuron('c', echo('c')));
    net.add(new Neuron('d', echo('d')));
    net.add(new Neuron('lone', echo('lone')));
    net.connect('a', 'b');
    net.connect('a', 'c');
    net.connect('a', 'd');
    return { net, seen };
  }

  it('links are bidirectional', async () => {
    const { net } = build();
    expect((await net.send('c', text('hi'), { to: ['a'] })).map((s) => s.from)).toEqual(['a']);
    expect((await net.send('a', text('hi'), { to: ['c'] })).map((s) => s.from)).toEqual(['c']);
  });

  it('broadcasts to every linked neuron, in link order', async () => {
    const { net, seen } = build();
    net.connect('c', 'd');
    const answers = await net.send('c', text('hi'));
    expect(answers.map((s) => s.from)).toEqual(['a', 'd']);
    expect(answers[1]?.payload).toEqual(text(`d:${JSON.stringify(text('hi'))}`));
    expect(seen).toHaveLength(3); // one outgoing, two answers
  });

  it('targets one id or a set', async () => {
    const { net } = build();
    expect((await net.send('a', text('x'), { to: ['c'] })).map((s) => s.from)).toEqual(['c']);
    expect((await net.send('a', text('x'), { to: ['d', 'c'] })).map((s) => s.from)).toEqual(['d', 'c']);
  });

  it('throws when targeting an unlinked id', async () => {
    const { net } = build();
    await expect(net.send('a', text('x'), { to: ['lone'] })).rejects.toThrow(/not linked/);
  });
});

describe('one interface', () => {
  it('shell, api and human neurons share one network', async () => {
    const server = createServer((_req, res) => {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end('{"api":true}');
    });
    await new Promise<void>((ok) => server.listen(0, '127.0.0.1', ok));
    const input = new PassThrough();
    const output = new PassThrough();
    input.write('approve\n');
    const net = new Network();
    net.add(new Neuron('script', new ShellAdapter('cat')));
    net.add(new Neuron('api', new HttpAdapter(`http://127.0.0.1:${(server.address() as AddressInfo).port}/`)));
    net.add(new Neuron('human', new HumanAdapter(input, output)));
    net.connect('script', 'api');
    net.connect('script', 'human');
    const answers = await net.send('script', json({ q: 1 }));
    server.close();
    expect(answers.map((s) => [s.from, s.payload])).toEqual([
      ['api', json({ api: true })],
      ['human', text('approve')],
    ]);
  });
});

describe('parsePayload', () => {
  it('is json when it parses, text otherwise', () => {
    expect(parsePayload('{"a":1}')).toEqual(json({ a: 1 }));
    expect(parsePayload('plain')).toEqual(text('plain'));
  });
});
