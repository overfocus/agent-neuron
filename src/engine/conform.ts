// The conformance runner (docs/CONFORMANCE.md): runs every fixture under a directory on a virtual clock and compares events.
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { basename, join, relative } from 'node:path';
import { isDeepStrictEqual } from 'node:util';
import { runNetwork, type Event, type Injection, type Observer } from './engine.js';
import { VirtualHost, type Script } from './virtual.js';

type Rule = { when: Record<string, unknown>; inject: { after: number; source: string; payload: unknown; replaces?: string } };
export type Fixture = {
  fixture: 0; description: string; network?: string; notes?: boolean; pending?: string; pingEvery?: number; refuses?: boolean;
  inputs: Injection[]; script?: Script; observer?: Rule[]; expect: Event[]; expectWithout?: Event[]; independent?: boolean;
};
export type Result = { name: string; status: 'pass' | 'fail' | 'pending'; detail: string[] };
type Engine = typeof runNetwork;
const KNOWN = ['fixture', 'description', 'network', 'notes', 'pending', 'pingEvery', 'refuses', 'inputs', 'script', 'observer', 'expect', 'expectWithout', 'independent'];

/** Ids become #1, #2, ... by first appearance, scanning id, trace, replyTo, cause (and a note's trigger) of each event in turn (8.3). */
export function normalise(events: Event[]): Event[] {
  const ids = new Map<string, string>();
  const name = (v: unknown) => (typeof v === 'string' ? (ids.get(v) ?? (ids.set(v, `#${ids.size + 1}`), ids.get(v)!)) : v);
  return events.map((e) => {
    const out: Event = { ...e };
    for (const k of ['id', 'trace', 'replyTo', 'cause', 'trigger']) if (k in out) out[k] = name(out[k]);
    return out;
  });
}

/** The first difference between expected and actual events (notes only if asked), or undefined. `via` absent equals "emit"; an error's message is a prefix. */
export function compare(expected: Event[], actual: Event[], notes: boolean): string | undefined {
  const keep = (list: Event[]) => normalise(list).filter((e) => notes || e.type !== 'note').map(({ via, ...e }) => (via === 'emit' || via === undefined ? e : { ...e, via }));
  const want = keep(expected), got = keep(actual);
  for (let i = 0; i < Math.max(want.length, got.length); i++) {
    const w = want[i], g = got[i];
    const same = w && g && (w.type === 'error' && typeof w.message === 'string' && typeof g.message === 'string'
      ? g.message.startsWith(w.message) && isDeepStrictEqual({ ...w, message: 0 }, { ...g, message: 0 })
      : isDeepStrictEqual(w, g));
    if (!same) return `event ${i + 1}: expected ${w ? JSON.stringify(w) : '(none)'}\n         got      ${g ? JSON.stringify(g) : '(none)'}`;
  }
}

export const fixtureDirs = (dir: string): string[] =>
  existsSync(join(dir, 'fixture.json')) ? [dir] : readdirSync(dir, { withFileTypes: true }).filter((d) => d.isDirectory()).sort((a, b) => (a.name < b.name ? -1 : 1)).flatMap((d) => fixtureDirs(join(dir, d.name)));

/** An observer that reads the events and injects as its rules say, each rule once (SPEC 9). */
const observer = (rules: Rule[]): Observer => {
  const fired = new Set<Rule>();
  return (e) => rules.filter((r) => !fired.has(r) && Object.entries(r.when).every(([k, v]) => isDeepStrictEqual(e[k], v))).map((r) => (fired.add(r), { at: r.inject.after, source: r.inject.source, payload: r.inject.payload, ...(r.inject.replaces !== undefined && { replaces: r.inject.replaces }) }));
};

