import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { afterAll, describe, expect, it } from 'vitest';
import { compare, conform, events, fixtureDirs, normalise, report, type Fixture } from '../src/engine/conform.js';
import { runNetwork } from '../src/engine/index.js';

const fixtures = fileURLToPath(new URL('../spec/fixtures/', import.meta.url));
const enginePath = fileURLToPath(new URL('../src/engine/engine.ts', import.meta.url));
const mutants = fileURLToPath(new URL('./.mutants/', import.meta.url));
const load = (dir: string) => JSON.parse(readFileSync(join(dir, 'fixture.json'), 'utf8')) as Fixture;

describe('the conformance fixtures (docs/CONFORMANCE.md)', () => {
  it('pass on the reference engine, and the pending ones are only the v0.1 budget', async () => {
    const results = await conform(fixtures);
    expect(results.filter((r) => r.status === 'fail').map((r) => `${r.name}\n${r.detail.join('\n')}`)).toEqual([]);
    expect(results.filter((r) => r.status === 'pending').every((r) => r.name.startsWith('budget/'))).toBe(true);
    expect(results.filter((r) => r.status === 'pass').length).toBeGreaterThan(100);
  }, 60_000);

  it('give identical events on five runs', async () => {
    const dirs = fixtureDirs(fixtures).filter((d) => !load(d).pending && !load(d).refuses);
    const once = async () => {
      const all: unknown[] = [];
      for (const d of dirs) all.push(await events(runNetwork, d, load(d), true)); // one virtual run at a time
      return JSON.stringify(all);
    };
    const first = await once();
    for (let i = 0; i < 4; i++) expect(await once()).toBe(first);
  }, 60_000);

  it('normalise ids by first appearance and compare error messages by prefix', () => {
    const e = [{ type: 'signal', t: 0, id: '7', trace: '7' }, { type: 'quiet', t: 1, id: '9', trace: '7', replyTo: '7' }];
    expect(normalise(e).map((x) => [x.id, x.trace, x.replyTo])).toEqual([['#1', '#1', undefined], ['#2', '#1', '#1']]);
    const err = (message: string) => [{ type: 'error', t: 1, id: '1', trace: '1', code: 'exit', message }];
    expect(compare(err('exit 1:'), err('exit 1: boom'), false)).toBeUndefined();
    expect(compare(err('exit 2:'), err('exit 1: boom'), false)).toContain('event 1');
    expect(compare([], [{ type: 'note', t: 0, neuron: 'a', what: 'x' }], false)).toBeUndefined();
    expect(compare([], [{ type: 'note', t: 0, neuron: 'a', what: 'x' }], true)).toContain('event 1');
  });

  it('lets real runs overlap, and refuses a virtual run while one is active', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'conf-'));
    const file = join(dir, 'n.network.json');
    writeFileSync(file, JSON.stringify({ version: 0, neurons: [{ id: 's', source: true }, { id: 'a', run: ['sh', '-c', 'sleep 0.3; echo 1'] }], links: [{ from: 's', to: 'a' }] }));
    const inj = [{ at: 0, source: 's', payload: 1 }];
    const both = await Promise.all([runNetwork(file, inj), runNetwork(file, inj)]);
    expect(both.map((t) => t.events.filter((e) => e.type === 'answer').length)).toEqual([1, 1]);
    const real = runNetwork(file, inj);
    const one = join(fixtures, 'routing/chain-forward-with-cause');
    await expect(events(runNetwork, one, load(one), true)).rejects.toThrow('needs the engine to itself');
    await real;
  }, 20_000);

  it('does not count just any exception as a refusal to start', async () => {
    const boom = (() => Promise.reject(new TypeError('x'))) as unknown as typeof runNetwork;
    const r = await conform(fixtures, { only: 'level1/first-line-must-be-ready', engine: boom });
    expect(r.map((x) => x.status)).toEqual(['fail']);
  });

  it('treats a crash that is not a signal name as a harness error', async () => {
    const dir = join(fixtures, 'failures/l0-killed-by-a-named-signal');
    const f = load(dir);
    f.script = { a: { calls: [{ after: 10, reply: { crash: 'USR1' } }] } };
    await expect(events(runNetwork, dir, f, true)).rejects.toThrow('signal name');
  });

  it('print pass, fail and pending lines and a summary', () => {
    expect(report([{ name: 'a/b', status: 'pass', detail: [] }, { name: 'c', status: 'fail', detail: ['x\ny'] }, { name: 'd', status: 'pending', detail: ['why'] }]))
      .toEqual(['pass a/b', 'fail c', '  x', '  y', 'pending d: why', '1 passed, 1 failed, 1 pending']);
  });
});

