// The reference engine: runs a network file (docs/SPEC.md) and returns a trace (docs/TRACE.md, format v1).
// `source`, level 0 `run`, level 1 `serve`, composed `network` and, given a desk, `human` neurons, with budgets (SPEC 10), effects (SPEC 11) and the live stream (SPEC 9.1). Not here: http (SPEC 3.3).
//
// Vocabulary as in the spec: an input is delivered to a neuron as a Work; a threshold group turns held works into one
// Call; a Call ends in an Outcome (answer, quiet, error). Everything is one synchronous event loop, so the order of the
// events is the order of the code below (SPEC 6.2); a serve neuron's process is the one other source of events. A composed neuron's inside is another Instance, run under its own
// Scope: its events go nowhere unless the caller asks (sealing, SPEC 2.6).
import { spawn, type ChildProcessWithoutNullStreams as Proc } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { basename, dirname, resolve } from 'node:path';
import { VERSION } from '../index.js';
import { check, formatFinding, readNetwork, type GroundSpec, type NeuronSpec } from './check.js';

export type Units = Record<string, number>;
export type Injection = { at: number; source: string; payload: unknown; replaces?: string; budget?: Units };
export type Event = { type: string; t: number; [field: string]: unknown };
export type Trace = {
  format: 'agent-neuron-trace'; version: 1; demo: string; description: string; library: string; engine: string;
  budget?: Units; neurons: { id: string; kind: string; description: string; options?: Record<string, number>; cost?: Units }[];
  links: { from: string; to: string }[]; events: Event[]; summary: Record<string, unknown>;
};

type Sig = { id: string; trace: string; from: string; to: string[]; hops: number; payload?: unknown; replyTo?: string; cause?: string; replaces?: string };
type Outcome = { answer: unknown; replaces?: string | null; to?: string[] } | { quiet: true } | { error: string; code: string; started?: false };

const MAX_AMOUNT = Number.MAX_SAFE_INTEGER, DEFAULT_WITHIN = 10_000, DEFAULT_TIMEOUT = 30_000, MAX_OUTPUT = 1 << 20, KILL_AFTER = 2_000, GRACE = 100, MAX_KEY = 256, PAYLOAD_CAP = 2000, PING_EVERY = 10_000, MAX_STARTS = 5, STABLE_AFTER = 1_000;
/**
 * A question to a person (SPEC 3.4): a `human` neuron's input, and where it comes from. `id` is the crossing signal's id.
 * `t` is the engine's clock (ms since the run started, as in the trace).
 */
export type Question = { id: string; neuron: string; path: string; description: string; payload: unknown; t: number };
/**
 * Where a `human` neuron's questions go: the command line's page (`./page.js`), or a test's scripted person. `answer` ends the call: an
 * empty string is quiet, any other string is the text payload it replies with (SPEC 3.4); a second answer, or one after `withdraw`, is
 * ignored. `withdraw` says the call ended without an answer (stale, timed out, or the run was cut), so the question can be taken down.
 */
export interface Desk {
  ask(q: Question, answer: (text: string) => void): void;
  withdraw(id: string): void;
}

/** The failures of an effect call (SPEC 11.3) that stay errors: the call never started, or the neuron itself said it failed. Every other failure is `unknown`. */
const KNOWN_FAILURES = ['budget', 'start', 'neuron'];
const groups = new Set<number>(); // process groups of running programs
/** Time and processes: the seam the conformance fixtures use (SPEC 8). `realHost` is what runs; `src/engine/virtual.ts` is the other. */
export interface Host {
  now(): number;
  setTimeout(fn: () => void, ms: number): NodeJS.Timeout;
  clearTimeout(t: NodeJS.Timeout | undefined): void;
  setInterval(fn: () => void, ms: number): NodeJS.Timeout;
  clearInterval(t: NodeJS.Timeout | undefined): void;
  launch(node: { spec: NeuronSpec; dir: string; path: string }): Proc;
  killGroup(pid: number | undefined, signal: NodeJS.Signals): void;
  alive(pid: number | undefined): boolean;
  /** Virtual only: runs the next due wake-up (same instant: the order they were set); false when none is left. */
  step?(): boolean;
}
const realKill = (pid: number | undefined, signal: NodeJS.Signals) => {
  try { process.kill(-pid!, signal); } catch { /* already gone */ }
};
const realHost: Host = {
  now: () => performance.now(),
  setTimeout: (fn, ms) => setTimeout(fn, ms),
  clearTimeout: (t) => clearTimeout(t),
  setInterval: (fn, ms) => setInterval(fn, ms),
  clearInterval: (t) => clearInterval(t),
  launch: (node) => launch(node),
  killGroup: realKill,
  alive: (pid) => {
    try { return pid !== undefined && (process.kill(-pid, 0), true); } catch { return false; }
  },
};
let host: Host = realHost; // swapped for the length of a virtual run (runNetwork)
const killGroup = (pid: number | undefined, signal: NodeJS.Signals) => host.killGroup(pid, signal);
const alive = (pid: number | undefined) => host.alive(pid);
/** Signals every running program's process group: for a CLI that is itself being stopped. */
export function killGroups(signal: NodeJS.Signals) {
  for (const pid of groups) realKill(pid, signal);
}
const sleep = (ms: number) => new Promise((r) => host.setTimeout(r as () => void, ms));
/** Resolves when no member of the process group is left, or after `ms`: the grace for what a leader left behind. */
const groupGone = async (pid: number | undefined, ms: number) => {
  for (let waited = 0; waited < ms && alive(pid); waited += 20) await sleep(20);
};
const withKey = (replaces: string | undefined) => (replaces === undefined ? {} : { replaces });
/** A program in its own process group, with only PATH and its `env` names (SPEC 2.4); the group is registered in `groups`. */
function launch(node: { spec: NeuronSpec; dir: string }): Proc {
  const spec = node.spec, cmd = (spec.run ?? spec.serve)!;
  const env: Record<string, string> = { PATH: process.env.PATH ?? '' };
  for (const name of spec.env ?? []) if (process.env[name] !== undefined) env[name] = process.env[name]!;
  const child = spawn(cmd[0]!, cmd.slice(1), { cwd: resolve(node.dir, spec.cwd ?? '.'), env, detached: true, stdio: ['pipe', 'pipe', 'pipe'] });
  if (child.pid !== undefined) groups.add(child.pid);
  return child;
}

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
  groups = new Map<string, Work[]>(); // threshold: scope and trace -> held works
  inner?: Instance;
  server?: Server; // serve neurons
  places = 0; // `maxInflight` (SPEC 2.4): the places its calls hold; one is given back at the end of the step in which its call ended
  constructor(readonly spec: NeuronSpec, readonly dir: string, readonly path: string) {
    if (spec.network) this.inner = new Instance(resolve(dir, spec.network), readNetwork(resolve(dir, spec.network)), path);
  }
  get id() { return this.spec.id; }
}
/** Where work happens: the outer network, or one call into a composed neuron (its own trace, its own staleness). */
class Scope {
  seq = 0; // ids are numbered per scope, so outer ids never reveal how much happened inside a composed neuron
  next = () => String(++this.seq);
  works = new Set<Work>(); // unfinished work
  failed = false; // an inner neuron failed
  onExit?: (o: Outcome) => void; onIdle?: () => void;
  pools = new Map<string, Pool>(); // trace -> what is left of its budget
  constructor(readonly id: string, readonly inst: Instance, readonly report?: (e: Event) => void, readonly budget?: Units) {}
}
/** What is left of one trace's budget (SPEC 10): the pool its calls reserve from. */
class Pool {
  exhausted = false; // a call of the trace was refused
  constructor(readonly left: Units) {}
}
/** The pool of a composed neuron's inside (SPEC 10.5): its cost, each unit lowered by the inner file's own budget; with no cost, that budget. */
const innerPool = (cost?: Units, own?: Units): Units | undefined =>
  cost ? Object.fromEntries(Object.entries(cost).map(([unit, n]) => [unit, Math.min(n, own?.[unit] ?? n)])) : own;
