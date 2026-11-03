import { describe, expect, it, vi } from 'vitest';
import { Network, Neuron, text, type Adapter, type AdapterContext, type Signal } from '../src/index.js';

/** Senders s1..s3 and a `vote` neuron (threshold 2 unless given) that records each run's inputs. */
function setup(options: { threshold?: number; within?: number } = { threshold: 2 }) {
  const runs: Signal[][] = [];
  const drops: [string, string][] = [];
  const answers: Signal[] = [];
  const network = new Network({
    onSignal: (s) => s.from === 'vote' && answers.push(s),
    onDrop: (s, id, reason) => drops.push([`${s.from}:${(s.payload as { text: string }).text}`, reason]),
  });
  const vote: Adapter = {
    handle: async (_p, _s, context?: AdapterContext) => {
      runs.push(context?.inputs ?? []);
      return text('ok');
    },
  };
  const voter = network.add(new Neuron('vote', vote, options));
  for (const id of ['s1', 's2', 's3']) {
    network.add(new Neuron(id, vote));
    network.connect(id, 'vote');
  }
  /** An input from `from` on the trace of `of` (or a new trace). */
  const input = (from: string, t: string, of?: Signal, replaces?: string) =>
    network.emit(from, text(t), { to: ['vote'], ...(of && { cause: of }), ...(replaces && { replaces }) });
  return { voter, network, runs, drops, answers, input };
}

describe('threshold', () => {
  it('runs once, after two distinct senders on one trace, with both inputs', async () => {
    const { network, runs, answers, input } = setup();
    const first = input('s1', 'a');
    await network.idle();
    expect(runs).toEqual([]);
    const second = input('s2', 'b', first);
    await network.idle();
    expect(runs.map((r) => r.map((s) => s.from))).toEqual([['s1', 's2']]);
    expect(answers.map((a) => a.replyTo)).toEqual([second.id]); // the held input gets no answer of its own
  });

  it('counts the same sender once: its newer input replaces the older', async () => {
    const { network, runs, drops, input } = setup();
    const first = input('s1', 'old');
    input('s1', 'new', first);
    await network.idle();
    expect(runs).toEqual([]);
    expect(drops).toEqual([['s1:old', 'replaced']]);
    input('s2', 'b', first);
    await network.idle();
    expect(runs[0]?.map((s) => (s.payload as { text: string }).text)).toEqual(['new', 'b']);
  });

  it('inputs on different traces never combine', async () => {
    const { network, runs, input } = setup();
    input('s1', 'a');
    input('s2', 'b');
    await network.idle();
    expect(runs).toEqual([]);
  });

  it('drops a held input as expired after `within`, and not before', async () => {
    vi.useFakeTimers();
    try {
      const { drops, input } = setup({ threshold: 2, within: 500 });
      input('s1', 'a');
      vi.advanceTimersByTime(499);
      expect(drops).toEqual([]);
      vi.advanceTimersByTime(1);
      expect(drops).toEqual([['s1:a', 'expired']]);
    } finally {
      vi.useRealTimers();
    }
  });

  it('drops a held input as replaced when its emit is replaced, and it then no longer counts', async () => {
    const { network, runs, drops, input } = setup();
    const first = input('s1', 'a', undefined, 'k');
    input('s1', 'b', undefined, 'k'); // new trace, same key: aborts the first
    expect(drops).toEqual([['s1:a', 'replaced']]);
    input('s2', 'c', first);
    await network.idle();
    expect(runs).toEqual([]); // 'a' is gone; only s2 is held on that trace
  });

  it('idle does not wait for held inputs, and a fired group is cleared', async () => {
    const { network, runs, input } = setup({ threshold: 2, within: 60_000 });
    const first = input('s1', 'a');
    await network.idle(); // returns at once despite the 60 s hold
    input('s2', 'b', first);
    input('s3', 'c', first); // the trace already ran: a new hold, not a second run
    await network.idle();
    expect(runs).toHaveLength(1);
  });

  it('send and query skip the threshold', async () => {
    const { network, runs, voter } = setup();
    const answers = await network.send('s1', text('hi'));
    expect(answers).toHaveLength(1);
    expect(await voter.query(text('hi'))).toEqual(text('ok'));
    expect(runs).toEqual([[], []]);
  });

  it('inputs consumed at the crossing never report expired later', async () => {
    vi.useFakeTimers();
    try {
      const { network, drops, input } = setup({ threshold: 2, within: 500 });
      const first = input('s1', 'a');
      input('s2', 'b', first);
      vi.advanceTimersByTime(5000);
      await network.idle();
      expect(drops).toEqual([]);
    } finally {
      vi.useRealTimers();
    }
  });

  it('an abort after crossing drops only the crossing input, as replaced', async () => {
    const { network, runs, drops, input } = setup();
    const first = input('s1', 'a');
    input('s2', 'b', first, 'k');
    input('s2', 'c', undefined, 'k'); // aborts the crossing emit
    await network.idle();
    expect(runs).toHaveLength(1);
    expect(drops).toEqual([['s2:b', 'replaced']]);
  });

  it('an emit from inside onDrop still reaches the threshold', async () => {
    const runs: Signal[][] = [];
    let first: Signal | undefined;
    const network = new Network({
      onDrop: (s, _id, reason) => {
        if (reason === 'replaced' && s.payload.type === 'text' && s.payload.text === 'old')
          network.emit('s2', text('x'), { to: ['vote'], cause: first as Signal });
      },
    });
    network.add(new Neuron('vote', { handle: async (_p, _s, c) => (runs.push(c?.inputs ?? []), text('ok')) }, { threshold: 2 }));
    for (const id of ['s1', 's2']) {
      network.add(new Neuron(id, { handle: async (p) => p }));
      network.connect(id, 'vote');
    }
    first = network.emit('s1', text('old'), { to: ['vote'] });
    network.emit('s1', text('new'), { to: ['vote'], cause: first });
    await network.idle();
    expect(runs.map((r) => r.map((s) => s.from))).toEqual([['s1', 's2']]);
  });

  it('without a threshold nothing is held and no timer is made', async () => {
    const spy = vi.spyOn(globalThis, 'setTimeout');
    const { network, runs, input } = setup({});
    input('s1', 'a');
    await network.idle();
    expect(runs).toHaveLength(1);
    expect(spy).not.toHaveBeenCalled();
    spy.mockRestore();
  });
});
