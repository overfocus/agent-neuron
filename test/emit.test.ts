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
  serialize,
  text,
  type Adapter,
  type Signal,
} from '../src/index.js';

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Answers `reply` after `ms`, unless aborted first (then rejects). */
function slow(ms: number, reply: string, seen: AbortSignal[] = []): Adapter {
  return {
    handle: (payload, _signal, context) =>
      new Promise((resolve, reject) => {
        if (context) seen.push(context.abort);
        const timer = setTimeout(() => resolve(text(`${reply}:${serialize(payload)}`)), ms);
        context?.abort.addEventListener('abort', () => {
          clearTimeout(timer);
          reject(context.abort.reason);
        });
      }),
  };
}

function setup(adapter: Adapter) {
  const answers: Signal[] = [];
  const errors: unknown[] = [];
  const drops: Signal[] = [];
  const net = new Network({
    onSignal: (s) => s.from === 'box' || answers.push(s),
    onError: (e) => errors.push(e),
    onDrop: (s) => drops.push(s),
  });
  net.add(new Neuron('box', slow(0, 'unused')));
  net.add(new Neuron('n', adapter));
  net.connect('box', 'n');
  return { net, answers, errors, drops };
}

describe('emit', () => {
  it('returns before a slow neuron answers; the answer arrives later', async () => {
    const { net, answers } = setup(slow(50, 'ok'));
    const sent = net.emit('box', text('hi'));
    expect(sent).toMatchObject({ from: 'box', to: ['n'] });
    expect(answers).toEqual([]);
    await net.idle();
    expect(answers.map((a) => [a.from, serialize(a.payload)])).toEqual([['n', 'ok:hi']]);
  });

  it('reports a failing neuron through onError', async () => {
    const { net, answers, errors } = setup({ handle: () => Promise.reject(new Error('boom')) });
    net.emit('box', text('hi'));
    await net.idle();
    expect(answers).toEqual([]);
    expect(errors).toEqual([new Error('boom')]);
  });

  it('a newer emit with the same key aborts the older work and only the newer one answers', async () => {
    const seen: AbortSignal[] = [];
    const { net, answers, errors, drops } = setup(slow(50, 'ok', seen));
    const first = net.emit('box', text('a'), { replaces: 'q' });
    await sleep(10);
    expect(seen[0]?.aborted).toBe(false);
    net.emit('box', text('ab'), { replaces: 'q' });
    expect(seen[0]?.aborted).toBe(true);
    await net.idle();
    expect(answers.map((a) => serialize(a.payload))).toEqual(['ok:ab']);
    expect(errors).toEqual([]);
    expect(drops).toEqual([first]);
  });

  it('drops the answer of an adapter that ignores the abort signal', async () => {
    const { net, answers, drops } = setup({
      handle: async (p) => {
        await sleep(30);
        return p;
      },
    });
    net.emit('box', text('a'), { replaces: 'q' });
    net.emit('box', text('ab'), { replaces: 'q' });
    await net.idle();
    expect(answers.map((a) => serialize(a.payload))).toEqual(['ab']);
    expect(drops).toHaveLength(1);
  });

  it('does not replace work under a different key', async () => {
    const { net, answers } = setup(slow(20, 'ok'));
    net.emit('box', text('a'), { replaces: 'x' });
    net.emit('box', text('b'), { replaces: 'y' });
    net.emit('box', text('c'));
    await net.idle();
    expect(answers).toHaveLength(3);
  });
});

describe('listeners that throw', () => {
  async function run(listeners: ConstructorParameters<typeof Network>[0], adapter: Adapter) {
    const unhandled: unknown[] = [];
    const record = (e: unknown) => unhandled.push(e);
    process.on('unhandledRejection', record);
    const net = new Network(listeners);
    net.add(new Neuron('box', slow(0, 'unused')));
    net.add(new Neuron('n', adapter));
    net.connect('box', 'n');
    net.emit('box', text('x'), { replaces: 'q' });
    if (adapter === dropped) net.emit('box', text('y'), { replaces: 'q' });
    await net.idle();
    await sleep(10); // let any unhandled rejection surface
    process.off('unhandledRejection', record);
    return unhandled;
  }
  const dropped = slow(30, 'ok');

  it('onSignal throwing is not the neuron\'s error', async () => {
    const errors: unknown[] = [];
    const unhandled = await run(
      { onSignal: (s) => { if (s.from === 'n') throw new Error('listener bug'); }, onError: (e) => errors.push(e) },
      slow(1, 'ok'),
    );
    expect(errors).toEqual([]);
    expect(unhandled).toEqual([]);
  });

  it('onError throwing is not an unhandled rejection', async () => {
    const unhandled = await run(
      { onError: () => { throw new Error('listener bug'); } },
      { handle: () => Promise.reject(new Error('boom')) },
    );
    expect(unhandled).toEqual([]);
  });

  it('onDrop throwing is not an unhandled rejection, nor called twice', async () => {
    let calls = 0;
    const unhandled = await run({ onDrop: () => { calls++; throw new Error('listener bug'); } }, dropped);
    expect(unhandled).toEqual([]);
    expect(calls).toBe(1);
  });
});

describe('send', () => {
  it('still waits for every answer', async () => {
    const { net } = setup(slow(10, 'ok'));
    const answers = await net.send('box', text('hi'));
    expect(answers.map((a) => serialize(a.payload))).toEqual(['ok:hi']);
  });
});

describe('adapters stop on abort', () => {
  const abortSoon = () => {
    const controller = new AbortController();
    setTimeout(() => controller.abort(), 20);
    return controller.signal;
  };
  const signal = { id: 'x', from: 'a', payload: text('x') };

  it('shell kills the command', async () => {
    const started = Date.now();
    const err = await new ShellAdapter('sleep', ['5'])
      .handle(text('x'), signal, { abort: abortSoon() })
      .catch((e: unknown) => e);
    expect((err as Error).name).toBe('AbortError');
    expect(Date.now() - started).toBeLessThan(1000);
  });

  describe('http', () => {
    let server: Server;
    afterEach(() => {
      server.closeAllConnections();
      return new Promise((done) => server.close(done));
    });

    it('aborts a request that never gets a response', async () => {
      server = createServer(() => {});
      await new Promise<void>((ok) => server.listen(0, '127.0.0.1', ok));
      const url = `http://127.0.0.1:${(server.address() as AddressInfo).port}/`;
      const err = await new HttpAdapter(url)
        .handle(text('x'), signal, { abort: abortSoon() })
        .catch((e: unknown) => e);
      expect((err as Error).name).toBe('AbortError');
    });
  });

  it('human stops waiting', async () => {
    const input = new PassThrough();
    const err = await new HumanAdapter(input, new PassThrough())
      .handle(text('ready?'), signal, { abort: abortSoon() })
      .catch((e: unknown) => e);
    expect((err as Error).name).toBe('AbortError');
  });
});