/** One input at one neuron, from delivery until its reply, drop or cancellation. */
class Work {
  timer?: NodeJS.Timeout; held = false; call?: Call;
  constructor(readonly sig: Sig, readonly node: Node, readonly scope: Scope) {}
}
/** One run of a program for the works that met the threshold; the last is the one that crossed. */
class Call {
  cancelled = false;
  queued = false; // waiting, not started: for a slot of a serve neuron, or for a place under its neuron's `maxInflight`
  place = false; // it holds one of the places of its neuron's `maxInflight`
  reserved?: Units; t0 = 0; // the cost reserved at the start of the call, and when it started
  finished = false; // the program is over and its reply is being made: no longer cuttable
  stale = false; // an effect call (SPEC 11) one of whose inputs went stale: not cut, its answer is not forwarded
  stop: () => void = () => {};
  constructor(readonly inputs: Work[]) {}
  get crossing() { return this.inputs.at(-1)!; }
}

/** One call to a serve neuron, from its place in the queue until the neuron replies for it (a cancelled one keeps its slot). */
type Job = { call: Call; message: object; arm: () => void; done: (o: Outcome) => void; re?: number; hang?: NodeJS.Timeout | undefined };

/** Level 1 (SPEC 3.2): the one long-lived process of a serve neuron: handshake, calls by `re`, ping, restart with backoff. */
class Server {
  child?: Proc | undefined;
  ready = false; failed = false; stopping = false;
  jobs = new Map<number, Job>(); // running, by re; a cancelled call stays until its reply or a hang
  queue: Job[] = [];
  maxInflight = 1; re = 0; pings = 0; line = 0; err = '';
  failedStarts = 0; // consecutive starts that did not prove themselves: no ready, a crash within STABLE_AFTER of ready, or no pong
  readyAt = 0;
  pinging?: number | undefined;
  startTimer?: NodeJS.Timeout; pingTimer?: NodeJS.Timeout; pingHang?: NodeJS.Timeout; restartTimer?: NodeJS.Timeout;
  reaping: Promise<void>[] = [];
  private begin?: { ok: () => void; no: (e: Error) => void } | undefined;
  constructor(readonly node: Node, readonly run: Run, readonly pingEvery: number) {}
  private get ms() { return this.node.spec.timeout ?? DEFAULT_TIMEOUT; }
  private write(m: object) { this.child?.stdin.write(`${JSON.stringify(m)}\n`); }

  /** Starts the process; resolves at `ready`, rejects if the first start fails (the engine then refuses to start). */
  start() {
    return new Promise<void>((ok, no) => {
      this.begin = { ok, no };
      this.spawn();
    });
  }

  private spawn() {
    const child = host.launch(this.node); // registers its process group
    this.child = child;
    let buf = Buffer.alloc(0);
    this.line = 0;
    this.err = '';
    child.stdin.on('error', () => {}); // a dead process is noticed by its close
    child.stdout.on('data', (d: Buffer) => { // bytes are split into lines first and each line is decoded once, so no character is cut by a chunk
      buf = Buffer.concat([buf, d]);
      for (let i = buf.indexOf(10); i >= 0 && child === this.child; i = buf.indexOf(10)) {
        if (i > MAX_OUTPUT) return this.tooLong(child);
        const line = buf.subarray(0, i).toString('utf8');
        buf = buf.subarray(i + 1);
        this.onLine(child, line);
      }
      if (buf.length > MAX_OUTPUT && child === this.child) this.tooLong(child);
    });
    child.stdout.on('end', () => this.down(child, 'crash', 'process failed')); // stdout closed, though the process may stay
    const errs: Buffer[] = [];
    child.stderr.on('data', (d: Buffer) => { // logs only: the first 500 bytes, decoded when read
      if (child !== this.child) return;
      if (Buffer.concat(errs).length < 500) errs.push(d);
      this.err = Buffer.concat(errs).subarray(0, 500).toString('utf8');
    });
    child.on('error', (e) => this.down(child, 'crash', `process failed: could not start ${this.node.spec.serve![0]}: ${e.message}`));
    // A helper that inherited stdout keeps 'close' from firing, so the exit itself is enough, after a grace for the last lines.
    child.on('exit', () => host.setTimeout(() => this.down(child, 'crash', 'process failed'), GRACE));
    child.on('close', () => this.down(child, 'crash', 'process failed'));
    this.write({ v: 1, type: 'hello', neuron: this.node.id });
    this.startTimer = host.setTimeout(() => this.down(child, 'timeout', `no ready within ${this.ms} ms`), this.ms);
  }

  private onLine(child: Proc, text: string) {
    this.line++;
    const raw = text.replace(/\r$/, '');
    if (!raw.trim()) return;
    let m: Record<string, unknown> | undefined;
    try { const v: unknown = JSON.parse(raw); if (typeof v === 'object' && v !== null && !Array.isArray(v) && typeof (v as { type?: unknown }).type === 'string') m = v as Record<string, unknown>; } catch { /* junk */ }
    if (!this.ready) return this.hello(child, m);
    if (!m) return this.junk(raw);
    if (m.type === 'pong') {
      if (m.n === this.pinging) {
        host.clearTimeout(this.pingHang);
        this.pinging = undefined;
        this.failedStarts = 0; // it answers: this start has proved itself
      }
    } else if (m.type === 'answer' || m.type === 'quiet' || m.type === 'error') this.reply(child, m);
  }

