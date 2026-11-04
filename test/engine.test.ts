import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { beforeAll, describe, expect, it } from 'vitest';
import { INNER_IDS } from '../examples/alerts-demo.mjs';
import { replay, start, step, type Trace } from '../examples/trace/replay.mjs';
import { check, formatTrace, readInjections, runNetwork, type Injection, type Trace as EngineTrace } from '../src/engine/index.js';
import { expectValidTrace } from './support/trace.js';

const examples = fileURLToPath(new URL('../spec/examples/', import.meta.url));
const hasPython = spawnSync('python3', ['--version']).status === 0;

describe('the alert storm, run from its network file (no caller code)', () => {
  const alerts = JSON.parse(readFileSync(join(examples, '../../examples/alerts/alerts.json'), 'utf8')) as { service: string }[];

  it('has an inputs file that is the demo schedule, each alert keyed by its service', () => {
    const inputs = readInjections(join(examples, 'alerts.inputs.jsonl'));
    expect(inputs).toHaveLength(40);
    expect(inputs.map((i) => i.payload)).toEqual(alerts);
    expect(inputs.every((i, n) => i.source === 'alerts' && i.replaces === alerts[n]!.service)).toBe(true);
  });

  // Exact counts depend on timing, so only invariants are asserted (as test/alerts.test.ts does).
  const storm = (network: string, skip = false) => {
    let trace: Trace;
    beforeAll(async () => {
      if (skip) return;
      const run = await runNetwork(join(examples, network), readInjections(join(examples, 'alerts.inputs.jsonl')));
      trace = JSON.parse(formatTrace(run)) as Trace; // what `--trace` writes
    }, 40_000);

    it.skipIf(skip)(`${network}: a valid v1 trace that replays, with the engine named`, () => {
      expectValidTrace(trace);
      expect((trace as { engine?: string }).engine).toEqual(expect.stringContaining('engine'));
      const state = replay(trace);
      expect(state.neurons.alerts!.sent).toBe(40);
      expect(Object.values(state.held).flat()).toEqual([]);
      expect(Object.values(state.neurons).reduce((n, c) => n + c.errors, 0)).toBe(0);
    });

    it.skipIf(skip)(`${network}: fewer expensive calls than alerts, each with two distinct detectors on one trace`, () => {
      const state = start(trace);
      for (const e of trace.events) {
        const before = (state.held.hypothesis ?? []).filter((h) => h.trace === e.trace).map((h) => h.from);
        if (step(state, e).includes('hypothesis')) expect(new Set([...before, e.from]).size).toBeGreaterThanOrEqual(2);
      }
      expect(state.neurons.hypothesis!.acted).toBeGreaterThan(0);
      expect(state.neurons.hypothesis!.acted).toBeLessThan(40);
    });

    it.skipIf(skip)(`${network}: diagnosis answers name the gateway and its deploy; no inner id appears`, () => {
      const verdicts = trace.events.filter((e) => e.type === 'answer' && e.from === 'diagnosis');
      expect(verdicts.length).toBeGreaterThan(0);
      for (const v of verdicts) expect((v.payload as { data: unknown }).data).toMatchObject({ root: 'gateway', deploy: 'dep-8b21' });
      const text = JSON.stringify(trace);
      for (const id of INNER_IDS) expect(text).not.toContain(`"${id}"`);
    });
  };

  storm('alerts.network.json');
  if (!hasPython) console.warn('python3 not found: the Python hypothesis variant is skipped');
  storm('alerts-python.network.json', !hasPython);
  storm('alerts-python-serve.network.json', !hasPython); // level 1: the same neuron as one long-lived process
});

describe('the alert storm with a budget (alerts-budget.network.json)', () => {
  type E = Record<string, any>;
  let events: E[];
  beforeAll(async () => {
    // the demo's producer lowers every alert that is not critical to half the file's budget
    const inputs = readInjections(join(examples, 'alerts.inputs.jsonl')).map((i) => ((i.payload as { severity: string }).severity === 'crit' ? i : { ...i, budget: { credits: 6 } }));
    const trace = JSON.parse(formatTrace(await runNetwork(join(examples, 'alerts-budget.network.json'), inputs))) as Trace;
    expectValidTrace(trace);
    events = trace.events as E[];
  }, 40_000);

  it('finishes every alert trace once, with a pool no larger than its ceiling', () => {
    const roots = events.filter((e) => e.type === 'signal' && e.from === 'alerts');
    const finished = events.filter((e) => e.type === 'finished');
    expect(roots).toHaveLength(40);
    expect(finished.map((f) => f.trace).sort()).toEqual(roots.map((r) => r.trace).sort());
    for (const f of finished) {
      const crit = roots.find((r) => r.trace === f.trace)!.payload.data.severity === 'crit';
      expect(f.remaining.credits).toBeGreaterThanOrEqual(0);
      expect(f.remaining.credits).toBeLessThanOrEqual(crit ? 12 : 6);
    }
  });

  it('refuses a call only for lack of budget, and only in an alert whose budget was lowered', () => {
    const refused = events.filter((e) => e.type === 'error');
    expect(refused.every((e) => e.code === 'budget')).toBe(true);
    expect(events.filter((e) => e.type === 'finished' && e.status === 'exhausted')).toHaveLength(new Set(refused.map((e) => e.trace)).size);
    for (const e of refused) expect(events.find((x) => x.type === 'signal' && x.id === e.trace)!.payload.data.severity).not.toBe('crit');
  });
});

