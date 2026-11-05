// `human` neurons (SPEC 3.4, decision 0008): the engine asks a desk and the person's answer is the reply. These tests use a scripted
// desk; the page that is the command line's desk is tested in test/page.test.ts.
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { runNetwork, type Desk, type Injection, type Question, type Trace } from '../src/engine/index.js';

type Spec = Record<string, unknown> & { id: string };
function run(neurons: Spec[], links: [string, string][], inputs: Injection[], desk?: Desk): Promise<Trace> {
  const file = join(mkdtempSync(join(tmpdir(), 'human-')), 'n.network.json');
  writeFileSync(file, JSON.stringify({ version: 0, neurons: [{ id: 's', source: true }, ...neurons], links: links.map(([from, to]) => ({ from, to })) }));
  return runNetwork(file, inputs, desk ? { desk } : {});
}
const inject = (at: number, payload: unknown, replaces?: string): Injection => ({ at, source: 's', payload, ...(replaces !== undefined && { replaces }) });
const events = (t: Trace, type: string) => t.events.filter((e) => e.type === type);

/** A person who answers each question by rule, after `delay` ms (or never, when the rule returns undefined). */
function person(rule: (q: Question) => string | undefined, delay = 5) {
  const asked: Question[] = [], withdrawn: string[] = [];
  const desk: Desk = {
    ask(q, answer) {
      asked.push(q);
      const a = rule(q);
      if (a !== undefined) setTimeout(() => answer(a), delay);
    },
    withdraw: (id) => void withdrawn.push(id),
  };
  return { desk, asked, withdrawn };
}

const review = { id: 'p', human: true, description: 'Approve, edit or no.' };

describe('human neurons on a desk', () => {
  it('asks with the input, the neuron and its description; a non-empty answer is a text payload, forwarded on', async () => {
    const p = person(() => 'Ship it as written.');
    const t = await run([review, { id: 'after', run: ['cat'] }], [['s', 'p'], ['p', 'after']], [inject(0, { draft: 'a', proposal: 'b' })], p.desk);
    expect(p.asked).toHaveLength(1);
    expect(p.asked[0]).toMatchObject({ neuron: 'p', path: 'p', description: 'Approve, edit or no.', payload: { draft: 'a', proposal: 'b' } });
    expect(p.asked[0]!.id).toBe(events(t, 'signal')[0]!.id);
    const answer = events(t, 'answer').find((e) => e.from === 'p')!;
    expect(answer.payload).toEqual({ type: 'text', text: 'Ship it as written.' });
    expect(events(t, 'answer').find((e) => e.from === 'after')!.payload).toEqual({ type: 'text', text: 'Ship it as written.' });
    expect(p.withdrawn).toEqual([]);
  });

  it('an empty answer is quiet: nothing goes on', async () => {
    const p = person(() => '');
    const t = await run([review, { id: 'after', run: ['cat'] }], [['s', 'p'], ['p', 'after']], [inject(0, 'x')], p.desk);
    expect(events(t, 'quiet').map((e) => e.from)).toEqual(['p']);
    expect(events(t, 'answer')).toHaveLength(0);
  });

  it('withdraws a waiting question when a newer input with the same key arrives; the newer one is answered', async () => {
    const p = person((q) => (q.payload === 'second' ? 'ok' : undefined), 5);
    const t = await run([review], [['s', 'p']], [inject(0, 'first', 'k'), inject(30, 'second', 'k')], p.desk);
    expect(p.asked.map((q) => q.payload)).toEqual(['first', 'second']);
    expect(p.withdrawn).toEqual([p.asked[0]!.id]);
    expect(events(t, 'drop')).toEqual([expect.objectContaining({ reason: 'replaced', phase: 'running' })]);
    expect(events(t, 'answer').map((e) => (e.payload as { text: string }).text)).toEqual(['ok']);
  });

  it('withdraws a question nobody answers within timeout, which is an error with code timeout', async () => {
    const p = person(() => undefined);
    const t = await run([{ ...review, timeout: 60 }], [['s', 'p']], [inject(0, 'x')], p.desk);
    expect(p.withdrawn).toEqual([p.asked[0]!.id]);
    expect(events(t, 'error')).toEqual([expect.objectContaining({ neuron: 'p', code: 'timeout' })]);
  });

  it('ignores a second answer, and an answer after the question was withdrawn', async () => {
    const answers: ((a: string) => void)[] = [];
    const desk: Desk = { ask: (_q, answer) => void answers.push(answer), withdraw: () => {} };
    const done = run([{ ...review, timeout: 80 }], [['s', 'p']], [inject(0, 'x')], desk);
    await new Promise((r) => setTimeout(r, 20));
    answers[0]!('one');
    answers[0]!('two');
    const t = await done;
    expect(events(t, 'answer').map((e) => (e.payload as { text: string }).text)).toEqual(['one']);

    const late: ((a: string) => void)[] = [];
    const t2 = await run([{ ...review, timeout: 40 }], [['s', 'p']], [inject(0, 'x')], { ask: (_q, a) => void late.push(a), withdraw: () => {} });
    late[0]!('too late');
    expect(events(t2, 'answer')).toHaveLength(0);
  });

  it('refuses a file with a human neuron when there is no desk to ask', async () => {
    await expect(run([review], [['s', 'p']], [inject(0, 'x')])).rejects.toThrow(/p \(human, and no desk to ask\)/);
  });
});