/** The events one run of the fixture in `dir` produces (all of them, notes included). */
export async function events(engine: Engine, dir: string, f: Fixture, withObserver: boolean): Promise<Event[]> {
  const trace = await engine(join(dir, f.network ?? 'network.json'), f.inputs, {
    host: new VirtualHost(f.script ?? {}), pingEvery: f.pingEvery ?? 10_000, ...(withObserver && f.observer && { observer: observer(f.observer) }),
  });
  return trace.events;
}

async function one(engine: Engine, dir: string, name: string): Promise<Result> {
  const f = JSON.parse(readFileSync(join(dir, 'fixture.json'), 'utf8')) as Fixture;
  const unknown = Object.keys(f).filter((k) => !KNOWN.includes(k));
  if (f.fixture !== 0 || unknown.length) return { name, status: 'fail', detail: [`harness: fixture format ${f.fixture}, unknown fields: ${unknown.join(', ') || 'none'}`] };
  if (f.pending) return { name, status: 'pending', detail: [f.pending] };
  const fail = (...detail: string[]): Result => ({ name, status: 'fail', detail });
  try {
    if (f.refuses) {
      try { await events(engine, dir, f, true); } catch (e) { if (!(e as { refusal?: boolean }).refusal) throw e; return { name, status: 'pass', detail: [] }; } // only a refusal to start counts, not any exception
      return fail('the engine started, but the fixture says it must refuse to start');
    }
    const actual = await events(engine, dir, f, true);
    const diff = compare(f.expect, actual, f.notes ?? false);
    if (diff) return fail(diff);
    if (f.observer) { // SPEC 8.5: take the observer away and rerun
      const without = await events(engine, dir, f, false);
      const wantWithout = f.expectWithout ?? (f.observer.length ? undefined : f.expect);
      const d = wantWithout && compare(wantWithout, without, f.notes ?? false);
      if (d) return fail(`without the observer, ${d}`);
      const at = actual.findIndex((e) => e.type === 'note' && e.what === 'inject'); // before its first injection nothing may differ
      const early = compare(without.slice(0, at < 0 ? actual.length : at), actual.slice(0, at < 0 ? actual.length : at), true);
      if (early) return fail(`the observer changed events before its first injection: ${early}`);
      if (f.independent || at < 0) { // its injections only add events of their own traces
        const rooted = new Set(actual.flatMap((e, i) => (e.type === 'note' && e.what === 'inject' ? [actual[i + 1]!.trace as string] : [])));
        const rest = actual.filter((e) => e.type !== 'note' && !rooted.has(e.trace as string)); // notes take no part: they name no trace
        const same = compare(without, rest, false);
        if (same) return fail(`removing the observer changed the other traces: ${same}`);
      }
    }
    return { name, status: 'pass', detail: [] };
  } catch (e) {
    return fail(`harness: ${(e as Error).message}`);
  }
}

/** Runs the fixtures under `dir` (those whose path contains `only`, if given). */
export async function conform(dir: string, options: { only?: string; engine?: Engine; bail?: boolean } = {}): Promise<Result[]> {
  const results: Result[] = [];
  for (const d of fixtureDirs(dir)) {
    const name = d === dir ? basename(dir) : relative(dir, d).split('\\').join('/');
    if (options.only && !name.includes(options.only)) continue;
    results.push(await one(options.engine ?? runNetwork, d, name));
    if (options.bail && results.at(-1)!.status === 'fail') break;
  }
  return results;
}

/** The lines the CLI prints: `pass NAME`, `fail NAME` with indented detail, `pending NAME: reason`, then a summary. */
export function report(results: Result[]): string[] {
  const n = (s: Result['status']) => results.filter((r) => r.status === s).length;
  return [
    ...results.flatMap((r) => (r.status === 'pending' ? [`pending ${r.name}: ${r.detail[0]}`] : [`${r.status === 'pass' ? 'pass' : 'fail'} ${r.name}`, ...r.detail.flatMap((d) => d.split('\n').map((l) => `  ${l}`))])),
    `${n('pass')} passed, ${n('fail')} failed, ${n('pending')} pending`,
  ];
}
