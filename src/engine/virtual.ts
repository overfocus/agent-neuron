// A virtual clock and scripted programs for the conformance fixtures (docs/CONFORMANCE.md).
// The engine sees a `Host` (time and processes). This one has no real time and no real processes: a scripted program is a
// fake child process whose output arrives on the clock, so the engine code above it is the code that runs for real.
import { EventEmitter } from 'node:events';
import { isDeepStrictEqual } from 'node:util';
import type { Host } from './engine.js';

type Timer = { due: number; seq: number; fn: () => void; cancelled: boolean };
type Reply = Record<string, unknown>;
type Entry = { when?: unknown; after?: number; reply: Reply; ignoreTerm?: boolean; onCancel?: { after?: number; reply: Reply } };
type Start = { ready?: number | false; maxInflight?: number; first?: unknown; readyV?: number; pong?: number | false; end?: { after?: number; how?: 'exit' | 'closeStdout' | 'error' }; ignoreTerm?: boolean; stderr?: string };
export type Script = Record<string, { calls?: Entry[]; starts?: Start[] }>;

const BIG = 1 << 20;

export class VirtualHost implements Host {
  t = 0;
  private seq = 0;
  private timers: Timer[] = [];
  private procs = new Map<number, VProc>();
  private pids = 1000;
  private used = new Set<Entry>();
  private starts = new Map<string, number>();
  constructor(private script: Script) {}

  now = () => this.t;
  setTimeout = (fn: () => void, ms: number) => {
    const timer: Timer = { due: this.t + Math.max(0, ms), seq: this.seq++, fn, cancelled: false };
    this.timers.push(timer);
    return timer as unknown as NodeJS.Timeout;
  };
  clearTimeout = (t: NodeJS.Timeout | undefined) => { if (t) (t as unknown as Timer).cancelled = true; };
  setInterval = (fn: () => void, ms: number) => {
    const tick = (): void => { handle = this.setTimeout(() => (fn(), tick()), ms); };
    let handle: NodeJS.Timeout;
    tick();
    return { cancel: () => this.clearTimeout(handle) } as unknown as NodeJS.Timeout;
  };
  clearInterval = (t: NodeJS.Timeout | undefined) => { if (t) (t as unknown as { cancel: () => void }).cancel(); };

  /** Runs the next wake-up: the earliest, and among equals the one set first (SPEC 6.2 rule 1). */
  step = () => {
    this.timers = this.timers.filter((x) => !x.cancelled);
    let next: Timer | undefined;
    for (const x of this.timers) if (!next || x.due < next.due || (x.due === next.due && x.seq < next.seq)) next = x;
    if (!next) return false;
    this.timers.splice(this.timers.indexOf(next), 1);
    this.t = next.due;
    next.fn();
    return true;
  };

  launch = (node: { spec: { run?: string[]; serve?: string[] }; path: string }) => {
    const level = node.spec.serve ? 1 : 0;
    const n = this.starts.get(node.path) ?? 0;
    this.starts.set(node.path, n + 1);
    const start = level ? (this.script[node.path]?.starts?.[n] ?? {}) : {};
    const p = new VProc(this, level, node.path, start, ++this.pids);
    this.procs.set(p.pid, p);
    return p as never;
  };
  killGroup = (pid: number | undefined, signal: NodeJS.Signals) => this.procs.get(pid!)?.signal(signal);
  alive = (pid: number | undefined) => this.procs.get(pid!)?.up ?? false;

  /** The first unused entry whose `when` equals the program's input (none: matches anything). A miss is the fixture's fault. */
  take(path: string, input: unknown): Entry {
    const entry = this.script[path]?.calls?.find((e) => !this.used.has(e) && (!('when' in e) || isDeepStrictEqual(e.when, input)));
    if (!entry) throw Object.assign(new Error(`no unused script entry for "${path}" matches the input ${JSON.stringify(input)}`), { harness: true });
    this.used.add(entry);
    return entry;
  }
}

/** A scripted program: level 0 acts when stdin closes; level 1 speaks the protocol over its pipes. */
class VProc extends EventEmitter {
  up = true;
  private exited = false;
  private ignoreTerm: boolean;
  private inflight = new Map<number, { entry: Entry; timer: NodeJS.Timeout | undefined }>();
  stdout = new EventEmitter();
  stderr = new EventEmitter();
  stdin = Object.assign(new EventEmitter(), { write: (line: string) => this.onLine(line), end: (text?: string) => (this.level ? this.shutdown() : this.onInput(text ?? '')) });
  constructor(private host: VirtualHost, private level: 0 | 1, private path: string, private start: Start, readonly pid: number) {
    super();
    this.ignoreTerm = start.ignoreTerm ?? false;
  }

