import { describe, expect, it } from 'vitest';
import { Network, Neuron, newSignal, text, type Adapter, type Signal } from '../src/index.js';

const echo: Adapter = { handle: async (payload) => payload };

describe('hops', () => {
  it('is 0 on a root and rises by 1 for each replyTo or cause link', () => {
    const root = newSignal('a', text('x'));
    const reply = newSignal('b', text('x'), { replyTo: root });
    const next = newSignal('a', text('x'), { cause: reply });
    const last = newSignal('b', text('x'), { replyTo: next });
    expect([root, reply, next, last].map((s) => s.hops)).toEqual([0, 1, 2, 3]);
  });

  /** a and b ping-pong: every answer is forwarded back with `cause`. Returns the chain of signals seen. */
  function pingPong(options?: { maxHops: number }) {
    const network = new Network({
      onSignal: (s) => {
        seen.push(s);
        if (s.replyTo && seen.length < 60) network.emit(s.to?.[0] as string, s.payload, { to: [s.from], cause: s });
      },
      onDrop: (s, id, reason) => drops.push([s, id, reason]),
    });
    const seen: Signal[] = [];
    const drops: [Signal, string, string][] = [];
    network.add(new Neuron('a', echo, options));
    network.add(new Neuron('b', echo, options));
    network.connect('a', 'b');
    return { network, seen, drops };
  }

  it('maxHops refuses a signal with more hops: emit reports onDrop "hops" and the adapter never runs', async () => {
    const { network, seen, drops } = pingPong({ maxHops: 3 });
    network.emit('a', text('go'));
    await network.idle();
    expect(seen.map((s) => s.hops)).toEqual([0, 1, 2, 3, 4]); // the refused signal is still announced; nothing follows it
    expect(drops.map(([s, id, reason]) => [s.hops, id, reason])).toEqual([[4, 'b', 'hops']]);
  });

  it('without maxHops a 50-hop chain is delivered in full', async () => {
    const { network, seen, drops } = pingPong();
    network.emit('a', text('go'));
    await network.idle();
    expect(seen.length).toBeGreaterThan(50);
    expect(seen.map((s) => s.hops)).toEqual(seen.map((_, i) => i));
    expect(drops).toEqual([]);
  });

  it('a waiting send leaves out the answer of a neuron that refuses; others still answer', async () => {
    const network = new Network();
    let ran = 0;
    const counted: Adapter = { handle: async (p) => (ran++, p) };
    network.add(new Neuron('a', echo));
    network.add(new Neuron('strict', counted, { maxHops: 0 }));
    network.add(new Neuron('open', echo));
    network.connect('a', 'strict');
    network.connect('a', 'open');
    const root = await network.send('a', text('x'));
    expect(root.map((s) => s.from)).toEqual(['strict', 'open']);
    const deep = await network.send('a', text('x'), { cause: root[0] as Signal });
    expect(deep.map((s) => s.from)).toEqual(['open']);
    expect(ran).toBe(1);
  });

  it('query skips the limit; the options are readable; no options is the default', async () => {
    const n = new Neuron('x', echo, { maxHops: 0 });
    expect(await n.query(text('hi'))).toEqual(text('hi'));
    expect(n.options).toEqual({ maxHops: 0 });
    expect(new Neuron('y', echo).options).toEqual({});
  });
});