// Small networks of real processes (sh, cat) for the rules the storm does not pin down.
type Spec = Record<string, unknown> & { id: string };
function run(neurons: Spec[], links: [string, string][], inputs: Injection[], extra: Record<string, unknown> = {}, options: { pingEvery?: number } = {}): Promise<EngineTrace> {
  const file = join(mkdtempSync(join(tmpdir(), 'engine-')), 'n.network.json');
  const all = [{ id: 's', source: true }, ...neurons];
  writeFileSync(file, JSON.stringify({ version: 0, neurons: all, links: links.map(([from, to]) => ({ from, to })), ...extra }));
  return runNetwork(file, inputs, options);
}
const inject = (at: number, payload: unknown, replaces?: string): Injection => ({ at, source: 's', payload, ...(replaces !== undefined && { replaces }) });
const events = (t: EngineTrace, type: string) => t.events.filter((e) => e.type === type);
const echo = (id: string, extra: Record<string, unknown> = {}): Spec => ({ id, run: ['cat'], ...extra }); // answers with its stdin
const sh = (id: string, script: string, extra: Record<string, unknown> = {}): Spec => ({ id, run: ['sh', '-c', script], ...extra });

describe('routing and level 0 processes', () => {
  it('answers go to the sender and forward along links with cause and the inherited key; quiet forwards nothing', async () => {
    const t = await run([echo('a'), sh('q', 'true'), echo('b')], [['s', 'a'], ['a', 'q'], ['a', 'b']], [inject(0, { n: 1 }, 'k')]);
    expectValidTrace(t as unknown as Trace);
    const [root, fwd] = events(t, 'signal');
    expect(root).toMatchObject({ from: 's', to: ['a'], hops: 0, replaces: 'k' });
    const answerA = events(t, 'answer')[0]!;
    expect(answerA).toMatchObject({ from: 'a', to: ['s'], replyTo: root!.id, hops: 1, payload: { type: 'json', data: { n: 1 } } });
    expect(fwd).toMatchObject({ from: 'a', to: ['q', 'b'], cause: answerA.id, hops: 2, replaces: 'k' });
    expect(events(t, 'quiet')).toHaveLength(1); // q is quiet and forwards nothing
    expect(events(t, 'signal')).toHaveLength(2);
    expect(events(t, 'answer').map((e) => e.from)).toEqual(['a', 'b']); // b has no outgoing links: its answer is the result
  });

  it('gives a threshold neuron {payload, inputs} only above 1, counts a sender once, and answers the input that crossed', async () => {
    const links: [string, string][] = [['s', 'p'], ['s', 'q'], ['p', 'h'], ['q', 'h']];
    const t = await run([echo('p'), echo('q'), echo('h', { threshold: 2 })], links, [inject(0, 1)]);
    const answer = events(t, 'answer').find((e) => e.from === 'h')!;
    expect(answer.payload).toMatchObject({ data: { payload: 1, inputs: [1, 1] } });
    const crossing = events(t, 'signal').filter((e) => (e.to as string[]).includes('h')).at(-1)!;
    expect(answer).toMatchObject({ to: [crossing.from], replyTo: crossing.id });
    const one = await run([echo('p'), echo('h', { threshold: 1 })], [['s', 'p'], ['p', 'h']], [inject(0, 7)]);
    expect(events(one, 'answer').find((e) => e.from === 'h')!.payload).toMatchObject({ data: 7 }); // bare payload
  });

  it('drops a held input as expired, and one that a newer input from the same sender replaced', async () => {
    const lone = await run([echo('p'), echo('h', { threshold: 2, within: 50 })], [['s', 'p'], ['p', 'h']], [inject(0, 1), inject(10, 2)]);
    expect(events(lone, 'drop').map((e) => [e.reason, e.phase])).toEqual([['expired', undefined], ['expired', undefined]]);
    expect(events(lone, 'answer').filter((e) => e.from === 'h')).toHaveLength(0);
    // one sender, one trace, three inputs: p forwards to itself and to h, so h gets p's second input while its first is held
    const loop = await run([echo('p', { maxHops: 4 }), echo('h', { threshold: 2, within: 50 })], [['s', 'p'], ['p', 'h'], ['p', 'p']], [inject(0, 1)]);
    const drops = events(loop, 'drop').map((e) => [e.reason, e.phase]);
    expect(drops).toHaveLength(4);
    expect(drops.filter(([r]) => r === 'replaced')).toEqual([['replaced', 'waiting'], ['replaced', 'waiting']]);
    expect(drops.map(([r]) => r).sort()).toEqual(['expired', 'hops', 'replaced', 'replaced']);
    // a newer key from the same sender also replaces a held input, across traces
    const keyed = await run([echo('p'), echo('h', { threshold: 2, within: 400 })], [['s', 'p'], ['p', 'h']], [inject(0, 1, 'k'), inject(250, 2, 'k')]);
    expect(events(keyed, 'drop').map((e) => [e.reason, e.phase])).toContainEqual(['replaced', 'waiting']);
  });

  it('cuts a running call when its input goes stale: SIGTERM to the group, one drop with phase running, no answer', async () => {
    const t = await run([sh('slow', 'sleep 2; echo 1')], [['s', 'slow']], [inject(0, 1, 'k'), inject(150, 2, 'k')]);
    expect(events(t, 'drop').map((e) => [e.reason, e.phase, e.neuron])).toEqual([['replaced', 'running', 'slow']]);
    expect(t.events.filter((e) => e.type === 'note').map((e) => e.what).slice(0, 2)).toEqual(['cancel', 'term']);
    expect(events(t, 'answer')).toHaveLength(1); // only the newer input answers
    expect(t.events.at(-1)!.t).toBeLessThan(2900);
  }, 10_000);

  it('turns failures into error events with their code, forwarding nothing', async () => {
    const t = await run(
      [sh('bad', 'echo oops >&2; exit 3'), sh('junk', 'echo not json'), sh('big', 'head -c 2000000 /dev/zero | tr "\\0" x'), sh('hang', 'sleep 5', { timeout: 100 }), { id: 'gone', run: ['no-such-program-here'] }, echo('after')],
      [['s', 'bad'], ['s', 'junk'], ['s', 'big'], ['s', 'hang'], ['s', 'gone'], ['bad', 'after']],
      [inject(0, 1)],
    );
    const errors = Object.fromEntries(events(t, 'error').map((e) => [e.neuron as string, e]));
    expect(errors.bad).toMatchObject({ code: 'exit', message: 'exit 3: oops' });
    expect(errors.junk).toMatchObject({ code: 'junk', message: expect.stringMatching(/^not JSON/) });
    expect(errors.big).toMatchObject({ code: 'output', message: expect.stringMatching(/^output too large/) });
    expect(errors.hang).toMatchObject({ code: 'timeout', message: 'timed out after 100 ms' });
    expect(errors.gone).toMatchObject({ code: 'exit' });
    expect(events(t, 'signal')).toHaveLength(1); // nothing was forwarded
    expect(t.events.filter((e) => e.type === 'note' && e.neuron === 'hang').map((e) => e.what)).toEqual(['cancel', 'term']);
  }, 10_000);

  it('gives a program only PATH and the variables its env names', async () => {
    process.env.AN_TEST_VISIBLE = 'yes';
    process.env.AN_TEST_HIDDEN = 'no';
    const t = await run([sh('e', 'printf \'"%s|%s"\' "$AN_TEST_VISIBLE" "$AN_TEST_HIDDEN"', { env: ['AN_TEST_VISIBLE'] })], [['s', 'e']], [inject(0, 1)]);
    expect(events(t, 'answer')[0]!.payload).toEqual({ type: 'text', text: 'yes|' });
  });

  it('cuts a payload over 2000 characters to a preview, as the recorder does', async () => {
    const t = await run([sh('long', 'printf \'"%s"\' "$(head -c 3000 /dev/zero | tr "\\0" x)"')], [['s', 'long']], [inject(0, 1)]);
    expect(events(t, 'answer')[0]!.payload).toEqual({ type: 'text', text: 'x'.repeat(2000), truncated: true });
  });

  it('refuses inputs beyond maxHops and so ends a loop', async () => {
    const t = await run([echo('a', { maxHops: 4 }), echo('b', { maxHops: 4 })], [['s', 'a'], ['a', 'b'], ['b', 'a']], [inject(0, 1)]);
    expect(events(t, 'drop').map((e) => [e.reason, e.phase, e.hops])).toEqual([['hops', undefined, 6]]);
  });

  it('seals a composed neuron: one answer out, no inner id, an inner failure is only its own error', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'engine-'));
    const inner = (exit: Spec) => ({ version: 0, entry: 'in', exit: 'out', neurons: [echo('in'), exit], links: [{ from: 'in', to: 'out' }] });
    writeFileSync(join(dir, 'ok.network.json'), JSON.stringify(inner(sh('out', 'cat; true'))));
    writeFileSync(join(dir, 'bad.network.json'), JSON.stringify(inner(sh('out', 'exit 1'))));
    writeFileSync(join(dir, 'quiet.network.json'), JSON.stringify(inner(sh('out', 'true'))));
    const outer = { version: 0, neurons: [{ id: 's', source: true }, ...['ok', 'bad', 'quiet'].map((id) => ({ id, network: `${id}.network.json` }))], links: ['ok', 'bad', 'quiet'].map((to) => ({ from: 's', to })) };
    writeFileSync(join(dir, 'outer.network.json'), JSON.stringify(outer));
    const seen: string[] = [];
    const t = await runNetwork(join(dir, 'outer.network.json'), [inject(0, { n: 1 })], { inner: (path, e) => seen.push(`${path}:${e.type}`) });
    expectValidTrace(t as unknown as Trace);
    expect(events(t, 'answer').map((e) => e.from)).toEqual(['ok']);
    expect(events(t, 'quiet').map((e) => e.from)).toEqual(['quiet']);
    expect(events(t, 'error')).toEqual([expect.objectContaining({ neuron: 'bad', code: 'inner', message: 'no answer: an inner neuron failed' })]);
    expect(JSON.stringify(t)).not.toMatch(/"(in|out)"/);
    expect(seen).toContain('ok:answer'); // the inside is visible only to an operator who asks, by instance path
    expect(seen.some((s) => s.startsWith('ok/'))).toBe(false);
  });
});

