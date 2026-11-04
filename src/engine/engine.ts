// The reference engine: runs a network file (docs/SPEC.md) and returns a trace (docs/TRACE.md, format v1).
// Level 0 `run` neurons, `source` and composed `network` neurons. Not here: serve, http, human (SPEC 3.2 to 3.4).
//
// Vocabulary as in the spec: an input is delivered to a neuron as a Work; a threshold group turns held works into one
// Call; a Call ends in an Outcome (answer, quiet, error). Everything is one synchronous event loop, so the order of the
// events is the order of the code below (SPEC 6.2). A composed neuron's inside is another Instance, run under its own
// Scope: its events go nowhere unless the caller asks (sealing, SPEC 2.6).
import { spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { basename, dirname, resolve } from 'node:path';
import { VERSION } from '../index.js';
import { check, formatFinding, readNetwork, type NeuronSpec } from './check.js';

export type Injection = { at: number; source: string; payload: unknown; replaces?: string };
export type Event = { type: string; t: number; [field: string]: unknown };
export type Trace = {
  format: 'agent-neuron-trace'; version: 1; demo: string; description: string; library: string; engine: string;
  neurons: { id: string; kind: string; description: string; options?: Record<string, number> }[];
  links: { from: string; to: string }[]; events: Event[]; summary: Record<string, unknown>;
};

type Sig = { id: string; trace: string; from: string; to: string[]; hops: number; payload?: unknown; replyTo?: string; cause?: string; replaces?: string };
type Outcome = { answer: unknown } | { quiet: true } | { error: string; code: string };

const DEFAULT_WITHIN = 10_000, DEFAULT_TIMEOUT = 30_000, MAX_OUTPUT = 1 << 20, KILL_AFTER = 2_000, MAX_KEY = 256, PAYLOAD_CAP = 2000;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const withKey = (replaces: string | undefined) => (replaces === undefined ? {} : { replaces });

/** One use of a network file: its neurons, and for a composed neuron its own inner Instance. */
class Instance {
  nodes = new Map<string, Node>();
  constructor(readonly file: string, readonly net: ReturnType<typeof readNetwork>, path: string) {
    for (const spec of net.neurons) this.nodes.set(spec.id, new Node(spec, dirname(file), path ? `${path}/${spec.id}` : spec.id));
    for (const l of net.links ?? []) this.nodes.get(l.from)!.out.push(this.nodes.get(l.to)!);
  }
}
class Node {
  out: Node[] = [];
  groups = new Map<string, Work[]>(); // threshold: trace -> held works
  inner?: Instance;
  constructor(readonly spec: NeuronSpec, readonly dir: string, readonly path: string) {
    if (spec.network) this.inner = new Instance(resolve(dir, spec.network), readNetwork(resolve(dir, spec.network)), path);
  }
  get id() { return this.spec.id; }
}
/** Where work happens: the outer network, or one call into a composed neuron (its own trace, its own staleness). */
class Scope {
  works = new Set<Work>(); // unfinished work
  failed = false; // an inner neuron failed
  onExit?: (o: Outcome) => void; onIdle?: () => void;
  constructor(readonly id: string, readonly inst: Instance, readonly report?: (e: Event) => void) {}
}
/** One input at one neuron, from delivery until its reply, drop or cancellation. */
class Work {
  timer?: NodeJS.Timeout; held = false; call?: Call;
  constructor(readonly sig: Sig, readonly node: Node, readonly scope: Scope) {}
}
/** One run of a program for the works that met the threshold; the last is the one that crossed. */
class Call {
  cancelled = false;
  stop: () => void = () => {};
  constructor(readonly inputs: Work[]) {}
  get crossing() { return this.inputs.at(-1)!; }
}

class Run {
  t0 = performance.now();
  seq = 0;
  live = 1; // unfinished works, plus the injection loop; the run is over at 0 (SPEC 6.2 rule 6)
  stale = new Map<string, Work[]>(); // (scope, sender, key) -> unfinished works, in creation order
  events: Event[] = [];
  rootScope: Scope;
  constructor(readonly root: Instance, readonly inner?: (path: string, e: Event) => void) {
    this.rootScope = new Scope('', root, (e) => this.events.push(e));
  }

  now = () => Math.round(performance.now() - this.t0);
  id = () => String(++this.seq);
  report(scope: Scope, e: { type: string; [field: string]: unknown }) {
    const { type, ...rest } = e;
    scope.report?.({ type, t: this.now(), ...rest });
  }
  fields = (s: Sig) => ({
    id: s.id, from: s.from, to: s.to, trace: s.trace,
    ...(s.replyTo !== undefined && { replyTo: s.replyTo }), ...(s.cause !== undefined && { cause: s.cause }), hops: s.hops,
  });
  /** A payload as the trace writes it; longer than 2000 characters, it is cut to a preview (TRACE.md). */
  private payload = (p: unknown) => {
    const raw = typeof p === 'string' ? p : JSON.stringify(p);
    if (raw.length <= PAYLOAD_CAP) return typeof p === 'string' ? { type: 'text', text: p } : { type: 'json', data: p };
    return typeof p === 'string' ? { type: 'text', text: raw.slice(0, PAYLOAD_CAP), truncated: true } : { type: 'json', preview: raw.slice(0, PAYLOAD_CAP), truncated: true };
  };
  private note(w: Work, what: string) { this.report(w.scope, { type: 'note', neuron: w.node.id, what }); }

  // ---- routing (SPEC 4) ----
  /** Reports a signal, makes older work with its (sender, key) stale, then delivers it to each target. */
  send(sig: Sig, scope: Scope) {
    this.report(scope, { type: 'signal', ...this.fields(sig), ...withKey(sig.replaces), payload: this.payload(sig.payload) });
    if (sig.replaces !== undefined) this.makeStale(`${scope.id}\0${sig.from}\0${sig.replaces}`);
    for (const to of sig.to) this.deliver(scope.inst.nodes.get(to)!, sig, scope);
  }

  private makeStale(key: string) {
    for (const w of this.stale.get(key) ?? []) {
      if (!w.scope.works.has(w)) continue; // finished
      if (w.call) { // running (any input of a threshold call cuts it; one drop, for the crossing input)
        if (w.call.cancelled) continue;
        this.drop(w.call.crossing, 'replaced', 'running');
        this.cancel(w.call);
      } else if (w.held) {
        this.ungroup(w);
        this.drop(w, 'replaced', 'waiting');
        this.retire(w);
      }
    }
    this.stale.delete(key);
  }

  private drop(w: Work, reason: string, phase?: string) {
    this.report(w.scope, { type: 'drop', ...this.fields(w.sig), neuron: w.node.id, reason, ...(phase && { phase }), payload: this.payload(w.sig.payload) });
  }

  private deliver(node: Node, sig: Sig, scope: Scope) {
    const w = new Work(sig, node, scope);
    if (node.spec.maxHops !== undefined && sig.hops > node.spec.maxHops) return this.drop(w, 'hops');
    this.live++;
    scope.works.add(w);
    if (sig.replaces !== undefined) {
      const key = `${scope.id}\0${sig.from}\0${sig.replaces}`;
      this.stale.set(key, [...(this.stale.get(key) ?? []), w]);
    }
    const k = node.spec.threshold ?? 1;
    if (k > 1) this.hold(w, k);
    else this.start([w]);
  }

  // ---- threshold (SPEC 5.2) ----
  private hold(w: Work, k: number) {
    const group = w.node.groups.get(w.sig.trace) ?? [];
    const older = group.find((h) => h.sig.from === w.sig.from); // a sender counts once: the newer input replaces
    if (older) {
      this.ungroup(older);
      this.drop(older, 'replaced', 'waiting');
      this.retire(older);
    }
    const held = [...(w.node.groups.get(w.sig.trace) ?? []), w];
    w.held = true;
    if (new Set(held.map((h) => h.sig.from)).size < k) {
      w.node.groups.set(w.sig.trace, held);
      w.timer = setTimeout(() => (this.ungroup(w), this.drop(w, 'expired'), this.retire(w)), w.node.spec.within ?? DEFAULT_WITHIN);
      return;
    }
    w.node.groups.delete(w.sig.trace);
    for (const h of held) (clearTimeout(h.timer), (h.held = false));
    this.start(held);
  }

  private ungroup(w: Work) {
    const rest = (w.node.groups.get(w.sig.trace) ?? []).filter((h) => h !== w);
    if (rest.length) w.node.groups.set(w.sig.trace, rest);
    else w.node.groups.delete(w.sig.trace);
    clearTimeout(w.timer);
    w.held = false;
  }

  /** Done with a work (replied, dropped, cancelled): it no longer counts as unfinished. */
  private retire(w: Work) {
    if (!w.scope.works.delete(w)) return;
    clearTimeout(w.timer);
    if (--this.live === 0) this.over?.();
    if (w.scope.works.size === 0) w.scope.onIdle?.();
  }

  // ---- calls (SPEC 3.1, 2.6, 6.1) ----
  private start(inputs: Work[]) {
    const call = new Call(inputs), { node } = call.crossing;
    for (const w of inputs) w.call = call;
    const ms = node.spec.timeout ?? DEFAULT_TIMEOUT;
    const timer = setTimeout(() => {
      if (call.cancelled) return;
      this.fail(call.crossing, { error: `timed out after ${ms} ms`, code: 'timeout' });
      this.cancel(call);
    }, ms);
    const done = (o: Outcome) => { // the program is over (process ended or inner network answered)
      clearTimeout(timer);
      if (!call.cancelled) this.reply(call.crossing, o);
      for (const w of inputs) this.retire(w);
    };
    const value = (node.spec.threshold ?? 1) > 1 ? { payload: call.crossing.sig.payload, inputs: inputs.map((w) => w.sig.payload) } : call.crossing.sig.payload;
    if (node.inner) this.compose(call, node, value, done);
    else this.program(call, node, value, done);
  }

  private cancel(call: Call) {
    call.cancelled = true;
    this.note(call.crossing, 'cancel');
    call.stop();
  }

  /** Level 0: one process per call, in its own process group. */
  private program(call: Call, node: Node, value: unknown, done: (o: Outcome) => void) {
    const spec = node.spec, cmd = spec.run!;
    const env: Record<string, string> = { PATH: process.env.PATH ?? '' };
    for (const name of spec.env ?? []) if (process.env[name] !== undefined) env[name] = process.env[name]!;
    const child = spawn(cmd[0]!, cmd.slice(1), { cwd: resolve(node.dir, spec.cwd ?? '.'), env, detached: true, stdio: ['pipe', 'pipe', 'pipe'] });
    const pid = child.pid;
    const note = (what: string) => this.note(call.crossing, what);
    const alive = () => { try { return pid !== undefined && (process.kill(-pid, 0), true); } catch { return false; } };
    const killGroup = (signal: NodeJS.Signals) => { try { process.kill(-pid!, signal); } catch { /* already gone */ } };
    let out = '', err = '', size = 0, over = false, stopped = false, killed = false, finished = false, outcome: Outcome | undefined, killTimer: NodeJS.Timeout | undefined;
    const end = () => { if (!finished) (finished = true, clearTimeout(killTimer), done(outcome!)); };
    child.stdin.on('error', () => {}); // a program need not read stdin
    child.stdin.end(JSON.stringify(value));
    child.stdout.on('data', (b: Buffer) => {
      size += b.length;
      if (size > MAX_OUTPUT && !over) (over = true, killed = true, killGroup('SIGKILL'), note('kill'));
      if (!over) out += b.toString();
    });
    child.stderr.on('data', (b: Buffer) => { if (err.length < 500) err += b.toString(); });
    const failed = (code: string, message: string) => { outcome = { error: message, code }; };
    child.on('error', (e) => { failed('exit', `exit 127: could not start ${cmd[0]}: ${e.message}`); end(); });
    child.on('close', (code, signal) => {
      if (finished) return;
      const why = err.trim().slice(0, 500);
      if (over) failed('output', 'output too large');
      else if (signal) failed('exit', `signal ${signal}: ${why}`);
      else if (code !== 0) failed('exit', `exit ${code}: ${why}`);
      else if (!out.trim()) outcome = { quiet: true };
      else {
        try { outcome = { answer: JSON.parse(out) }; } catch { failed('junk', `not JSON: ${out.trim().slice(0, 80)}`); }
      }
      if (!stopped || killed || !alive()) end(); // else the kill timer ends it
    });
    call.stop = () => { // cancel: SIGTERM to the group, SIGKILL 2 s later if anything remains (discarded output)
      if (finished || stopped) return;
      stopped = true;
      killGroup('SIGTERM');
      note('term');
      killTimer = setTimeout(() => {
        if (alive()) (killed = true, killGroup('SIGKILL'), note('kill'));
        if (outcome) end();
      }, KILL_AFTER);
    };
  }

  /** A composed neuron: the input enters the shared inner network as a new root signal; `exit`'s first reply is the reply. */
  private compose(call: Call, node: Node, value: unknown, done: (o: Outcome) => void) {
    const inner = node.inner!;
    const scope = new Scope(this.id(), inner, this.inner && ((e) => this.inner!(node.path, e)));
    let over = false;
    const finish = (o: Outcome) => { if (!over) (over = true, this.cancelScope(scope), done(o)); };
    scope.onExit = finish;
    scope.onIdle = () => finish(scope.failed ? { error: 'no answer: an inner neuron failed', code: 'inner' } : { quiet: true });
    call.stop = () => finish({ quiet: true });
    const id = this.id();
    this.send({ id, trace: id, from: node.id, to: [inner.net.entry!], hops: 0, payload: value, ...withKey(call.crossing.sig.replaces) }, scope);
  }

  /** Cancels all unfinished work of a scope without reports (SPEC 2.6 step 2 and 4). */
  private cancelScope(scope: Scope) {
    for (const w of [...scope.works]) {
      if (w.held) (this.ungroup(w), this.retire(w));
      else if (w.call && !w.call.cancelled) (w.call.cancelled = true, w.call.stop());
    }
  }

  // ---- replies (SPEC 4.3, 4.4) ----
  private fail(w: Work, o: { error: string; code: string }) {
    w.scope.failed = true;
    this.report(w.scope, { type: 'error', ...this.fields(w.sig), neuron: w.node.id, message: o.error, code: o.code, payload: this.payload(w.sig.payload) });
  }

  private reply(w: Work, o: Outcome) {
    if ('error' in o) return this.fail(w, o);
    const { sig, node, scope } = w;
    const reply: Sig = { id: this.id(), trace: sig.trace, from: node.id, to: [sig.from], hops: sig.hops + 1, replyTo: sig.id };
    if ('quiet' in o) this.report(scope, { type: 'quiet', ...this.fields(reply) });
    else this.report(scope, { type: 'answer', ...this.fields(reply), payload: this.payload(o.answer) });
    if (node === scope.inst.nodes.get(scope.inst.net.exit ?? '')) scope.onExit?.(o);
    if ('answer' in o && node.out.length) {
      this.send({ id: this.id(), trace: sig.trace, from: node.id, to: node.out.map((n) => n.id), hops: reply.hops + 1, cause: reply.id, payload: o.answer, ...withKey(sig.replaces) }, scope);
    }
  }

  // ---- the run ----
  over?: () => void;
  async injectAll(injections: Injection[]) {
    for (const inj of [...injections].sort((a, b) => a.at - b.at)) {
      const wait = inj.at - this.now();
      if (wait > 0) await sleep(wait);
      const source = this.root.nodes.get(inj.source)!;
      const id = this.id();
      this.send({ id, trace: id, from: source.id, to: source.out.map((n) => n.id), hops: 0, payload: inj.payload, ...withKey(inj.replaces) }, this.rootScope);
    }
    if (--this.live === 0) this.over?.();
  }
}

/** Reads injections, one JSON object per line: `at` (ms), `source`, `payload`, optional `replaces`. Throws on a bad line. */
export function readInjections(file: string): Injection[] {
  return readFileSync(file, 'utf8').split('\n').flatMap((line, i) => {
    if (!line.trim()) return [];
    const bad = (m: string): never => { throw new Error(`${file}:${i + 1}: ${m}`); };
    let v: Record<string, unknown>;
    try { v = JSON.parse(line) as Record<string, unknown>; } catch { return bad('not JSON'); }
    if (typeof v !== 'object' || v === null || Array.isArray(v)) bad('expected an object');
    if (typeof v.at !== 'number' || !(v.at >= 0)) bad('"at" must be a number of ms, 0 or more');
    if (typeof v.source !== 'string') bad('"source" must be a neuron id');
    if (!('payload' in v)) bad('missing "payload"');
    if (v.replaces !== undefined && (typeof v.replaces !== 'string' || Buffer.byteLength(v.replaces) > MAX_KEY)) bad(`"replaces" must be a string of at most ${MAX_KEY} bytes`);
    return [{ at: v.at as number, source: v.source as string, payload: v.payload, ...withKey(v.replaces as string | undefined) }];
  });
}

/**
 * Runs the network in `file` with the producer's `injections` and returns its trace.
 * Throws if the file does not check, uses a kind this engine lacks, or an injection names no source.
 * `inner` receives the events inside composed neurons (by instance path): an operator's tool, never in the trace.
 */
export async function runNetwork(file: string, injections: Injection[], options: { inner?: (path: string, e: Event) => void } = {}): Promise<Trace> {
  const errors = check(file).filter((f) => f.severity === 'error');
  if (errors.length) throw new Error(errors.map(formatFinding).join('\n'));
  const net = readNetwork(file);
  const root = new Instance(resolve(file), net, '');
  const unsupported = (i: Instance): string[] =>
    [...i.nodes.values()].flatMap((n) => (n.inner ? unsupported(n.inner) : n.spec.serve || n.spec.http || n.spec.human ? [`${n.path} (${n.spec.serve ? 'serve' : n.spec.http ? 'http' : 'human'})`] : []));
  if (unsupported(root).length) throw new Error(`unsupported neuron kinds in v0 of the engine: ${unsupported(root).join(', ')}`);
  for (const inj of injections) if (!root.nodes.get(inj.source)?.spec.source) throw new Error(`injection names "${inj.source}", which is not a source of ${file}`);

  const run = new Run(root, options.inner);
  await new Promise<void>((resolveRun) => {
    run.over = resolveRun;
    void run.injectAll(injections);
  });
  const kind = (s: NeuronSpec) => (s.source ? 'source' : s.run ? 'run' : s.serve ? 'serve' : s.http ? 'http' : s.human ? 'human' : 'composed');
  return {
    format: 'agent-neuron-trace', version: 1, demo: net.name ?? basename(file), description: net.description ?? '', library: VERSION, engine: `agent-neuron engine ${VERSION}`,
    neurons: net.neurons.map((s) => {
      const options = Object.fromEntries((['threshold', 'within', 'maxHops'] as const).flatMap((k) => (s[k] === undefined ? [] : [[k, s[k]!]])));
      return { id: s.id, description: s.description || `${kind(s)} neuron`, kind: kind(s), ...(Object.keys(options).length && { options }) };
    }),
    links: (net.links ?? []).map((l) => ({ from: l.from, to: l.to })),
    events: run.events,
    summary: {},
  };
}

/** The trace as text, one neuron, link and event per line (as the recorder writes it). */
export const formatTrace = (t: Trace): string => {
  const list = (items: unknown[]) => `[\n${items.map((x) => `    ${JSON.stringify(x)}`).join(',\n')}\n  ]`;
  return `{\n${Object.entries(t).map(([k, v]) => `  ${JSON.stringify(k)}: ${['neurons', 'links', 'events'].includes(k) ? list(v as unknown[]) : JSON.stringify(v)}`).join(',\n')}\n}\n`;
};