  private hello(child: Proc, m?: Record<string, unknown>) {
    const mi = m?.maxInflight ?? 1;
    const bad = !m || m.type !== 'ready' ? 'the first line must be ready' : m.v !== 1 ? 'ready.v differs from hello.v' : !Number.isInteger(mi) || (mi as number) < 1 ? 'maxInflight must be an integer, at least 1' : '';
    if (bad) return this.down(child, 'protocol', `protocol: ${bad}`);
    host.clearTimeout(this.startTimer);
    this.ready = true;
    this.readyAt = host.now();
    this.maxInflight = mi as number;
    this.pingTimer = host.setInterval(() => { // liveness, only when nothing is running (3.2)
      if (this.jobs.size || this.pinging !== undefined) return;
      this.pinging = ++this.pings;
      this.write({ type: 'ping', n: this.pinging });
      this.pingHang = host.setTimeout(() => this.down(child, 'crash', 'process failed: no pong', true), this.ms);
    }, this.pingEvery);
    this.begin?.ok();
    this.begin = undefined;
    this.pump();
  }

  private junk(raw: string) {
    this.run.processNote(this.node, 'junk', { line: this.line, text: raw.slice(0, 2000) });
    if (this.jobs.size !== 1) return; // with several calls running, the line cannot be attributed to one (decision 0001 section 9)
    const [re, job] = [...this.jobs][0]!;
    if (job.call.cancelled) return;
    this.jobs.delete(re);
    job.done({ error: `junk on stdout: ${raw.slice(0, 80)}`, code: 'junk' });
    this.pump();
  }

  private reply(child: Proc, m: Record<string, unknown>) {
    if (m.type === 'error' && m.re === undefined) return this.down(child, 'crash', `process failed: ${typeof m.message === 'string' ? m.message : 'error'}`);
    const job = typeof m.re === 'number' ? this.jobs.get(m.re) : undefined;
    if (!job) return; // unknown or finished: discarded
    this.jobs.delete(job.re!);
    host.clearTimeout(job.hang);
    job.done(job.call.cancelled ? { quiet: true } : this.outcome(m)); // a cancelled call's reply is discarded
    this.pump();
  }

  private outcome(m: Record<string, unknown>): Outcome {
    const bad = (why: string): Outcome => ({ error: `protocol: ${why}`, code: 'protocol' });
    if (m.type === 'quiet') return { quiet: true };
    if (m.type === 'error') return typeof m.message === 'string' ? { error: m.message, code: 'neuron' } : bad('error without a string message');
    const { replaces, to } = m;
    if (!('payload' in m)) return bad('answer without payload');
    if (replaces !== undefined && replaces !== null && (typeof replaces !== 'string' || Buffer.byteLength(replaces) > MAX_KEY)) return bad(`replaces must be null or a string of at most ${MAX_KEY} bytes`);
    if (to !== undefined && !(Array.isArray(to) && to.every((id) => this.node.out.some((n) => n.id === id)))) return bad('to must name outgoing links of this neuron');
    return { answer: m.payload, ...(replaces !== undefined && { replaces: replaces as string | null }), ...(to !== undefined && { to: to as string[] }) };
  }

  /** A call joins the queue; it is written when the process is ready and a slot is free. */
  submit(job: Job) {
    if (this.failed) return job.done({ error: 'neuron failed to start', code: 'start' });
    job.call.queued = true;
    job.call.stop = () => {
      const at = this.queue.indexOf(job);
      if (at >= 0) this.queue.splice(at, 1);
      job.done({ quiet: true });
    };
    this.queue.push(job);
    this.pump();
  }

  private pump() {
    while (this.ready && this.jobs.size < this.maxInflight && this.queue.length) {
      const job = this.queue.shift()!;
      job.call.queued = false;
      const refused = this.run.reserve(job.call); // checked when the slot frees (SPEC 10)
      if (refused) { job.done(refused); continue; }
      const re = ++this.re;
      job.re = re;
      this.jobs.set(re, job);
      job.call.stop = () => { // cancel: advisory; the slot is kept until the neuron replies for it or `timeout` ms pass, then it is hung
        this.write({ type: 'cancel', re });
        const child = this.child!;
        job.hang = host.setTimeout(() => this.down(child, 'crash', 'process failed: no reply to cancel'), this.ms);
      };
      this.write({ type: 'signal', re, ...job.message });
      job.arm();
    }
  }

  /** A line over 1 MiB: the call fails with `output` only if it is the one call running; else it cannot be told which, and all crash. */
  private tooLong(child: Proc) {
    if (this.jobs.size === 1) this.down(child, 'output', 'output too large');
    else this.down(child, 'crash', 'process failed: output too large');
  }

  /** The process is gone or to be killed (exit, hang, bad first line, oversize line, `error` without `re`): fail what ran, restart with backoff.
   *  `unproven`: count this start as failed however long it lived (it never answered a ping). */
  private down(child: Proc, code: string, message: string, unproven = false) {
    if (child !== this.child) return;
    this.child = undefined;
    const wasReady = this.ready;
    this.ready = false;
    host.clearTimeout(this.startTimer);
    host.clearInterval(this.pingTimer);
    host.clearTimeout(this.pingHang);
    this.pinging = undefined;
    const why = this.err.trim();
    const text = code === 'crash' && why ? `${message}: ${why}` : message;
    for (const job of this.jobs.values()) {
      host.clearTimeout(job.hang);
      job.done(job.call.cancelled ? { quiet: true } : { error: text, code });
    }
    this.jobs.clear();
    this.terminate(child);
    if (this.begin) {
      this.begin.no(Object.assign(new Error(`serve neuron "${this.node.path}" did not reach ready: ${text}`), { refusal: true }));
      this.begin = undefined;
      return;
    }
    if (this.stopping) return;
    const proven = wasReady && !unproven && host.now() - this.readyAt >= STABLE_AFTER;
    this.failedStarts = proven ? 0 : this.failedStarts + 1;
    if (this.failedStarts >= MAX_STARTS) { // failed: every input is an error until the engine restarts
      this.failed = true;
      for (const job of this.queue.splice(0)) {
        job.call.queued = false;
        job.done({ error: 'neuron failed to start', code: 'start' });
      }
      return;
    }
    this.restartTimer = host.setTimeout(() => {
      this.run.processNote(this.node, 'restart');
      this.spawn();
    }, Math.min(100 * 2 ** this.failedStarts, 5000));
  }