describe('call lifecycle', () => {
  it('does not cut a call that has answered when its keyed forward comes back to itself', async () => {
    const t = await run([echo('p', { maxHops: 2 })], [['s', 'p'], ['p', 'p']], [inject(0, 1, 'k')]);
    expect(events(t, 'drop').map((e) => e.reason)).toEqual(['hops']);
    expect(events(t, 'answer')).toHaveLength(2);
  });

  it('does the same through a composed neuron, and check refuses a composed id that is an inner id', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'engine-'));
    const inner = (a: string) => ({ version: 0, entry: a, exit: 'out', neurons: [echo(a), echo('out')], links: [{ from: a, to: 'out' }] });
    writeFileSync(join(dir, 'in.network.json'), JSON.stringify(inner('in')));
    writeFileSync(join(dir, 'c.network.json'), JSON.stringify({ version: 0, neurons: [{ id: 's', source: true }, { id: 'c', network: 'in.network.json', maxHops: 2 }], links: [{ from: 's', to: 'c' }, { from: 'c', to: 'c' }] }));
    const t = await runNetwork(join(dir, 'c.network.json'), [inject(0, 1, 'k')]);
    expect(events(t, 'drop').map((e) => e.reason)).toEqual(['hops']);
    writeFileSync(join(dir, 'same.network.json'), JSON.stringify({ version: 0, neurons: [{ id: 's', source: true }, { id: 'in', network: 'in.network.json' }], links: [{ from: 's', to: 'in' }] }));
    expect(check(join(dir, 'same.network.json'))).toEqual([expect.objectContaining({ severity: 'error', message: expect.stringContaining('same id as a neuron inside in.network.json') })]);
  });

  it('decodes a long non-ASCII answer whole, whatever the chunking', async () => {
    const answers: unknown[] = [];
    const file = join(mkdtempSync(join(tmpdir(), 'engine-')), 'n.network.json');
    const program = 'process.stdout.write(JSON.stringify("a" + "\u20ac".repeat(300000)))';
    writeFileSync(file, JSON.stringify({ version: 0, neurons: [{ id: 's', source: true }, { id: 'p', run: ['node', '-e', program] }], links: [{ from: 's', to: 'p' }] }));
    await runNetwork(file, [inject(0, 1)], { result: (_n, _t, payload) => answers.push(payload) });
    expect(answers).toEqual(['a' + '\u20ac'.repeat(300000)]);
  });

  it('kills what a program leaves behind, and does not wait for a child that holds stdout open', async () => {
    const orphan = await run([sh('o', 'sleep 38 >/dev/null 2>&1 & echo 1')], [['s', 'o']], [inject(0, 1)]);
    expect(events(orphan, 'answer')).toHaveLength(1);
    expect(spawnSync('pgrep', ['-f', 'sleep 38']).status).toBe(1); // none left
    const held = await run([sh('h', 'echo 1; sleep 3 &', { timeout: 1500 })], [['s', 'h']], [inject(0, 1)]);
    expect(events(held, 'answer')).toHaveLength(1);
    expect(events(held, 'error')).toEqual([]);
  }, 10_000);

  it('numbers ids per scope: outer ids are consecutive around a composed call', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'engine-'));
    writeFileSync(join(dir, 'in.network.json'), JSON.stringify({ version: 0, entry: 'a', exit: 'b', neurons: [echo('a'), echo('b')], links: [{ from: 'a', to: 'b' }] }));
    writeFileSync(join(dir, 'o.network.json'), JSON.stringify({ version: 0, neurons: [{ id: 's', source: true }, { id: 'c', network: 'in.network.json' }, echo('z')], links: [{ from: 's', to: 'c' }, { from: 'c', to: 'z' }] }));
    const t = await runNetwork(join(dir, 'o.network.json'), [inject(0, 1)]);
    expect(t.events.filter((e) => e.type !== 'note' && e.type !== 'finished').map((e) => e.id)).toEqual(['1', '2', '3', '4']);
  });
});