// Each mutant is the engine with one deliberate bug. The fixtures must catch every one: that is what shows they bite.
const mutations: { name: string; edits: [from: string, to: string, count?: number][] }[] = [
  { name: 'a forward without cause', edits: [['cause: reply.id, ', '']] },
  { name: 'drops ordered before the signal', edits: [[
    "    this.report(scope, { type: 'signal', ...this.fields(sig), ...withKey(sig.replaces), payload: this.payload(sig.payload) });\n    if (sig.replaces !== undefined) this.makeStale(`${scope.id}\\0${sig.from}\\0${sig.replaces}`);\n",
    "    if (sig.replaces !== undefined) this.makeStale(`${scope.id}\\0${sig.from}\\0${sig.replaces}`);\n    this.report(scope, { type: 'signal', ...this.fields(sig), ...withKey(sig.replaces), payload: this.payload(sig.payload) });\n"]] },
  { name: 'threshold counting the same sender twice', edits: [['if (older) {', 'if (false as boolean) {'], ['new Set(held.map((h) => h.sig.from)).size < k', 'held.length < k']] },
  { name: 'no cut by an earlier input of a running call (issue 17)', edits: [['if (w.call.cancelled || w.call.finished) continue;', 'if (w.call.cancelled || w.call.finished || w !== w.call.crossing) continue;']] },
  { name: 'an inner event leaking into the outer trace', edits: [['this.inner && ((e) => this.inner!(node.path, e))', '(e) => this.events.push(e)']] },
  { name: 'a reply with the same hops as its input', edits: [['to: [sig.from], hops: sig.hops + 1, replyTo', 'to: [sig.from], hops: sig.hops, replyTo']] },
  { name: 'maxHops off by one', edits: [['sig.hops > node.spec.maxHops', 'sig.hops >= node.spec.maxHops']] },
  { name: 'a replaced held input not reported', edits: [["this.drop(older, 'replaced', 'waiting');", '']] },
  { name: 'a queued drop reported as running', edits: [["w.call.queued ? 'waiting' : 'running'", "'running'"]] },
  { name: 'the key not inherited by a level 1 answer', edits: [['const key = o.replaces === undefined ? sig.replaces : (o.replaces ?? undefined);', 'const key = o.replaces ?? undefined;']] },
  { name: 'the envelope at threshold 1', edits: [['(node.spec.threshold ?? 1) > 1', '(node.spec.threshold ?? 1) >= 1', 2]] },
  { name: 'consumed inputs still expiring', edits: [['host.clearTimeout(h.timer);', '']] },
  { name: 'a reply "to" in the wrong order', edits: [['node.out.filter((n) => !o.to || o.to.includes(n.id))', '[...node.out].reverse().filter((n) => !o.to || o.to.includes(n.id))']] },
  { name: 'exit reply not cancelling the rest of the inside', edits: [['this.cancelScope(scope);', '']] },
  { name: 'junk failing a call whenever any runs', edits: [['if (this.jobs.size !== 1) return;', 'if (this.jobs.size === 0) return;']] },
  { name: 'a constant restart delay', edits: [['Math.min(100 * 2 ** this.failedStarts, 5000)', '100']] },
  { name: 'no early-crash rule', edits: [['const proven = wasReady && !unproven && host.now() - this.readyAt >= STABLE_AFTER;', 'const proven = wasReady && !unproven;']] },
  { name: 'a pong not clearing the failed count', edits: [['this.failedStarts = 0; // it answers: this start has proved itself', '']] },
  { name: 'failed after 6 starts', edits: [['MAX_STARTS = 5', 'MAX_STARTS = 6']] },
  { name: 'a cancelled call giving up its slot', edits: [["this.write({ type: 'cancel', re });", "this.write({ type: 'cancel', re }); this.jobs.delete(re);"]] },
  { name: 'staleness shared across calls of a composed neuron', edits: [['`${scope.id}\\0${sig.from}\\0${sig.replaces}`', '`${sig.from}\\0${sig.replaces}`', 2]] },
  { name: 'threshold groups shared across calls of a composed neuron', edits: [['private group = (w: Work) => `${w.scope.id}\\0${w.sig.trace}`;', 'private group = (w: Work) => w.sig.trace;']] },
  { name: 'timeout counted from the queue', edits: [['node.server.submit({ call, message:', 'arm(); node.server.submit({ call, message:']] },
  { name: 'a late answer for cancelled work reported', edits: [['if (!call.cancelled) this.reply(call.crossing, o);', 'this.reply(call.crossing, o);']] },
  { name: 'a wrong default within', edits: [['DEFAULT_WITHIN = 10_000', 'DEFAULT_WITHIN = 5_000']] },
  { name: 'a short kill grace', edits: [['KILL_AFTER = 2_000', 'KILL_AFTER = 1_000']] },
  { name: 'a level 0 timeout winning a reply due at the same instant', edits: [['    if (!call.finished) arm();\n', ''], ['    if (node.inner) this.compose(call, node, value, done);', '    arm();\n    if (node.inner) this.compose(call, node, value, done);']] },
  { name: 'a level 1 timeout winning a reply due at the same instant', edits: [["this.write({ type: 'signal', re, ...job.message });\n      job.arm();", "job.arm();\n      this.write({ type: 'signal', re, ...job.message });"]] },
  { name: 'a phase on an expired drop', edits: [["this.drop(w, 'expired');", "this.drop(w, 'expired', 'waiting');"]] },
  { name: 'a life of exactly 1000 ms not proven', edits: [['host.now() - this.readyAt >= STABLE_AFTER', 'host.now() - this.readyAt > STABLE_AFTER']] },
  { name: 'junk not counting a cancelled call that holds a slot', edits: [['if (this.jobs.size !== 1) return;', 'if ([...this.jobs.values()].filter((j) => !j.call.cancelled).length !== 1) return;']] },
  { name: 'a TypeError while a start is refused', edits: [['this.begin.no(Object.assign(', '(undefined as unknown as { x: { y: number } }).x.y; this.begin.no(Object.assign(']] },
  { name: 'a level 0 kill note before the error', edits: [["      if (over) note('kill'); // the failure first, then the note of the kill that caused it (SPEC 6.3)\n", ''], ["        over = killed = true;\n        killGroup(pid, 'SIGKILL');", "        over = killed = true;\n        note('kill');\n        killGroup(pid, 'SIGKILL');"]] },
  { name: 'a timeout going last among the wake-ups of its instant', edits: [
    ['      timer = host.setTimeout(() => {\n        if (call.cancelled || call.finished) return;', '      timer = host.setTimeout(() => host.setTimeout(() => {\n        if (call.cancelled || call.finished) return;'],
    ['      }, ms);\n    };\n    const done', '      }, 0), ms);\n    };\n    const done']] },
  { name: 'a signal number instead of its name', edits: [['`signal ${status.signal}: ${why}`', '`signal ${({ SIGUSR1: 30 } as Record<string, number>)[status.signal] ?? status.signal}: ${why}`']] },
  { name: 'no kill for a survivor of a cut level 0 program', edits: [['        if (alive(pid)) {\n          killed = true;', '        if (alive(pid) && !exited) {\n          killed = true;']] },
  { name: 'no kill for a survivor of a level 1 process', edits: [['too\n      if (alive(pid)) {', 'too\n      if (false as boolean) {']] },
  { name: 'a crashed process grace not keeping the run open', edits: [['    this.reaping.push(groupGone(pid, KILL_AFTER).then(', '    void (groupGone(pid, KILL_AFTER).then(']] },
  { name: 'a pending restart delay keeping the run open', edits: [[
    "    this.restartTimer = host.setTimeout(() => {\n      this.run.processNote(this.node, 'restart');\n      this.spawn();\n    },",
    "    this.run.live++;\n    this.restartTimer = host.setTimeout(() => {\n      this.run.processNote(this.node, 'restart');\n      this.spawn();\n      if (--this.run.live === 0) this.run.over?.();\n    },"]] },
  { name: 'a composed reply without the key of exit', edits: [['      this.cancelScope(scope);\n      done(o);', "      this.cancelScope(scope);\n      done('answer' in o ? { answer: o.answer } : o);"]] },
  { name: 'an error code renamed', edits: [["code: 'neuron' }", "code: 'error' }"]] },
];