  /** What remains of the process group: SIGTERM, then SIGKILL after the grace period. */
  private terminate(child: Proc) {
    const pid = child.pid;
    if (!alive(pid)) {
      if (pid !== undefined) groups.delete(pid);
      return;
    }
    this.run.processNote(this.node, 'term');
    killGroup(pid, 'SIGTERM');
    this.reaping.push(groupGone(pid, KILL_AFTER).then(() => { // the grace holds for what a leader that has exited left behind, too
      if (alive(pid)) {
        killGroup(pid, 'SIGKILL');
        this.run.processNote(this.node, 'kill');
      }
      if (pid !== undefined) groups.delete(pid);
    }));
  }

  /** End of input: close stdin; SIGTERM after 2 s, SIGKILL 2 s later. Silent (SPEC 3.2). */
  async stop() {
    this.stopping = true;
    host.clearTimeout(this.startTimer);
    host.clearInterval(this.pingTimer);
    host.clearTimeout(this.pingHang);
    host.clearTimeout(this.restartTimer);
    const child = this.child, pid = child?.pid;
    this.child = undefined;
    if (child) {
      child.stdin.end();
      await groupGone(pid, KILL_AFTER);
      killGroup(pid, 'SIGTERM'); // a no-op if the group is gone
      await groupGone(pid, KILL_AFTER);
      killGroup(pid, 'SIGKILL');
      if (pid !== undefined) groups.delete(pid);
    }
    await Promise.all(this.reaping);
  }
}

class Run {
  t0 = host.now();
  scopes = 0; // scope ids (the staleness keys); never shown
  live = 1; // unfinished works, plus the injection loop; the run is over at 0 (SPEC 6.2 rule 6)
  stale = new Map<string, Work[]>(); // (scope, sender, key) -> unfinished works, in creation order
  events: Event[] = [];
  rootScope: Scope;
  open = new Map<string, number>(); // outer trace -> unfinished works; at none the trace is finished
  // ---- the ground (SPEC 4.5, decision 0012): one trail and one generator per run; only the outer file declares a ground ----
  ground?: GroundSpec | undefined;
  trail = new Map<string, { v: number; at: number }>(); // the entries: a place's (stored value, instant of its last write)
  gen = 0; // the generator's 32-bit state
  constructor(readonly root: Instance, readonly inner?: (path: string, e: Event) => void, readonly result?: (neuron: string, t: number, payload: unknown) => void, readonly observer?: Observer, readonly onEvent?: (e: Event) => unknown, readonly desk?: Desk) {
    this.rootScope = new Scope('', root, (e) => (this.events.push(e), this.observe(e), this.stream(e)), root.net.budget);
    this.ground = root.net.ground;
    this.gen = (root.net.ground?.seed ?? 0) >>> 0; // one 32-bit state, started at the ground's seed (0)
  }

  /** The live stream (SPEC 9.1): every outer event, once, in written order, offered to `onEvent` from a microtask, never inside the routing.
   *  The consumer gets a copy and its failure, sync or async, is ignored: it fails open. shortcut: the buffer is unbounded, so a consumer
   *  that never keeps up holds the events in memory; bound it if a run can outlast its reader. */
  private pending: Event[] = [];
  private stream(e: Event) {
    if (!this.onEvent) return;
    this.pending.push(structuredClone(e));
    if (this.pending.length === 1) queueMicrotask(() => this.flush());
  }
  private flush() { // by the time a run returns, the microtask for its last event has run: the stream is complete
    for (let e = this.pending.shift(); e; e = this.pending.shift()) {
      try { void Promise.resolve(this.onEvent!(e)).catch(() => {}); } catch { /* fails open */ }
    }
  }

  /** An observer (SPEC 9) sees outer events only and can inject into a source later; it is not part of the network. */
  private observe(e: Event) {
    if (typeof e.id !== 'string' && e.type !== 'finished') return; // an observer sees signals, replies, drops, errors and unknowns (events with an id) and `finished`, not notes
    let injections: Injection[] = [];
    try { injections = this.observer?.(e) ?? []; } catch { /* an observer that throws fails open (SPEC 9) */ }
    for (const inj of injections) {
      this.live++; // a pending injection keeps the run open (SPEC 6.2 rule 6)
      host.setTimeout(() => {
        this.report(this.rootScope, { type: 'note', neuron: inj.source, what: 'inject', trigger: e.id ?? e.trace }); // `finished` has no id: its trace is the trigger
        this.inject(inj);
        if (--this.live === 0) this.over?.();
      }, inj.at);
    }
  }

  now = () => Math.round(host.now() - this.t0);
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
  /** A process fact of a serve neuron, which belongs to no call. Inside a composed neuron it goes to the `inner` hook only. */
  processNote(node: Node, what: string, extra: object = {}) {
    const at = node.path.lastIndexOf('/'), e = { type: 'note', neuron: node.id, what, ...extra };
    if (at < 0) this.report(this.rootScope, e);
    else this.inner?.(node.path.slice(0, at), { ...e, t: this.now() });
  }

  // ---- serve neurons: started with the engine, stopped at the end of input ----
  servers: Server[] = [];
  async startServers(pingEvery: number) {
    const collect = (inst: Instance) => {
      for (const n of inst.nodes.values()) {
        if (n.inner) collect(n.inner);
        if (n.spec.serve) {
          n.server = new Server(n, this, pingEvery);
          this.servers.push(n.server);
        }
      }
    };
    collect(this.root);
    try {
      await Promise.all(this.servers.map((sv) => sv.start()));
    } catch (e) {
      await this.stopServers();
      throw e;
    }
    this.t0 = host.now(); // the run's clock starts when every neuron is ready
  }
  stopServers = () => Promise.all(this.servers.map((sv) => sv.stop()));

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
      if (w.call) { // running, or queued for a slot (any input of a threshold call cuts it; one drop, for the crossing input)
        if (w.call.cancelled || w.call.finished) continue;
        if (w.node.spec.effect && !w.call.queued) { w.call.stale = true; continue; } // a started effect call is never cut (SPEC 11.2)
        this.drop(w.call.crossing, 'replaced', w.call.queued ? 'waiting' : 'running');
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
    if (scope === this.rootScope) this.open.set(sig.trace, (this.open.get(sig.trace) ?? 0) + 1);
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
    const group = w.node.groups.get(this.group(w)) ?? [];
    const older = group.find((h) => h.sig.from === w.sig.from); // a sender counts once: the newer input replaces
    if (older) {
      this.ungroup(older);
      this.drop(older, 'replaced', 'waiting');
      this.retire(older);
    }
    const held = [...(w.node.groups.get(this.group(w)) ?? []), w];
    w.held = true;
    if (new Set(held.map((h) => h.sig.from)).size < k) {
      w.node.groups.set(this.group(w), held);
      w.timer = host.setTimeout(() => {
        this.ungroup(w);
        this.drop(w, 'expired');
        this.retire(w);
      }, w.node.spec.within ?? DEFAULT_WITHIN);
      return;
    }
    w.node.groups.delete(this.group(w));
    for (const h of held) {
      host.clearTimeout(h.timer);
      h.held = false;
    }
    this.start(held);
  }