describe('refusals before a run', () => {
  const one = (neuron: Spec) => {
    const file = join(mkdtempSync(join(tmpdir(), 'engine-')), 'n.network.json');
    writeFileSync(file, JSON.stringify({ version: 0, neurons: [{ id: 's', source: true }, neuron], links: [{ from: 's', to: neuron.id }] }));
    return file;
  };

  it('says plainly that http and human are not run by this engine', async () => {
    await expect(runNetwork(one({ id: 'x', http: 'http://127.0.0.1:1' }), [])).rejects.toThrow('unsupported neuron kinds in v0 of the engine: x (http)');
    await expect(runNetwork(one({ id: 'x', human: true }), [])).rejects.toThrow(/x \(human\)/);
  });

  it('refuses a file that does not check, and an injection into something that is not a source', async () => {
    await expect(runNetwork(one({ id: 'x', run: ['true'], bogus: 1 }), [])).rejects.toThrow('unknown field "bogus"');
    await expect(runNetwork(one({ id: 'x', run: ['true'] }), [{ at: 0, source: 'x', payload: 1 }])).rejects.toThrow('not a source');
  });

  it('enforces the injection limits itself, not only when reading a file', async () => {
    const net = one({ id: 'x', run: ['true'] });
    await expect(runNetwork(net, [{ at: 0, source: 's', payload: 1, replaces: 'k'.repeat(257) }])).rejects.toThrow('256 bytes');
    await expect(runNetwork(net, [{ at: -5, source: 's', payload: 1 }])).rejects.toThrow('"at"');
    await expect(runNetwork(net, [{ at: Number.NaN, source: 's', payload: 1 }])).rejects.toThrow('"at"');
  });

  it('lets an injection lower the budget of its trace and never raise it', async () => {
    const file = join(mkdtempSync(join(tmpdir(), 'engine-')), 'n.network.json');
    const net = (budget?: object) => writeFileSync(file, JSON.stringify({ version: 0, ...(budget && { budget }), neurons: [{ id: 's', source: true }, { id: 'x', run: ['true'], ...(budget && { cost: { credits: 0 } }) }], links: [{ from: 's', to: 'x' }] }));
    const inject = (budget: unknown) => runNetwork(file, [{ at: 0, source: 's', payload: 1, budget: budget as NonNullable<Injection['budget']> }]);
    net({ credits: 10 });
    await expect(inject({ credits: 11 })).rejects.toThrow('never raise');
    await expect(inject({ gold: 1 })).rejects.toThrow('does not have');
    await expect(inject({ credits: 1.5 })).rejects.toThrow('integer');
    await expect(inject({ credits: -1 })).rejects.toThrow('integer');
    await expect(inject([1])).rejects.toThrow('object of units');
    expect((await inject({ credits: 0 })).events.at(-1)).toMatchObject({ type: 'finished', remaining: { credits: 0 } });
    net();
    await expect(inject({ credits: 1 })).rejects.toThrow('does not have');
  });

  it('reads inputs strictly: line number, at, source, payload, a key of at most 256 bytes', () => {
    const read = (text: string) => {
      const file = join(mkdtempSync(join(tmpdir(), 'engine-')), 'in.jsonl');
      writeFileSync(file, text);
      return () => readInjections(file);
    };
    expect(read('{"at":0,"source":"s","payload":null}\n\n{"at":5,"source":"s","payload":"x","replaces":"k"}\n')()).toHaveLength(2);
    expect(read('{"at":0,"source":"s","payload":1}\nnope\n')).toThrow(/in\.jsonl:2: not JSON/);
    expect(read('{"at":-1,"source":"s","payload":1}')).toThrow('"at"');
    expect(read('{"at":0,"payload":1}')).toThrow('"source"');
    expect(read('{"at":0,"source":"s"}')).toThrow('"payload"');
    expect(read(JSON.stringify({ at: 0, source: 's', payload: 1, replaces: 'k'.repeat(257) }))).toThrow('256 bytes');
    expect(read('{"at":0,"source":"s","payload":1,"budget":{"credits":3}}')()[0]!.budget).toEqual({ credits: 3 });
  });
});

