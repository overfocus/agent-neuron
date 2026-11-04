import { existsSync, readdirSync, readFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { describe, expect, it } from 'vitest';
import { INNER_IDS } from '../examples/alerts-demo.mjs';
import { replay, start, step, type Trace, type TraceEvent } from '../examples/trace/replay.mjs';
import { tap } from '../examples/trace/tap.mjs';
import { Network, Neuron, text, type Adapter, type AdapterContext } from '../src/index.js';

// Every committed trace (docs/TRACE.md). Re-record with `npm run record` after changing a demo.
const dir = new URL('../traces/', import.meta.url);
const files = readdirSync(dir).filter((f) => f.endsWith('.json')).sort();
const raw = new Map(files.map((f) => [f, readFileSync(new URL(f, dir), 'utf8')]));
const traces = new Map([...raw].map(([f, text]) => [f, JSON.parse(text) as Trace]));
const REASONS = ['replaced', 'expired', 'hops'];
const TYPES = ['signal', 'answer', 'quiet', 'drop', 'error', 'fail'];

describe('committed traces', () => {
  it('has the expected recordings', () => {
    expect(files).toEqual(['alerts-threshold-1.json', 'alerts-threshold-3.json', 'alerts.json', 'suggest.json', 'typing.json']);
  });

  for (const [file, trace] of traces) {
    it(`${file} is valid`, () => {
      expect(trace).toMatchObject({ format: 'agent-neuron-trace', version: 1 });
      expect(trace.description).toEqual(expect.any(String));
      expect(trace.library).toMatch(/^\d+\.\d+\.\d+/);
      const ids = new Set(trace.neurons.map((n) => n.id));
      expect(ids.size).toBe(trace.neurons.length);
      for (const n of trace.neurons) expect([n.kind, n.description].every((s) => typeof s === 'string' && s)).toBe(true);
      const hops = new Map<string, number>(); // signal id -> hops, from the first event that names it
      let last = 0;
      for (const e of trace.events) {
        if (!TYPES.includes(e.type)) continue; // TRACE.md: a reader ignores event types it does not know
        expect(Number.isInteger(e.t)).toBe(true);
        expect(e.t).toBeGreaterThanOrEqual(last);
        last = e.t;
        if (e.type === 'fail') continue;
        for (const id of [e.from!, ...(e.to ?? []), ...(e.neuron ? [e.neuron] : [])]) expect(ids).toContain(id);
        if (e.type === 'drop') expect(REASONS).toContain(e.reason);
        if (e.type === 'drop' || e.type === 'error') {
          expect(hops.has(e.id!)).toBe(true); // it is about a signal already reported
          continue;
        }
        const origin = e.replyTo ?? e.cause;
        if (origin === undefined) expect(e.hops).toBe(0);
        else {
          expect(hops.has(origin)).toBe(true); // an earlier event's signal
          expect(e.hops).toBe(hops.get(origin)! + 1);
        }
        for (const link of [e.replyTo, e.cause]) if (link !== undefined) expect(hops.has(link)).toBe(true);
        hops.set(e.id!, e.hops!);
      }
    });
  }

  it('alerts: no inner id of diagnosis appears anywhere', () => {
    for (const [file, text] of raw) {
      if (!file.startsWith('alerts')) continue;
      for (const id of INNER_IDS) expect(text).not.toContain(`"${id}"`);
    }
  });
});

describe('replay rules', () => {
  const ev = (id: string, from: string, hops: number, extra = {}) => ({ t: 0, type: 'signal' as const, id, from, to: ['m'], trace: 'x', hops, ...extra });
  const base = (options: object, links: { from: string; to: string; strength?: number }[] = []) => ({
    neurons: [{ id: 'a', kind: 'js', description: '' }, { id: 'b', kind: 'js', description: '' }, { id: 'm', kind: 'js', description: '', options }],
    links,
  });

  it('does not hold an input beyond the receiver maxHops, or one sent with send', () => {
    const trace = { ...base({ threshold: 1, maxHops: 1 }), events: [ev('1', 'a', 2), ev('2', 'a', 1, { via: 'send' })] };
    const state = replay(trace);
    expect(state.neurons.m!.acted).toBe(0);
    expect(replay({ ...trace, events: [ev('3', 'a', 1)] }).neurons.m!.acted).toBe(1);
  });

  it('sums link strengths toward the threshold (default 1)', () => {
    const events = [ev('1', 'a', 0), ev('2', 'b', 0)];
    expect(replay({ ...base({ threshold: 2 }), events }).neurons.m!.acted).toBe(1);
    const strong = base({ threshold: 2 }, [{ from: 'a', to: 'm', strength: 2 }]);
    expect(replay({ ...strong, events: events.slice(0, 1) }).neurons.m!.acted).toBe(1);
    const weak = base({ threshold: 2 }, [{ from: 'a', to: 'm', strength: 0.5 }, { from: 'b', to: 'm', strength: 0.5 }]);
    expect(replay({ ...weak, events }).neurons.m!.acted).toBe(0);
  });
});

describe('replaying committed traces', () => {
  for (const [file, trace] of traces) {
    it(`${file} ends with the numbers its summary states`, () => {
      const state = replay(trace);
      const n = state.neurons;
      // Held inputs: every drop at a threshold neuron (but a hops refusal, or a call cut short that had already run)
      // removes an input that was really held; none are waiting at the end.
      const walk = start(trace);
      const ran = new Set<string>();
      for (const e of trace.events) {
        const heldBefore = e.type === 'drop' ? (walk.held[e.neuron!] ?? []).map((h) => h.id) : [];
        if (e.type === 'drop' && walk.thresholds[e.neuron!] !== undefined && e.reason !== 'hops' && !ran.has(e.id!)) {
          expect(heldBefore, `${file}: drop of ${e.id} at ${e.neuron}`).toContain(e.id);
        }
        if (e.type === 'drop' && e.phase !== undefined) expect(e.phase, `${file}: phase of the drop of ${e.id}`).toBe(e.reason !== 'replaced' ? undefined : heldBefore.includes(e.id!) ? 'waiting' : 'running');
        if (step(walk, e).length) ran.add(e.id!);
      }
      for (const held of Object.values(state.held)) expect(held).toHaveLength(0);
      expect(Object.values(n).reduce((a, c) => a + c.errors, 0)).toBe(0);

      const s = trace.summary as Record<string, any>;
      if (trace.demo === 'alerts') {
        const hyp = n.hypothesis!;
        const cut = hyp.acted - hyp.answers;
        expect(n.alerts!.sent).toBe(s.alerts);
        expect(n.logs!.answers + n.deploys!.answers + n.metrics!.answers).toBe(s.flags);
        expect(hyp.acted).toBe(s.calls);
        expect(hyp.answers).toBe(s.finished);
        expect(cut).toBe(s.cut);
        expect((hyp.dropped.replaced ?? 0) - cut).toBe(s.heldReplaced);
        expect(hyp.dropped.expired ?? 0).toBe(s.expired);
        expect(n.diagnosis!.answers).toBe(s.diagnoses);
        const finalVerdict = trace.events.filter((e) => e.type === 'answer' && e.from === 'diagnosis').at(-1)?.payload;
        expect(finalVerdict && 'data' in finalVerdict ? finalVerdict.data : null).toEqual(s.final);
        expect(s.threshold).toBe(trace.neurons.find((x) => x.id === 'hypothesis')!.options!.threshold);
        expect(trace.parameters!.threshold).toBe(s.threshold);
      } else {
        expect(n.box!.sent).toBe(s.keystrokes);
        if (trace.demo === 'suggest') expect(n.suggest!.acted).toBe(s.suggestRuns);
      }
    });
  }

  it('alerts variants: more agreement needed, fewer calls; every run had enough distinct detectors', () => {
    const calls = [1, 2, 3].map((k) => {
      const trace = traces.get(k === 2 ? 'alerts.json' : `alerts-threshold-${k}.json`)!;
      const state = start(trace);
      for (const e of trace.events) {
        const before = (state.held.hypothesis ?? []).filter((h) => h.trace === e.trace).map((h) => h.from);
        if (step(state, e).includes('hypothesis')) {
          expect(new Set([...before, e.from]).size).toBeGreaterThanOrEqual(k);
        }
      }
      return state.neurons.hypothesis!.acted;
    });
    expect(calls[0]).toBeGreaterThanOrEqual(calls[1]!);
    expect(calls[1]).toBeGreaterThanOrEqual(calls[2]!);
  });
});

// Nothing private or machine-specific may be in a trace. Personal names live outside the repo: list them (comma
// separated) in $TRACE_DENYLIST, or one per line in the gitignored `.trace-denylist` or in
// ~/.config/agent-neuron/trace-denylist (outside the repo), and they are checked too.
const listFile = new URL('../.trace-denylist', import.meta.url);
const outside = pathToFileURL(path.join(os.homedir(), '.config/agent-neuron/trace-denylist')); // outside the repo, so every worktree sees it
const local = [
  ...(process.env.TRACE_DENYLIST ?? '').split(','),
  ...[listFile, outside].flatMap((f) => (existsSync(f) ? readFileSync(f, 'utf8').split('\n') : [])),
  os.userInfo().username,
  os.hostname().split('.')[0]!,
]
  .map((w) => w.trim().toLowerCase())
  .filter((w) => w.length >= 3);
const patterns: [string, RegExp][] = [
  ['absolute path', /(^|[\s"'(=:])\/(?:[\w.-]+\/)+[\w.-]*/],
  ['windows path', /\b[A-Za-z]:\\/],
  ['home path', /~\//],
  ['email', /[\w.+-]+@[\w-]+\.[\w.-]+/],
  ['hostname', /\b[\w-]+\.(local|lan|internal|home|corp)\b/i],
  ['denylisted word', /dropbox|\/users\/|\/home\//i],
];
const hits = (text: string): string[] => [
  ...patterns.filter(([, re]) => re.test(text)).map(([name]) => name),
  ...local.filter((w) => text.toLowerCase().includes(w)).map(() => 'local denylist, user name or hostname'),
];

describe('trace privacy', () => {
  it('finds nothing in any committed trace', () => {
    for (const [file, text] of raw) expect({ file, hits: hits(text) }).toEqual({ file, hits: [] });
  });

  it('catches a broken copy', () => {
    const good = raw.get('typing.json')!;
    const leaks = ['/Users/someone/project/run.sh', 'someone@example.com', 'build-box.local', os.userInfo().username, os.hostname(), '~/notes'];
    for (const leak of leaks) {
      const broken = good.replace('"description": "', `"description": "${leak} `);
      expect(broken).not.toBe(good);
      expect(hits(broken).length, leak).toBeGreaterThan(0);
    }
  });
});

describe('tap', () => {
  it('writes phase on a replaced drop: waiting for a held input, running for a cut call', async () => {
    const events: TraceEvent[] = [];
    const { listeners } = tap((e) => events.push(e), [{ id: 'h', options: { threshold: 2 } }]);
    const slow: Adapter = {
      handle: (_p, _s, c?: AdapterContext) =>
        new Promise((resolve) => {
          const timer = setTimeout(() => resolve(text('done')), 200);
          c?.abort?.addEventListener('abort', () => (clearTimeout(timer), resolve(text('cut'))), { once: true });
        }),
    };
    const network = new Network(listeners);
    network.add(new Neuron('h', slow, { threshold: 2 }));
    for (const id of ['a', 'b']) (network.add(new Neuron(id, slow)), network.connect(id, 'h'));
    const a1 = network.emit('a', text('a1'), { to: ['h'], replaces: 'k' });
    network.emit('a', text('a1b'), { to: ['h'], cause: a1, replaces: 'k' }); // a1 was held: waiting
    network.emit('b', text('b1'), { to: ['h'], cause: a1 }); // crosses
    await new Promise((r) => setTimeout(r, 30));
    network.emit('a', text('a2'), { to: ['h'], cause: a1, replaces: 'k' }); // cuts the running call
    await network.idle();
    const drops = events.filter((e) => e.type === 'drop').map((e) => [e.reason, e.phase]);
    expect(drops).toEqual([['replaced', 'waiting'], ['replaced', 'running']]);
    const bare: TraceEvent[] = [];
    tap((e) => bare.push(e)).listeners.onDrop(events.find((e) => e.type === 'drop') as never, 'h', 'replaced');
    expect(bare[0]).not.toHaveProperty('phase'); // no neurons given: no guess

  });
});