  /** Inputs are grouped by (neuron, trace); ids are per scope, so the scope is part of the key: two calls of a composed neuron never share a group (SPEC 2.6, 5.2). */
  private group = (w: Work) => `${w.scope.id}\0${w.sig.trace}`;
  private ungroup(w: Work) {
    const rest = (w.node.groups.get(this.group(w)) ?? []).filter((h) => h !== w);
    if (rest.length) w.node.groups.set(this.group(w), rest);
    else w.node.groups.delete(this.group(w));
    host.clearTimeout(w.timer);
    w.held = false;
  }

  /** Done with a work (replied, dropped, cancelled): it no longer counts as unfinished. */
  private retire(w: Work) {
    if (!w.scope.works.delete(w)) return;
    host.clearTimeout(w.timer);
    if (w.scope === this.rootScope) this.settle(w.sig.trace, -1);
    if (--this.live === 0) this.over?.();
    if (w.scope.works.size === 0) w.scope.onIdle?.();
  }

  // ---- calls (SPEC 3.1, 2.6, 6.1) ----
  private start(inputs: Work[]) {
    const call = new Call(inputs), { node } = call.crossing;
    for (const w of inputs) w.call = call;
    const ms = node.spec.timeout ?? DEFAULT_TIMEOUT;
    let timer: NodeJS.Timeout | undefined;
    const arm = () => { // counted from the start of the call: a serve call's, from the moment its signal is written
      timer = host.setTimeout(() => {
        if (call.cancelled || call.finished) return;
        this.fail(call.crossing, { error: `timed out after ${ms} ms`, code: 'timeout' });
        this.cancel(call);
      }, ms);
    };
    const done = (o: Outcome) => { // the program is over (process ended or inner network answered)
      host.clearTimeout(timer);
      call.finished = true; // before replying: a keyed forward to itself must not cut the work it just answered
      this.commit(call); // every outcome consumes what it reserved: answer, cut, timeout, crash
      if (!call.cancelled) this.reply(call.crossing, o);
      for (const w of inputs) this.retire(w);
      if (call.place) this.vacate(call);
    };
    const value = (node.spec.threshold ?? 1) > 1 ? { payload: call.crossing.sig.payload, inputs: inputs.map((w) => w.sig.payload) } : call.crossing.sig.payload;
    if (node.server) {
      node.server.submit({ call, message: (node.spec.threshold ?? 1) > 1 ? (value as object) : { payload: value }, arm, done });
      return;
    }
    const go = () => this.begin(call, node, value, done, arm);
    const limit = node.spec.maxInflight;
    if (limit === undefined) return go();
    if (node.places < limit) {
      node.places++;
      call.place = true;
      return go();
    }
    // Every place of the neuron is taken (SPEC 2.4, 5.1): the call waits and has not started, so it reserves nothing and its timeout does not count.
    call.queued = true;
    call.stop = () => { // dropped while it waits (stale, or its composed call ended): it never starts
      const at = this.waiting.findIndex((x) => x.call === call);
      if (at >= 0) this.waiting.splice(at, 1);
      done({ quiet: true });
    };
    this.waiting.push({ call, go });
  }

  /** The call starts (SPEC 5.1): the budget first, then the program, then its timeout. */
  private begin(call: Call, node: Node, value: unknown, done: (o: Outcome) => void, arm: () => void) {
    const refused = this.reserve(call);
    if (refused) return done(refused);
    // The program's wake-ups are set before the timeout's: a reply due at the very instant of the timeout wins (SPEC 6.2 rule 7).
    if (node.inner) this.compose(call, node, value, done);
    else if (node.spec.human) this.person(call, node, value, done);
    else this.program(call, node, value, done);
    if (!call.finished) arm();
  }

  // ---- a limit on calls at once (SPEC 2.4 `maxInflight`, 5.1; decision 0021) ----
  private waiting: { call: Call; go: () => void }[] = []; // the calls waiting for a place at their own neuron, in arrival order, every neuron's in one list
  private ended: Call[] = []; // calls that held a place and are over; their places are free at the end of the step
  broken?: (e: unknown) => void; // ends the run with an error raised outside a wake-up (the end of a step)
  /** A call that held a place is over. Its place is given back at the end of the step (a microtask: after everything the step's wake-up caused), not here. */
  private vacate(call: Call) {
    this.ended.push(call);
    if (this.ended.length === 1) queueMicrotask(() => this.endOfStep());
  }
  private endOfStep() {
    try { this.drain(); } catch (e) { this.broken?.(e); } // a failure to start a waiting call is the run's failure, as one inside a wake-up is
  }
  /** The end of a step in which a place came free: the oldest waiting call whose neuron has a free place starts, one at a time, until none can. */
  private drain() {
    for (;;) {
      for (const over of this.ended.splice(0)) over.crossing.node.places--;
      const at = this.waiting.findIndex(({ call }) => call.crossing.node.places < call.crossing.node.spec.maxInflight!);
      if (at < 0) return;
      const [{ call, go }] = this.waiting.splice(at, 1) as [{ call: Call; go: () => void }];
      call.queued = false;
      call.place = true;
      call.crossing.node.places++;
      go();
    }
  }

  // ---- budget (SPEC 10) ----
  private pool = (scope: Scope, trace: string) => {
    if (!scope.budget) return undefined;
    if (!scope.pools.has(trace)) scope.pools.set(trace, new Pool({ ...scope.budget }));
    return scope.pools.get(trace)!;
  };
  /** Reserves the cost of a call as it starts; if some unit cannot be covered, nothing is taken and the error is returned. */
  reserve(call: Call): { error: string; code: string } | undefined {
    const { node, scope, sig } = call.crossing, cost = node.spec.cost, pool = this.pool(scope, sig.trace);
    if (!cost || !pool) return;
    const short = Object.entries(cost).find(([unit, n]) => n > (pool.left[unit] ?? 0));
    if (short) {
      pool.exhausted = true;
      return { error: `budget: needs ${short[1]} ${short[0]}, ${pool.left[short[0]] ?? 0} left`, code: 'budget' };
    }
    for (const [unit, n] of Object.entries(cost)) pool.left[unit] = (pool.left[unit] ?? 0) - n;
    call.reserved = cost;
    call.t0 = host.now();
    this.report(scope, { type: 'note', neuron: node.id, what: 'reserve', units: cost });
  }
  /** The call is over: commit min(claim, reservation). No claims exist yet, so the reservation is committed, marked estimated. */
  private commit(call: Call) {
    if (!call.reserved) return;
    const w = call.crossing;
    this.report(w.scope, { type: 'note', neuron: w.node.id, what: 'commit', units: call.reserved, ms: Math.round(host.now() - call.t0), basis: 'estimated' });
  }
  /** Counts the unfinished works of an outer trace; at none it is finished (SPEC 7.2). */
  private settle(trace: string, by: number) {
    const n = (this.open.get(trace) ?? 0) + by;
    if (n > 0) return void this.open.set(trace, n);
    this.open.delete(trace);
    const pool = this.rootScope.pools.get(trace);
    this.report(this.rootScope, { type: 'finished', trace, status: pool?.exhausted ? 'exhausted' : 'done', ...(pool && { remaining: { ...pool.left } }) });
    this.rootScope.pools.delete(trace); // a finished trace needs its pool no more: a long run does not keep them
  }