describe('mutation check: every broken engine fails at least one fixture', () => {
  afterAll(() => rmSync(mutants, { recursive: true, force: true }));
  const source = readFileSync(enginePath, 'utf8');
  it.each(mutations)('$name', async ({ name, edits }) => {
    let text = source;
    for (const [from, to, count = 1] of edits) {
      expect(text.split(from).length - 1, `"${from}" must occur ${count} time(s) in engine.ts`).toBe(count);
      text = text.split(from).join(to);
    }
    text = text.replace("from '../index.js'", "from '../../src/index.js'").replace("from './check.js'", "from '../../src/engine/check.js'");
    mkdirSync(mutants, { recursive: true });
    const file = join(mutants, `${name.replace(/\W+/g, '-')}.ts`);
    writeFileSync(file, text);
    // The mutant shares the virtual host with the real engine, but has its own copy of the engine code and its own `host`.
    const mutant = (await import(pathToFileURL(file).href)) as { runNetwork: typeof runNetwork };
    const results = await conform(fixtures, { engine: mutant.runNetwork, bail: !process.env.MUTANTS });
    const caught = results.find((r) => r.status === 'fail');
    if (process.env.MUTANTS) console.log(`${name} -> ${results.filter((r) => r.status === 'fail').map((r) => r.name).join(', ')}`);
    expect(caught, `no fixture caught: ${name}`).toBeDefined();
  }, 30_000);
});