// Level 1: test/support/serve.mjs is a scriptable neuron that logs what it receives (flags and payloads: see that file).
describe('level 1 serve neurons', () => {
  const script = fileURLToPath(new URL('./support/serve.mjs', import.meta.url));
  const made = () => join(mkdtempSync(join(tmpdir(), 'engine-serve-')), 'log.jsonl');
  const srv = (id: string, log: string, flags: string[] = [], extra: Record<string, unknown> = {}): Spec => ({ id, serve: ['node', script, log, ...flags], ...extra });
  const seen = (log: string) => readFileSync(log, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l) as { type: string; re?: number });
  const notes = (t: EngineTrace) => events(t, 'note').map((e) => e.what);
  const data = (e: EngineTrace["events"][number]) => (e.payload as { data?: unknown; text?: string }).data ?? (e.payload as { text?: string }).text;

  it('says hello, numbers calls with re, queues past maxInflight, and gives a threshold neuron its inputs', async () => {
    const log = made();
    const t = await run([srv('x', log)], [['s', 'x']], [inject(0, 1), inject(0, 2), inject(0, 3)]);
    expectValidTrace(t as unknown as Trace);
    expect(events(t, 'answer').map((e) => data(e))).toEqual([1, 2, 3]);
    expect(seen(log)[0]).toEqual({ v: 1, type: 'hello', neuron: 'x' });
    expect(seen(log).filter((m) => m.type === 'signal').map((m) => m.re)).toEqual([1, 2, 3]);
    const k = await run([echo('p'), echo('q'), srv('h', made(), [], { threshold: 2 })], [['s', 'p'], ['s', 'q'], ['p', 'h'], ['q', 'h']], [inject(0, 7)]);
    expect(events(k, 'answer').find((e) => e.from === 'h')!.payload).toMatchObject({ data: [7, 7] });
  });

  it('sends cancel to the running process, discards its late reply, and drops a queued call as waiting', async () => {
    const log = made();
    // A runs and is cut by D; B waits for a slot and is replaced by C; the slot is kept until A replies for its re
    const t = await run([srv('x', log)], [['s', 'x']], [inject(0, { hold: 1 }, 'a'), inject(50, { n: 'B' }, 'b'), inject(100, { n: 'C' }, 'b'), inject(150, { n: 'D' }, 'a')]);
    expectValidTrace(t as unknown as Trace);
    expect(events(t, 'drop').map((e) => [e.reason, e.phase, (data(e) as { n?: string }).n ?? 'A'])).toEqual([['replaced', 'waiting', 'B'], ['replaced', 'running', 'A']]);
    expect(seen(log).filter((m) => m.type === 'cancel')).toEqual([{ type: 'cancel', re: 1 }]); // only the running call is told
    expect(notes(t)).toEqual(['cancel']);
    expect(events(t, 'answer').map((e) => data(e))).toEqual([{ n: 'C' }, { n: 'D' }]); // "late" is discarded
  });

  it('times out a hung call, records cancel and term, kills what ignores SIGTERM, restarts, and carries on', async () => {
    const t = await run([srv('x', made(), ['stubborn'], { timeout: 150 })], [['s', 'x']], [inject(0, { hang: 1 }), inject(500, 'again')]);
    expect(events(t, 'error').map((e) => [e.code, e.message])).toEqual([['timeout', 'timed out after 150 ms']]);
    expect(notes(t)).toEqual(['cancel', 'term', 'restart', 'kill']);
    expect(events(t, 'answer').map((e) => data(e))).toEqual(['again']);
  }, 15_000);

  it('turns a junk line into a note, and fails the call only when exactly one call is running', async () => {
    const one = await run([srv('x', made())], [['s', 'x']], [inject(0, { say: ['not json', { type: 'answer', payload: 1 }] })]);
    expect(events(one, 'error').map((e) => [e.code, e.message])).toEqual([['junk', 'junk on stdout: not json']]);
    expect(events(one, 'note')).toEqual([expect.objectContaining({ what: 'junk', line: 2, text: 'not json', neuron: 'x' })]); // line 1 was ready
    expect(events(one, 'answer')).toHaveLength(0); // the late answer for the failed call is discarded
    const many = await run([srv('x', made(), ['many'], { timeout: 300 })], [['s', 'x']], [inject(0, { hang: 1 }), inject(0, { say: ['oops', { type: 'answer', payload: 2 }] })]);
    expect(events(many, 'note').filter((e) => e.what === 'junk')).toHaveLength(1);
    expect(events(many, 'error').map((e) => e.code)).toEqual(['timeout']); // the hung call; junk failed nobody
    expect(events(many, 'answer').map((e) => data(e))).toEqual([2]);
  });

  it('fails every running call when the process exits, restarts it with backoff, and the network carries on', async () => {
    const t = await run([srv('x', made(), ['many']), echo('after')], [['s', 'x'], ['x', 'after']], [inject(0, { hang: 1 }), inject(0, { exit: 1 }), inject(700, 'next')]);
    expectValidTrace(t as unknown as Trace);
    expect(events(t, 'error').map((e) => e.code)).toEqual(['crash', 'crash']);
    expect(events(t, 'error')[0]!.message).toMatch(/^process failed/);
    expect(notes(t)).toEqual(['restart']);
    expect(events(t, 'answer').map((e) => [e.from, data(e)])).toEqual([['x', 'next'], ['after', 'next']]);
  });

  it('is failed after 5 failed starts: every input is an error with code start', async () => {
    const t = await run([srv('x', made(), ['failrestart'])], [['s', 'x']], [inject(0, { exit: 1 }), inject(200, 'queued'), inject(3700, 'after')]);
    expect(events(t, 'error').map((e) => e.code)).toEqual(['crash', 'start', 'start']);
    expect(events(t, 'error')[1]!.message).toBe('neuron failed to start');
    expect(notes(t)).toEqual(Array(4).fill('restart')); // the first crash came within a second of ready, so it counts as a failed start
  }, 15_000);

  it('notices a crash even when a helper keeps stdout open, and restarts', async () => {
    const t = await run([srv('x', made())], [['s', 'x']], [inject(0, { helper: 1 }), inject(600, 'next')]);
    expect(events(t, 'error').map((e) => e.code)).toEqual(['crash']);
    expect(notes(t)).toEqual(['term', 'restart']); // the helper stayed in the process group until SIGTERM
    expect(events(t, 'answer').map((e) => data(e))).toEqual(['next']);
  });

  it('fails the one running call with output for a line over 1 MiB, several with crash, and restarts', async () => {
    const big = 'x'.repeat((1 << 20) + 10);
    const one = await run([srv('x', made())], [['s', 'x']], [inject(0, { say: [{ type: 'answer', payload: big }] })]);
    expect(events(one, 'error').map((e) => [e.code, e.message])).toEqual([['output', 'output too large']]);
    const many = await run([srv('x', made(), ['many'])], [['s', 'x']], [inject(0, { hang: 1 }), inject(0, { say: [{ type: 'answer', payload: big }] })]);
    expect(events(many, 'error').map((e) => e.code)).toEqual(['crash', 'crash']);
  });

  it('answers an error reply without a message with a protocol error', async () => {
    const t = await run([srv('x', made())], [['s', 'x']], [inject(0, { say: [{ type: 'error' }] })]);
    expect(events(t, 'error').map((e) => e.code)).toEqual(['protocol']);
  });

  it('refuses to start when a serve neuron cannot reach ready', async () => {
    const refuse = (serve: string[], why: RegExp, extra: Record<string, unknown> = {}) => {
      const file = join(mkdtempSync(join(tmpdir(), 'engine-')), 'n.network.json');
      writeFileSync(file, JSON.stringify({ version: 0, neurons: [{ id: 's', source: true }, { id: 'x', serve, ...extra }], links: [{ from: 's', to: 'x' }] }));
      return expect(runNetwork(file, [])).rejects.toThrow(why);
    };
    await refuse(['node', '-e', 'process.exit(1)'], /serve neuron "x" did not reach ready: process failed/);
    await refuse(['sh', '-c', 'echo \'{"type":"hello"}\'; exec sleep 3'], /the first line must be ready/);
    await refuse(['sh', '-c', 'echo \'{"v":2,"type":"ready"}\'; exec sleep 3'], /ready\.v differs from hello\.v/);
    await refuse(['sh', '-c', 'exec sleep 3'], /no ready within 100 ms/, { timeout: 100 });
    await refuse(['no-such-program-here'], /could not start no-such-program-here/);
  });

  it('applies a reply\'s to and replaces, and fails a call whose reply has a bad shape (code protocol)', async () => {
    const reply = async (fields: object | undefined) => {
      const t = await run([srv('x', made()), echo('a'), echo('b')], [['s', 'x'], ['x', 'a'], ['x', 'b']], [inject(0, { say: [{ type: 'answer', ...fields }] }, 'k')]);
      return { signals: events(t, 'signal').slice(1), errors: events(t, 'error'), answers: events(t, 'answer').filter((e) => e.from === 'x') };
    };
    expect((await reply({ payload: 1 })).signals[0]).toMatchObject({ to: ['a', 'b'], replaces: 'k' });
    expect((await reply({ payload: 1, to: ['b', 'a'] })).signals[0]).toMatchObject({ to: ['a', 'b'] }); // link declaration order
    expect((await reply({ payload: 1, to: ['b'] })).signals[0]).toMatchObject({ to: ['b'] });
    expect((await reply({ payload: 1, replaces: 'new' })).signals[0]).toMatchObject({ replaces: 'new' });
    expect((await reply({ payload: 1, replaces: null })).signals[0]).not.toHaveProperty('replaces');
    const none = await reply({ payload: 1, to: [] });
    expect([none.signals.length, none.answers.length]).toEqual([0, 1]); // reported, forwarded to nobody
    for (const bad of [{ payload: 1, to: ['zzz'] }, { payload: 1, to: 'a' }, { payload: 1, replaces: 'k'.repeat(257) }, { payload: 1, replaces: 5 }, undefined]) {
      const r = await reply(bad);
      expect(r.errors.map((e) => [e.code, e.message])).toEqual([['protocol', expect.stringMatching(/^protocol: /)]]);
      expect(r.answers).toHaveLength(0);
    }
  });

  it('pings an idle neuron, and restarts one that does not pong', async () => {
    const log = made();
    const ok = await run([srv('x', log, [], { timeout: 300 })], [['s', 'x']], [inject(0, 1), inject(400, 2)], {}, { pingEvery: 30 });
    expect(seen(log).filter((m) => m.type === 'ping').length).toBeGreaterThan(0);
    expect(notes(ok)).toEqual([]);
    expect(events(ok, 'answer').map((e) => data(e))).toEqual([1, 2]);
    const dead = await run([srv('x', made(), ['noping'], { timeout: 100 })], [['s', 'x']], [inject(0, 1), inject(600, 2)], {}, { pingEvery: 30 });
    expect(notes(dead).slice(0, 2)).toEqual(['term', 'restart']);
    expect(data(events(dead, 'answer')[0]!)).toBe(1);
  }, 15_000);

  it('keeps one process for a serve neuron inside a composed neuron, shared by every call', async () => {
    const log = made(), dir = mkdtempSync(join(tmpdir(), 'engine-'));
    writeFileSync(join(dir, 'in.network.json'), JSON.stringify({ version: 0, entry: 'in', exit: 'in', neurons: [srv('in', log)], links: [] }));
    writeFileSync(join(dir, 'outer.network.json'), JSON.stringify({ version: 0, neurons: [{ id: 's', source: true }, { id: 'c', network: 'in.network.json' }], links: [{ from: 's', to: 'c' }] }));
    const t = await runNetwork(join(dir, 'outer.network.json'), [inject(0, 1), inject(100, 2)]);
    expect(events(t, 'answer').map((e) => data(e))).toEqual([1, 2]);
    expect(seen(log).filter((m) => m.type === 'hello')).toHaveLength(1);
    expect(seen(log).filter((m) => m.type === 'signal').map((m) => m.re)).toEqual([1, 2]);
    expect(JSON.stringify(t)).not.toContain('"in"');
  });
});