  private cancel(call: Call) {
    call.cancelled = true;
    if (!call.queued) this.note(call.crossing, 'cancel');
    call.stop();
  }

  /** `human` (SPEC 3.4): the input goes to the desk as a question; the answer ends the call; a cancel withdraws the question. */
  private person(call: Call, node: Node, value: unknown, done: (o: Outcome) => void) {
    const id = call.crossing.sig.id;
    let over = false;
    const end = (o: Outcome) => {
      if (over) return;
      over = true;
      done(o);
    };
    call.stop = () => {
      if (over) return;
      this.desk!.withdraw(id);
      end({ quiet: true }); // a cancelled call's outcome is not replied (done skips it)
    };
    this.desk!.ask({ id, neuron: node.id, path: node.path, description: node.spec.description ?? '', payload: value, t: this.now() }, (text) =>
      end(typeof text === 'string' && text !== '' ? { answer: text } : { quiet: true }));
  }

  /** Level 0: one process per call, in its own process group, which is killed when the call ends. */
  private program(call: Call, node: Node, value: unknown, done: (o: Outcome) => void) {
    const spec = node.spec, cmd = spec.run!, cwd = resolve(node.dir, spec.cwd ?? '.');
    const child = host.launch(node), pid = child.pid;
    const note = (what: string) => this.note(call.crossing, what);
    const stdout: Buffer[] = [], stderr: Buffer[] = []; // decoded once at the end: a character may span chunks
    let size = 0, errSize = 0, over = false, stopped = false, killed = false, exited = false, finished = false;
    let status: { code: number | null; signal: NodeJS.Signals | null } | undefined, failure: Outcome | undefined;
    let killTimer: NodeJS.Timeout | undefined, grace: NodeJS.Timeout | undefined;
    const outcome = (): Outcome => {
      if (failure) return failure;
      const why = Buffer.concat(stderr).toString().trim().slice(0, 500);
      if (over) return { error: 'output too large', code: 'output' };
      if (status?.signal) return { error: `signal ${status.signal}: ${why}`, code: 'exit' };
      if (status?.code !== 0) return { error: `exit ${status?.code}: ${why}`, code: 'exit' };
      const out = Buffer.concat(stdout).toString();
      if (!out.trim()) return { quiet: true };
      try { return { answer: JSON.parse(out) }; } catch { return { error: `not JSON: ${out.trim().slice(0, 80)}`, code: 'junk' }; }
    };
    const settle = () => {
      if (finished) return;
      finished = true;
      host.clearTimeout(killTimer);
      host.clearTimeout(grace);
      killGroup(pid, 'SIGKILL'); // whatever the program left behind must not outlive its call
      if (pid !== undefined) groups.delete(pid);
      done(outcome());
      if (over) note('kill'); // the failure first, then the note of the kill that caused it (SPEC 6.3)
    };
    // A cancelled call whose group still has members waits for the kill timer.
    const waiting = () => stopped && !killed && alive(pid);
    child.stdin.on('error', () => {}); // a program need not read stdin
    child.stdin.end(JSON.stringify(value));
    child.stdout.on('data', (b: Buffer) => {
      size += b.length;
      if (size > MAX_OUTPUT && !over) {
        over = killed = true;
        killGroup(pid, 'SIGKILL');
      }
      if (!over) stdout.push(b);
    });
    child.stderr.on('data', (b: Buffer) => {
      if (errSize < 2000) stderr.push(b);
      errSize += b.length;
    });
    child.on('error', (e: NodeJS.ErrnoException) => {
      const why = e.code === 'ENOENT' && !existsSync(cwd) ? `working directory not found: ${spec.cwd}` : `could not start ${cmd[0]}: ${e.message}`;
      failure = { error: `exit 127: ${why}`, code: 'exit', started: false };
      settle();
    });
    child.on('exit', (code, signal) => {
      exited = true;
      status = { code, signal };
      if (!waiting()) grace = host.setTimeout(settle, GRACE); // stdout normally ends first; a child holding it open does not keep the call
    });
    child.on('close', () => { if (exited && !waiting()) settle(); });
    call.stop = () => { // cancel: SIGTERM to the group, SIGKILL 2 s later if anything remains (output is discarded)
      if (finished || stopped) return;
      stopped = true;
      note('term'); // before the kill: where the process ends at once, the call's end follows the note
      killGroup(pid, 'SIGTERM');
      killTimer = host.setTimeout(() => {
        if (alive(pid)) {
          killed = true;
          note('kill'); // before the kill, as for term: where the process ends at once, the call's end follows the note
          killGroup(pid, 'SIGKILL');
        }
        if (exited) settle();
      }, KILL_AFTER);
    };
  }

  /** A composed neuron: the input enters the shared inner network as a new root signal; `exit`'s first reply is the reply. */
  private compose(call: Call, node: Node, value: unknown, done: (o: Outcome) => void) {
    const inner = node.inner!;
    const scope = new Scope(String(++this.scopes), inner, this.inner && ((e) => this.inner!(node.path, e)), innerPool(node.spec.cost, inner.net.budget)); // the cost caps the inside (SPEC 10.5)
    let over = false;
    const finish = (o: Outcome) => {
      if (over) return;
      over = true;
      this.cancelScope(scope);
      done(o);
    };
    scope.onExit = finish;
    scope.onIdle = () => finish(scope.failed ? { error: 'no answer: an inner neuron failed', code: 'inner' } : { quiet: true });
    call.stop = () => finish({ quiet: true });
    const id = scope.next();
    this.send({ id, trace: id, from: node.id, to: [inner.net.entry!], hops: 0, payload: value, ...withKey(call.crossing.sig.replaces) }, scope);
  }

