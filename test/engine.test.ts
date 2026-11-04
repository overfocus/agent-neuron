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
});

// Small networks of real processes (sh, cat) for the rules the storm does not pin down.
type Spec = Record<string, unknown> & { id: string };
function run(neurons: Spec[], links: [string, string][], inputs: Injection[], extra: Record<string, unknown> = {}): Promise<EngineTrace> {
  const file = join(mkdtempSync(join(tmpdir(), 'engine-')), 'n.network.json');
  const all = [{ id: 's', source: true }, ...neurons];
  writeFileSync(file, JSON.stringify({ version: 0, neurons: all, links: links.map(([from, to]) => ({ from, to })), ...extra }));
  return runNetwork(file, inputs);
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
    expect(t.events.filter((e) => e.type !== 'note').map((e) => e.id)).toEqual(['1', '2', '3', '4']);
  });
});

describe('refusals before a run', () => {
  const one = (neuron: Spec) => {
    const file = join(mkdtempSync(join(tmpdir(), 'engine-')), 'n.network.json');
    writeFileSync(file, JSON.stringify({ version: 0, neurons: [{ id: 's', source: true }, neuron], links: [{ from: 's', to: neuron.id }] }));
    return file;
  };

  it('says plainly that serve, http and human are not run by this engine', async () => {
    await expect(runNetwork(one({ id: 'x', serve: ['true'] }), [])).rejects.toThrow('unsupported neuron kinds in v0 of the engine: x (serve)');
    await expect(runNetwork(one({ id: 'x', http: 'http://127.0.0.1:1' }), [])).rejects.toThrow(/x \(http\)/);
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
  });
});
