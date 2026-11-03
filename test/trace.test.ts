import { describe, expect, it } from 'vitest';
import { Network, Neuron, text, type Adapter, type Signal } from '../src/index.js';

const echo: Adapter = { handle: async (payload) => payload };

function net(onSignal?: (s: Signal) => void) {
  const network = new Network(onSignal ? { onSignal } : {});
  for (const id of ['a', 'b', 'c']) network.add(new Neuron(id, echo));
  network.connect('a', 'b');
  network.connect('b', 'c');
  return network;
}

describe('replyTo and trace', () => {
  it('send answers reply to the request and share its trace', async () => {
    const seen: Signal[] = [];
    const network = net((s) => seen.push(s));
    const answers = await network.send('b', text('hi'));
    const request = seen[0] as Signal;
    expect(request.replyTo).toBeUndefined();
    expect(request.cause).toBeUndefined();
    expect(request.trace).toBe(request.id);
    expect(answers).toHaveLength(2);
    for (const answer of answers) {
      expect(answer.replyTo).toBe(request.id);
      expect(answer.trace).toBe(request.trace);
      expect(answer.cause).toBeUndefined();
    }
    expect((await network.send('b', text('x')))[0]?.trace).not.toBe(request.trace);
  });

  it('emit answers reply to the request and share its trace', async () => {
    const seen: Signal[] = [];
    const network = net((s) => seen.push(s));
    const request = network.emit('b', text('hi'));
    await network.idle();
    const answers = seen.filter((s) => s !== request);
    expect(answers.map((s) => s.from).sort()).toEqual(['a', 'c']);
    for (const answer of answers) {
      expect(answer.replyTo).toBe(request.id);
      expect(answer.trace).toBe(request.trace);
    }
  });
});

describe('cause', () => {
  it('a two-hop chain keeps one trace, with replyTo and cause links', async () => {
    const seen: Signal[] = [];
    const network = net((s) => seen.push(s));
    const first = network.emit('a', text('start')); // a -> b
    await network.idle();
    const answer = seen.find((s) => s.replyTo === first.id) as Signal; // b answers a
    const second = network.emit('b', answer.payload, { cause: answer }); // outside code passes it on: b -> a, c
    await network.idle();
    const answers = seen.filter((s) => s.replyTo === second.id);
    const sent = await network.send('b', text('again'), { cause: answer });

    expect(answer.from).toBe('b');
    expect(second.cause).toBe(answer.id);
    expect(second.replyTo).toBeUndefined();
    expect(answers).toHaveLength(2);
    const sendRequest = seen.find((s) => s.from === 'b' && s.payload.type === 'text' && s.payload.text === 'again') as Signal;
    expect(sendRequest.cause).toBe(answer.id);
    expect(sent.every((s) => s.replyTo === sendRequest.id)).toBe(true);
    const chain = [first, answer, second, ...answers];
    expect(new Set(chain.map((s) => s.trace))).toEqual(new Set([first.id]));
    // send takes cause too
    expect(sent.every((s) => s.trace === first.id)).toBe(true);
  });
});