describe('budgets, with real processes (docs/SPEC.md 3.5)', () => {
  const file = join(mkdtempSync(join(tmpdir(), 'engine-')), 'n.network.json');
  const sh = (id: string, cost: number, script: string) => ({ id, run: ['sh', '-c', script], cost: { credits: cost } });
  const net = (budget?: object) => writeFileSync(file, JSON.stringify({
    version: 0, ...(budget && { budget }),
    neurons: [{ id: 's', source: true }, sh('a', 3, 'sleep 0.05; echo 1'), sh('b', 3, 'echo 2')], links: [{ from: 's', to: 'a' }, { from: 'a', to: 'b' }],
  }));

  it('reserves, commits with measured ms, refuses what the pool cannot cover, and finishes the trace', async () => {
    net({ credits: 5 });
    const t = await runNetwork(file, [{ at: 0, source: 's', payload: 0 }]);
    expect(t.budget).toEqual({ credits: 5 });
    expect(t.neurons[1]).toMatchObject({ id: 'a', cost: { credits: 3 } });
    const notes = t.events.filter((e) => e.type === 'note').map((e) => [e.neuron, e.what]);
    expect(notes).toEqual([['a', 'reserve'], ['a', 'commit']]);
    const commit = t.events.find((e) => e.what === 'commit')!;
    expect(commit).toMatchObject({ units: { credits: 3 }, basis: 'estimated' });
    expect(commit.ms as number).toBeGreaterThanOrEqual(40);
    expect(t.events.find((e) => e.type === 'error')).toMatchObject({ neuron: 'b', code: 'budget', message: 'budget: needs 3 credits, 2 left' });
    expect(t.events.at(-1)).toMatchObject({ type: 'finished', status: 'exhausted', remaining: { credits: 2 } });
    expectValidTrace(JSON.parse(formatTrace(t)) as Trace);
  });

  it('writes no accounting without a budget, and still ends the trace with finished', async () => {
    writeFileSync(file, JSON.stringify({ version: 0, neurons: [{ id: 's', source: true }, { id: 'a', run: ['sh', '-c', 'echo 1'] }], links: [{ from: 's', to: 'a' }] }));
    const t = await runNetwork(file, [{ at: 0, source: 's', payload: 0 }]);
    expect(t.events.filter((e) => e.type === 'note')).toEqual([]);
    expect(t.events.filter((e) => e.type === 'finished')).toEqual([expect.objectContaining({ status: 'done' })]);
    expect(t.events.at(-1)).not.toHaveProperty('remaining');
    expect(t).not.toHaveProperty('budget');
  });
});