  private later(ms: number, fn: () => void) { return this.host.setTimeout(() => this.up && fn(), ms); }
  private out(m: unknown) { this.stdout.emit('data', Buffer.from(typeof m === 'string' ? `${m}\n` : `${JSON.stringify(m)}\n`)); }
  signal(sig: NodeJS.Signals) {
    if (sig === 'SIGTERM' && this.ignoreTerm) return;
    this.die(null, sig);
  }
  /** The process ends: its group is empty now, and the engine hears of it on the clock (the same instant, after what is already due). */
  private die(code: number | null, sig: NodeJS.Signals | null = null) {
    if (!this.up) return;
    this.up = false;
    this.host.setTimeout(() => {
      if (this.exited) return;
      this.exited = true;
      this.stdout.emit('end');
      this.emit('exit', code, sig);
      this.emit('close', code, sig);
    }, 0);
  }
  private shutdown() { this.die(0); } // stdin closed: end of input

  // ---- level 0 ----
  private onInput(text: string) {
    const entry = this.host.take(this.path, JSON.parse(text));
    if (entry.ignoreTerm) this.ignoreTerm = true;
    this.later(entry.after ?? 0, () => this.finish(entry.reply));
  }
  private finish(r: Reply) {
    if ('answer' in r) { this.stdout.emit('data', Buffer.from(JSON.stringify(r.answer))); this.die(0); }
    else if (r.quiet) this.die(0);
    else if ('error' in r) { this.stderr.emit('data', Buffer.from(String(r.error))); this.die(1); }
    else if ('junk' in r) { this.stdout.emit('data', Buffer.from(String(r.junk))); this.die(0); }
    else if (r.crash) this.die(null, 'SIGKILL');
    else if (r.overlong) this.stdout.emit('data', Buffer.alloc(BIG + 1, 97));
    // hang: nothing, ever
  }

  // ---- level 1 ----
  private onLine(line: string) {
    const m = JSON.parse(line) as { type: string; re?: number; n?: number; payload?: unknown; inputs?: unknown[] };
    if (m.type === 'hello') return this.hello();
    if (m.type === 'ping') {
      const pong = this.start.pong ?? 0;
      if (pong !== false) this.later(pong, () => this.out({ type: 'pong', n: m.n }));
    } else if (m.type === 'signal') {
      const entry = this.host.take(this.path, m.inputs ? { payload: m.payload, inputs: m.inputs } : m.payload);
      if (entry.ignoreTerm) this.ignoreTerm = true;
      this.inflight.set(m.re!, { entry, timer: entry.reply.hang ? undefined : this.later(entry.after ?? 0, () => this.send(m.re!, entry.reply)) });
    } else if (m.type === 'cancel') {
      const call = this.inflight.get(m.re!);
      if (!call?.entry.onCancel) return; // the neuron ignores it and finishes in its own time
      this.host.clearTimeout(call.timer);
      const { after, reply } = call.entry.onCancel;
      call.timer = this.later(after ?? 0, () => this.send(m.re!, reply));
    }
  }
  private hello() {
    const s = this.start;
    if (s.stderr) this.stderr.emit('data', Buffer.from(s.stderr));
    if (s.ready === false) return;
    this.later(s.ready ?? 0, () => {
      this.out('first' in s ? (s.first as string) : { v: s.readyV ?? 1, type: 'ready', ...(s.maxInflight !== undefined && { maxInflight: s.maxInflight }) });
      if (s.end) this.later(s.end.after ?? 0, () => {
        if (s.end!.how === 'closeStdout') this.stdout.emit('end');
        else if (s.end!.how === 'error') this.out({ type: 'error', message: 'scripted failure' });
        else this.die(1);
      });
    });
  }
  private send(re: number, r: Reply) {
    this.inflight.delete(re);
    for (const m of (r.also as (object | string)[] | undefined) ?? []) this.out(typeof m === 'string' ? m : { re, ...m }); // other lines first (junk, unknown types, replies for other calls)
    if ('answer' in r) this.out({ type: 'answer', re, payload: r.answer, ...('replaces' in r && { replaces: r.replaces }), ...('to' in r && { to: r.to }) });
    else if (r.quiet) this.out({ type: 'quiet', re });
    else if ('error' in r) this.out({ type: 'error', re, message: r.error });
    else if ('junk' in r) this.out(String(r.junk));
    else if (r.crash) this.die(1);
    else if (r.overlong) this.stdout.emit('data', Buffer.alloc(BIG + 1, 97));
    else if ('send' in r) this.out({ re, ...(r.send as object) });
    // hang: nothing, ever
  }
}