  /** Cancels all unfinished work of a scope without reports (SPEC 2.6 step 2 and 4). */
  private cancelScope(scope: Scope) {
    for (const w of [...scope.works]) {
      if (w.held) {
        this.ungroup(w);
        this.retire(w);
      } else if (w.call && !w.call.cancelled) {
        w.call.cancelled = true;
        w.call.stop();
      }
    }
  }

  // ---- the ground (SPEC 4.5, decision 0012) ----
  /** A place's effective value at `t`: the stored `v` aged by the evaporate rate; a read never changes the entry. */
  private effective(place: string, t: number): number {
    const e = this.trail.get(place);
    return e ? e.v * Math.exp(-(this.ground!.evaporate ?? 0) * (t - e.at) / 1000) : 0;
  }

  /** The generator (SPEC 4.5): one 32-bit state, started at the seed, advanced once per draw; the word, with 32-bit wrap add and multiply and zero-fill right shifts. */
  private word(): number {
    this.gen = (this.gen + 0x6D2B79F5) | 0; // 32-bit wrap add
    let t = this.gen;
    t = Math.imul(t ^ (t >>> 15), t | 1); // (t xor (t >> 15)) × (1 | t)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61); // ((t + ((t xor (t >> 7)) × (61 | t))) xor t)
    return ((t ^ (t >>> 14)) >>> 0) / 2 ** 32; // (t xor (t >> 14)) (unsigned) / 2^32
  }

  /** A write (SPEC 4.5): a reply's payload carrying the named fields, the place aged then the amount added; one `ground` event, right after the answer's. */
  private deposit(node: Node, replyId: string, t: number, payload: unknown) {
    const d = this.ground?.deposit;
    if (!d) return;
    if (d.by !== undefined && d.by !== node.id) return;
    const fields = payload && typeof payload === 'object' && !Array.isArray(payload) ? (payload as Record<string, unknown>) : undefined;
    const place = fields?.[d.on], amount = fields?.[d.add];
    if (typeof place !== 'string' || typeof amount !== 'number' || !Number.isFinite(amount)) return;
    const e = this.trail.get(place);
    const v = (e ? e.v * Math.exp(-(this.ground!.evaporate ?? 0) * (t - e.at) / 1000) : 0) + amount;
    this.trail.set(place, { v, at: t });
    this.report(this.rootScope, { type: 'ground', place, v, cause: replyId });
  }

  /** The draw (SPEC 4.5): one word, then the first out-link, in declaration order, whose cumulative weight is greater than u·total; a zero total forwards nothing. */
  private drawn(node: Node, t: number): Node[] {
    const g = this.ground!;
    const u = this.word(); // exactly one word per omitted-`to` reply, whatever the outcome
    const weights = node.out.map((n) => Math.pow(Math.max(0, (g.base ?? 1) + this.effective(n.id, t)), g.power ?? 1));
    const total = weights.reduce((a, w) => a + w, 0);
    if (total <= 0) return [];
    let acc = 0;
    for (let i = 0; i < node.out.length; i++) if ((acc += weights[i]!) > u * total) return [node.out[i]!];
    return [node.out[node.out.length - 1]!]; // rounding left none: the last link
  }

  // ---- replies (SPEC 4.3, 4.4) ----
  private fail(w: Work, o: { error: string; code: string; started?: false }) {
    w.scope.failed = true;
    const unknown = w.node.spec.effect && o.started !== false && !KNOWN_FAILURES.includes(o.code); // the effect may have happened (SPEC 11.3); a program that could not be spawned never ran
    this.report(w.scope, { type: unknown ? 'unknown' : 'error', ...this.fields(w.sig), neuron: w.node.id, message: o.error, code: o.code, payload: this.payload(w.sig.payload) });
  }

  private reply(w: Work, o: Outcome) {
    if ('error' in o) return this.fail(w, o);
    const { sig, node, scope } = w;
    const reply: Sig = { id: scope.next(), trace: sig.trace, from: node.id, to: [sig.from], hops: sig.hops + 1, replyTo: sig.id };
    const stale = w.call?.stale === true; // an effect call whose input went stale: the answer is recorded, not forwarded (SPEC 11.2)
    if ('quiet' in o) this.report(scope, { type: 'quiet', ...this.fields(reply) });
    else this.report(scope, { type: 'answer', ...this.fields(reply), ...(stale && { forwarded: false }), payload: this.payload(o.answer) });
    if (node === scope.inst.nodes.get(scope.inst.net.exit ?? '')) scope.onExit?.(o);
    if ('answer' in o && !node.out.length && scope === this.rootScope) this.result?.(node.id, this.now(), o.answer);
    if ('answer' in o && scope === this.rootScope) this.deposit(node, reply.id, this.now(), o.answer); // the write lands right after the answer's event (SPEC 4.5)
    // the forward's targets (SPEC 4.4): a level 1 reply may name some of the outgoing links, kept in declaration order; a reply that
    // names no `to` takes them all — or, with the ground's select (SPEC 4.5), the one link the draw picks
    const named = 'answer' in o && !stale;
    const to = named
      ? o.to !== undefined
        ? node.out.filter((n) => !o.to || o.to.includes(n.id))
        : scope === this.rootScope && this.ground?.select ? this.drawn(node, this.now()) : node.out
      : [];
    if ('answer' in o && to.length) {
      const key = o.replaces === undefined ? sig.replaces : (o.replaces ?? undefined);
      this.send({ id: scope.next(), trace: sig.trace, from: node.id, to: to.map((n) => n.id), hops: reply.hops + 1, cause: reply.id, payload: o.answer, ...withKey(key) }, scope);
    }
  }

  // ---- the run ----
  over?: () => void;
  private inject(inj: Injection) {
    const source = this.root.nodes.get(inj.source)!;
    const id = this.rootScope.next();
    const pool = this.pool(this.rootScope, id);
    for (const [unit, n] of Object.entries(inj.budget ?? {})) if (pool) pool.left[unit] = n; // lowered at injection; runNetwork refused any raise
    this.open.set(id, 1); // the injection itself: the trace is not finished while its signal is still being delivered
    this.send({ id, trace: id, from: source.id, to: source.out.map((n) => n.id), hops: 0, payload: inj.payload, ...withKey(inj.replaces) }, this.rootScope);
    this.settle(id, -1);
  }
  async injectAll(injections: Injection[]) {
    if (host.step) { // virtual: every injection is scheduled before the run, in the order given, so at an instant it goes before anything the run schedules
      this.live += injections.length;
      for (const inj of injections) host.setTimeout(() => (this.inject(inj), --this.live === 0 && this.over?.()), inj.at);
    } else {
      for (const inj of [...injections].sort((a, b) => a.at - b.at)) {
        const wait = inj.at - this.now();
        if (wait > 0) await sleep(wait);
        this.inject(inj);
      }
    }
    if (--this.live === 0) this.over?.();
  }
}

