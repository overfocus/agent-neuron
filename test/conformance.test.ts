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
  it('pass on the reference engine, with none pending', async () => {
    const results = await conform(fixtures);
    expect(results.filter((r) => r.status === 'fail').map((r) => `${r.name}\n${r.detail.join('\n')}`)).toEqual([]);
    expect(results.filter((r) => r.status === 'pending')).toEqual([]);
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
    const finished = [{ type: 'finished', t: 0, trace: '1', status: 'done' }];
    expect(compare([], finished, false)).toBeUndefined();
    expect(compare([], finished, false, true)).toContain('event 1');
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
  { name: 'a person\'s empty answer forwarded as an answer, not quiet (SPEC 3.4)', edits: [["typeof text === 'string' && text !== '' ? { answer: text } : { quiet: true }", "typeof text === 'string' ? { answer: text } : { quiet: true }"]] },
  { name: 'a withdrawn question that never ends its call', edits: [["      this.desk!.withdraw(id);\n      end({ quiet: true });", "      this.desk!.withdraw(id);"]] },
  { name: 'a human call\'s timeout set before the person is asked (the answer loses the tie, SPEC 6.2 rule 7)', edits: [['else if (node.spec.human) this.person(call, node, value, done);', 'else if (node.spec.human) { arm(); this.person(call, node, value, done); }']] },
  { name: 'drops ordered before the signal', edits: [[
    "    this.report(scope, { type: 'signal', ...this.fields(sig), ...withKey(sig.replaces), payload: this.payload(sig.payload) });\n    if (sig.replaces !== undefined) this.makeStale(`${scope.id}\\0${sig.from}\\0${sig.replaces}`);\n",
    "    if (sig.replaces !== undefined) this.makeStale(`${scope.id}\\0${sig.from}\\0${sig.replaces}`);\n    this.report(scope, { type: 'signal', ...this.fields(sig), ...withKey(sig.replaces), payload: this.payload(sig.payload) });\n"]] },
  { name: 'threshold counting the same sender twice', edits: [['if (older) {', 'if (false as boolean) {'], ['new Set(held.map((h) => h.sig.from)).size < k', 'held.length < k']] },
  { name: 'a key that reaches only the links the newer signal took (SPEC 5.4, decision 0017)', edits: [
    ['    if (sig.replaces !== undefined) this.makeStale(`${scope.id}\\0${sig.from}\\0${sig.replaces}`);\n    for (const to of sig.to)', '    if (sig.replaces !== undefined) this.makeStale(`${scope.id}\\0${sig.from}\\0${sig.replaces}`, sig.to);\n    for (const to of sig.to)'],
    ['  private makeStale(key: string) {\n    for (const w of this.stale.get(key) ?? []) {', '  private makeStale(key: string, only: string[] = []) {\n    for (const w of this.stale.get(key) ?? []) {\n      if (!only.includes(w.node.spec.id)) continue;'],
  ] },
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
  { name: 'a cut call releasing its reservation instead of committing', edits: [['if (!call.reserved) return;', 'if (!call.reserved || call.cancelled) return;']] },
  { name: 'a held input reserving', edits: [['  private hold(w: Work, k: number) {\n', '  private hold(w: Work, k: number) {\n    this.reserve(new Call([w]));\n']] },
  { name: 'branches drawing from the pool out of order', edits: [['for (const to of sig.to) this.deliver(', 'for (const to of [...sig.to].reverse()) this.deliver(']] },
  { name: 'the cost of a composed neuron not capping its inside', edits: [['innerPool(node.spec.cost, inner.net.budget)); // the cost caps the inside (SPEC 10.5)', 'undefined);']] },
  { name: 'a queued level 1 call reserving when it queues', edits: [
    ['      const refused = this.run.reserve(job.call); // checked when the slot frees (SPEC 10)\n      if (refused) { job.done(refused); continue; }\n', ''],
    ['      node.server.submit({ call, message:', '      const early = this.reserve(call);\n      if (early) return done(early);\n      node.server.submit({ call, message:']] },
  { name: 'one pool shared by every trace', edits: [['scope.pools.has(trace)', "scope.pools.has('')"], ['scope.pools.set(trace,', "scope.pools.set('',"], ['return scope.pools.get(trace)!;', "return scope.pools.get('')!;"]] },
  { name: 'a trace that never reports exhausted', edits: [["pool?.exhausted ? 'exhausted' : 'done'", "'done'"]] },
  { name: 'a refused call keeping part of the cost', edits: [['      pool.exhausted = true;\n', '      pool.exhausted = true;\n      for (const [unit, n] of Object.entries(cost)) if (unit !== short[0]) pool.left[unit] = (pool.left[unit] ?? 0) - n;\n']] },
  { name: 'ms counted from the start of the run', edits: [['call.t0 = host.now();', 'call.t0 = 0;']] },
  { name: 'a commit labelled measured', edits: [["basis: 'estimated'", "basis: 'measured'"]] },
  { name: 'an inner commit note in the outer trace', edits: [["this.report(w.scope, { type: 'note', neuron: w.node.id, what: 'commit'", "this.report(this.rootScope, { type: 'note', neuron: w.node.id, what: 'commit'"]] },
  { name: 'an injection that cannot lower the budget', edits: [['if (pool) pool.left[unit] = n;', 'if (pool) pool.left[unit] = pool.left[unit]!;']] },
  { name: 'a trace finished while its signal is still being delivered', edits: [['this.open.set(id, 1); // the injection itself', 'this.open.set(id, 0); // the injection itself']] },
  { name: 'the term note after the end of the process', edits: [["      note('term'); // before the kill: where the process ends at once, the call's end follows the note\n      killGroup(pid, 'SIGTERM');\n", "      killGroup(pid, 'SIGTERM');\n      note('term');\n"]] },
  { name: 'an engine that does not check its file', edits: [["const errors = check(file).filter((f) => f.severity === 'error');", 'const errors: { message: string }[] = [];']] },
  { name: 'an injection that raises the budget accepted', edits: [['(n as number) > ceiling[unit]!', 'false as boolean']] },
  { name: 'the inner budget not bounding a composed neuron with no cost', edits: [['?? n)])) : own;', '?? n)])) : undefined;']] },
  { name: 'the inner budget not lowering the cost of a composed neuron', edits: [['Math.min(n, own?.[unit] ?? n)', 'n']] },
  { name: 'a unit only the inner budget has granted to a composed neuron', edits: [['cost ? Object.fromEntries(', 'cost ? { ...own, ...Object.fromEntries('], ['?? n)])) : own;', '?? n)])) } : own;']] },
  { name: 'the kill note after the end of the process', edits: [["note('kill'); // before the kill, as for term: where the process ends at once, the call's end follows the note\n          killGroup(pid, 'SIGKILL');", "killGroup(pid, 'SIGKILL');\n          note('kill');"]] },
  { name: 'a stale input cutting an effect call', edits: [["        if (w.node.spec.effect && !w.call.queued) { w.call.stale = true; continue; } // a started effect call is never cut (SPEC 11.2)\n", '']] },
  { name: 'a queued effect call kept when its input goes stale', edits: [['w.node.spec.effect && !w.call.queued', 'w.node.spec.effect']] },
  { name: 'only the crossing input making a threshold effect call stale', edits: [['{ w.call.stale = true; continue; }', '{ if (w === w.call.crossing) w.call.stale = true; continue; }']] },
  { name: 'the answer of a stale effect call forwarded', edits: [[`const named = 'answer' in o && !stale;`, `const named = 'answer' in o;`]] },
  { name: 'the answer of a stale effect call not marked', edits: [['...(stale && { forwarded: false }), ', '']] },
  { name: 'the answer of a stale effect call discarded', edits: [['if (!call.cancelled) this.reply(call.crossing, o);', 'if (!call.cancelled && !call.stale) this.reply(call.crossing, o);']] },
  { name: 'a stale effect call not committing its cost', edits: [['if (!call.reserved) return;', 'if (!call.reserved || call.stale) return;']] },
  { name: 'a failed effect call recorded as an error', edits: [["type: unknown ? 'unknown' : 'error'", "type: 'error'"]] },
  { name: 'the error of an effect neuron recorded as unknown', edits: [["KNOWN_FAILURES = ['budget', 'start', 'neuron']", "KNOWN_FAILURES = ['budget', 'start']"]] },
  { name: 'a refused effect call recorded as unknown', edits: [["KNOWN_FAILURES = ['budget', 'start', 'neuron']", "KNOWN_FAILURES = ['start', 'neuron']"]] },
  { name: 'only a crash and a timeout of an effect call unknown', edits: [['!KNOWN_FAILURES.includes(o.code)', "['timeout', 'crash'].includes(o.code)"]] },
  { name: 'an effect call that times out left running', edits: [['        this.cancel(call);\n      }, ms);', '        if (!node.spec.effect) this.cancel(call);\n      }, ms);']] },
  { name: 'the stream consumer called inside the engine', edits: [['    this.pending.push(structuredClone(e));\n    if (this.pending.length === 1) queueMicrotask(() => this.flush());', '    this.onEvent(structuredClone(e));']] },
  { name: 'the stream handing over the engine\'s own events', edits: [['this.pending.push(structuredClone(e));', 'this.pending.push(e);']] },
  { name: 'the stream out of order', edits: [['for (let e = this.pending.shift(); e; e = this.pending.shift()) {', 'for (let e = this.pending.pop(); e; e = this.pending.pop()) {']] },
  { name: 'the stream without finished events', edits: [['    if (!this.onEvent) return;\n', "    if (!this.onEvent || e.type === 'finished') return;\n"]] },
  { name: 'the stream without notes', edits: [['    if (!this.onEvent) return;\n', "    if (!this.onEvent || e.type === 'note') return;\n"]] },
  { name: 'an observer that is not shown finished', edits: [["if (typeof e.id !== 'string' && e.type !== 'finished') return;", "if (typeof e.id !== 'string') return;"]] },
  { name: 'the trigger of an observer injection after finished', edits: [['trigger: e.id ?? e.trace }', 'trigger: e.id }']] },
  { name: 'a program that could not be started recorded as unknown', edits: [['o.started !== false && ', '']] },
  { name: 'an error code renamed', edits: [["code: 'neuron' }", "code: 'error' }"]] },
  { name: 'a ground write that does not age the value (4.5)', edits: [['const v = (e ? e.v * Math.exp(-(this.ground!.evaporate ?? 0) * (t - e.at) / 1000) : 0) + amount;', 'const v = (e ? e.v : 0) + amount;']] },
  { name: 'a ground read that does not age the value (4.5)', edits: [['    return e ? e.v * Math.exp(-(this.ground!.evaporate ?? 0) * (t - e.at) / 1000) : 0;', '    return e ? e.v : 0;']] },
  { name: 'a ground generator that never advances (4.5)', edits: [['this.gen = (this.gen + 0x6D2B79F5) | 0; // 32-bit wrap add', 'this.gen = this.gen;']] },
  { name: 'a ground draw over the links in reverse order (4.5)', edits: [['    for (let i = 0; i < node.out.length; i++) if ((acc += weights[i]!) > u * total) return [node.out[i]!];', '    for (let i = node.out.length - 1; i >= 0; i--) if ((acc += weights[i]!) > u * total) return [node.out[i]!];']] },
  { name: 'a ground deposit.by that filters nobody (4.5)', edits: [['if (d.by !== undefined && d.by !== node.id) return;', 'if (false as boolean) return;']] },
  { name: 'a ground event without its cause (4.5)', edits: [["{ type: 'ground', place, v, cause: replyId }", "{ type: 'ground', place, v }"]] },
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