/** What is wrong with an injection (SPEC 4.1), if anything; `ceiling` is the network's budget: a producer may lower it, never raise it. */
function problem(v: { at?: unknown; source?: unknown; replaces?: unknown; budget?: unknown }, ceiling?: Units): string | undefined {
  if (typeof v.at !== 'number' || !Number.isFinite(v.at) || v.at < 0) return '"at" must be a number of ms, 0 or more';
  if (typeof v.source !== 'string') return '"source" must be a neuron id';
  if (v.replaces !== undefined && (typeof v.replaces !== 'string' || Buffer.byteLength(v.replaces) > MAX_KEY)) return `"replaces" must be a string of at most ${MAX_KEY} bytes`;
  if (v.budget === undefined) return;
  if (typeof v.budget !== 'object' || v.budget === null || Array.isArray(v.budget)) return '"budget" must be an object of units';
  for (const [unit, n] of Object.entries(v.budget)) {
    if (!Number.isSafeInteger(n) || (n as number) < 0) return `"budget" ${unit} must be an integer from 0 to ${MAX_AMOUNT}`;
    if (!ceiling || !(unit in ceiling)) return `"budget" names unit "${unit}", which the network's budget does not have`;
    if ((n as number) > ceiling[unit]!) return `"budget" ${unit} ${n} is above the network's ${ceiling[unit]}: a producer may lower a budget, never raise it`;
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
    const wrong = problem({ ...v, budget: undefined }); // a budget is checked against the network's, by runNetwork
    if (wrong) bad(wrong);
    if (!('payload' in v)) bad('missing "payload"');
    return [{ at: v.at as number, source: v.source as string, payload: v.payload, ...withKey(v.replaces as string | undefined), ...(v.budget !== undefined && { budget: v.budget as Units }) }];
  });
}

const VIRTUAL_LIMIT = 2 * 86_400_000; // a virtual run still going after two days of virtual time is stuck (idle pings never stop); a human timeout may be a day
let active = 0; // runs in progress
/** Waits for `p`; on a virtual host it also runs the clock, one wake-up at a time, letting promises settle in between. */
async function drive<T>(p: Promise<T>): Promise<T> {
  if (!host.step) return p;
  let settled = false;
  void p.then(() => (settled = true), () => (settled = true));
  for (;;) {
    await new Promise((r) => setImmediate(r));
    if (settled) return p;
    if (!host.step()) throw Object.assign(new Error('virtual run stalled: nothing is due and the run is not over'), { harness: true });
    if (host.now() > VIRTUAL_LIMIT) throw Object.assign(new Error('virtual run did not end within two days of virtual time'), { harness: true });
  }
}

/**
 * Runs the network in `file` with the producer's `injections` and returns its trace.
 * Throws if the file does not check, uses a kind this engine lacks, or an injection names no source.
 * `inner` receives the events inside composed neurons (by instance path): an operator's tool, never in the trace.
 * `pingEvery` (ms, default 10000) is how often an idle serve neuron is pinged.
 * `result` receives the full answers of the neurons with no outgoing links (the trace cuts long payloads).
 * `host` replaces time and processes (a virtual clock and scripted programs, SPEC 8); one virtual run at a time.
 * `observer` is called with each outer event; the injections it returns (`at` is a delay in ms) enter later (SPEC 9).
 * `onEvent` is the live stream (SPEC 9.1): each event of the trace, in written order, as it happens; its failure never touches the run.
 * `desk` takes the questions of `human` neurons (SPEC 3.4) and answers them; without one, a file with a `human` neuron is refused.
 */
export type Observer = (e: Event) => Injection[] | void;
export async function runNetwork(file: string, injections: Injection[], options: { pingEvery?: number; inner?: (path: string, e: Event) => void; result?: (neuron: string, t: number, payload: unknown) => void; host?: Host; observer?: Observer; onEvent?: (e: Event) => unknown; desk?: Desk } = {}): Promise<Trace> {
  const errors = check(file).filter((f) => f.severity === 'error');
  if (errors.length) throw Object.assign(new Error(errors.map(formatFinding).join('\n')), { refusal: true }); // a file that does not check is refused like a serve neuron that is not ready (CONFORMANCE 4.3)
  const net = readNetwork(file);
  const root = new Instance(resolve(file), net, '');
  const unsupported = (i: Instance): string[] =>
    [...i.nodes.values()].flatMap((n) => (n.inner ? unsupported(n.inner) : n.spec.http ? [`${n.path} (http)`] : n.spec.human && !options.desk ? [`${n.path} (human, and no desk to ask)`] : []));
  if (unsupported(root).length) throw new Error(`unsupported neuron kinds in v0 of the engine: ${unsupported(root).join(', ')}`);
  for (const inj of injections) {
    const wrong = problem(inj, net.budget);
    if (wrong) throw Object.assign(new Error(`injection into "${inj.source}": ${wrong}`), { refusal: true });
  }
  for (const inj of injections) if (!root.nodes.get(inj.source)?.spec.source) throw new Error(`injection names "${inj.source}", which is not a source of ${file}`);

  if (options.host ? active > 0 : host !== realHost) throw new Error('a virtual run needs the engine to itself: no other run may be active, and a real run cannot start during a virtual one');
  const previous = host;
  if (options.host) host = options.host;
  active++;
  const run = new Run(root, options.inner, options.result, options.observer, options.onEvent, options.desk);
  try {
    await drive(run.startServers(options.pingEvery ?? PING_EVERY)); // throws if a serve neuron cannot reach ready
    try {
      await drive(new Promise<void>((resolveRun, rejectRun) => {
        run.over = resolveRun;
        run.broken = rejectRun;
        void run.injectAll(injections);
      }));
    } finally {
      await drive(run.stopServers());
    }
  } finally {
    active--;
    host = previous;
  }
  const kind = (s: NeuronSpec) => (s.source ? 'source' : s.run ? 'run' : s.serve ? 'serve' : s.http ? 'http' : s.human ? 'human' : 'composed');
  return {
    format: 'agent-neuron-trace', version: 1, demo: net.name ?? basename(file), description: net.description ?? '', library: VERSION, engine: `agent-neuron engine ${VERSION}`,
    ...(net.budget && { budget: net.budget }),
    neurons: net.neurons.map((s) => {
      const options = Object.fromEntries((['threshold', 'within', 'maxHops'] as const).flatMap((k) => (s[k] === undefined ? [] : [[k, s[k]!]])));
      return { id: s.id, description: s.description || `${kind(s)} neuron`, kind: kind(s), ...(Object.keys(options).length && { options }), ...(s.cost && { cost: s.cost }) };
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
