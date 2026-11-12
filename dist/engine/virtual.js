// A virtual clock and scripted programs for the conformance fixtures (docs/CONFORMANCE.md).
// The engine sees a `Host` (time and processes). This one has no real time and no real processes: a scripted program is a
// fake child process whose output arrives on the clock, so the engine code above it is the code that runs for real.
import { EventEmitter } from 'node:events';
import { isDeepStrictEqual } from 'node:util';
const BIG = 1 << 20;
export class VirtualHost {
    script;
    t = 0;
    seq = 0;
    timers = [];
    procs = new Map();
    pids = 1000;
    used = new Set();
    starts = new Map();
    constructor(script) {
        this.script = script;
    }
    now = () => this.t;
    setTimeout = (fn, ms) => {
        const timer = { due: this.t + Math.max(0, ms), seq: this.seq++, fn, cancelled: false };
        this.timers.push(timer);
        return timer;
    };
    clearTimeout = (t) => { if (t)
        t.cancelled = true; };
    setInterval = (fn, ms) => {
        const tick = () => { handle = this.setTimeout(() => (fn(), tick()), ms); };
        let handle;
        tick();
        return { cancel: () => this.clearTimeout(handle) };
    };
    clearInterval = (t) => { if (t)
        t.cancel(); };
    /** Runs the next wake-up: the earliest, and among equals the one set first (SPEC 6.2 rule 1). */
    step = () => {
        this.timers = this.timers.filter((x) => !x.cancelled);
        let next;
        for (const x of this.timers)
            if (!next || x.due < next.due || (x.due === next.due && x.seq < next.seq))
                next = x;
        if (!next)
            return false;
        this.timers.splice(this.timers.indexOf(next), 1);
        this.t = next.due;
        next.fn();
        return true;
    };
    launch = (node) => {
        const level = node.spec.serve ? 1 : 0;
        const n = this.starts.get(node.path) ?? 0;
        this.starts.set(node.path, n + 1);
        const start = level ? (this.script[node.path]?.starts?.[n] ?? {}) : {};
        const p = new VProc(this, level, node.path, start, ++this.pids);
        this.procs.set(p.pid, p);
        return p;
    };
    killGroup = (pid, signal) => this.procs.get(pid)?.signal(signal);
    alive = (pid) => { const p = this.procs.get(pid); return !!p && (p.up || p.member); };
    /**
     * The scripted person (CONFORMANCE 5.5): the desk a virtual run gives the engine for `human` neurons. Each question takes the
     * first unused `calls` entry of its instance path whose `when` equals the input, like a program; the reply comes `after` ms
     * later on the virtual clock, set when the question is asked, so it goes before the call's timeout at the same instant.
     * `{"answer": "text"}` replies with the text, `{"quiet": true}` with nothing (No), `{"hang": true}` never. A withdrawn
     * question's pending reply is cancelled.
     */
    desk() {
        const due = new Map();
        return {
            ask: (q, answer) => {
                const e = this.take(q.path, q.payload), r = e.reply;
                if (r.hang === true)
                    return;
                if (r.quiet !== true && typeof r.answer !== 'string')
                    throw Object.assign(new Error(`a person's reply for "${q.path}" is {"answer": text}, {"quiet": true} or {"hang": true}, not ${JSON.stringify(r)}`), { harness: true });
                due.set(q.id, this.setTimeout(() => (due.delete(q.id), answer(r.quiet === true ? '' : r.answer)), e.after ?? 0));
            },
            withdraw: (id) => {
                this.clearTimeout(due.get(id));
                due.delete(id);
            },
        };
    }
    /** The first unused entry whose `when` equals the program's input (none: matches anything). A miss is the fixture's fault. */
    take(path, input) {
        const entry = this.script[path]?.calls?.find((e) => !this.used.has(e) && (!('when' in e) || isDeepStrictEqual(e.when, input)));
        if (!entry)
            throw Object.assign(new Error(`no unused script entry for "${path}" matches the input ${JSON.stringify(input)}`), { harness: true });
        this.used.add(entry);
        return entry;
    }
}
/** A scripted program: level 0 acts when stdin closes; level 1 speaks the protocol over its pipes. */
class VProc extends EventEmitter {
    host;
    level;
    path;
    start;
    pid;
    up = true;
    /** `leaves`: a member of the group that outlives the process, ignores SIGTERM and ends only on SIGKILL (5.4). */
    member = false;
    lingers;
    ignoreTerm;
    inflight = new Map();
    stdout = new EventEmitter();
    stderr = new EventEmitter();
    stdin = Object.assign(new EventEmitter(), { write: (line) => this.onLine(line), end: (text) => (this.level ? this.shutdown() : this.onInput(text ?? '')) });
    constructor(host, level, path, start, pid) {
        super();
        this.host = host;
        this.level = level;
        this.path = path;
        this.start = start;
        this.pid = pid;
        this.ignoreTerm = start.ignoreTerm ?? false;
        this.lingers = start.leaves ?? false;
    }
    later(ms, fn) { return this.host.setTimeout(() => this.up && fn(), ms); }
    out(m) { this.stdout.emit('data', Buffer.from(typeof m === 'string' ? `${m}\n` : `${JSON.stringify(m)}\n`)); }
    signal(sig) {
        if (sig === 'SIGKILL')
            this.member = false;
        if (sig === 'SIGTERM' && this.ignoreTerm)
            return;
        this.die(null, sig);
    }
    /** The process ends: its group is empty at once, and the engine hears of it at once (no wake-up of its own). */
    die(code, sig = null) {
        if (!this.up)
            return;
        this.up = false;
        this.member = this.lingers && sig !== 'SIGKILL';
        this.stdout.emit('end');
        this.emit('exit', code, sig);
        this.emit('close', code, sig);
    }
    shutdown() { this.die(0); } // stdin closed: end of input
    // ---- level 0 ----
    onInput(text) {
        const entry = this.host.take(this.path, JSON.parse(text));
        if (entry.ignoreTerm)
            this.ignoreTerm = true;
        if (entry.leaves)
            this.lingers = true;
        this.later(entry.after ?? 0, () => this.finish(entry.reply));
    }
    finish(r) {
        if ('answer' in r) {
            this.stdout.emit('data', Buffer.from(JSON.stringify(r.answer)));
            this.die(0);
        }
        else if (r.quiet)
            this.die(0);
        else if ('error' in r) {
            this.stderr.emit('data', Buffer.from(String(r.error)));
            this.die(1);
        }
        else if ('junk' in r) {
            this.stdout.emit('data', Buffer.from(String(r.junk)));
            this.die(0);
        }
        else if (r.crash) {
            if (r.crash !== true && !/^SIG[A-Z0-9]+$/.test(String(r.crash)))
                throw Object.assign(new Error(`level 0 "crash" must be true or a signal name such as SIGUSR1, not ${JSON.stringify(r.crash)}`), { harness: true });
            this.die(null, r.crash === true ? 'SIGKILL' : r.crash);
        }
        else if (r.overlong)
            this.stdout.emit('data', Buffer.alloc(BIG + 1, 97));
        else if (r.spawnFails)
            this.emit('error', Object.assign(new Error('spawn ENOENT'), { code: 'ENOENT' })); // the program could not be started
        // hang: nothing, ever
    }
    // ---- level 1 ----
    onLine(line) {
        const m = JSON.parse(line);
        if (m.type === 'hello')
            return this.hello();
        if (m.type === 'ping') {
            const pong = this.start.pong ?? 0;
            if (pong !== false)
                this.later(pong, () => this.out({ type: 'pong', n: m.n }));
        }
        else if (m.type === 'signal') {
            const entry = this.host.take(this.path, m.inputs ? { payload: m.payload, inputs: m.inputs } : m.payload);
            if (entry.ignoreTerm)
                this.ignoreTerm = true;
            this.inflight.set(m.re, { entry, timer: entry.reply.hang ? undefined : this.later(entry.after ?? 0, () => this.send(m.re, entry.reply)) });
        }
        else if (m.type === 'cancel') {
            const call = this.inflight.get(m.re);
            if (!call?.entry.onCancel)
                return; // the neuron ignores it and finishes in its own time
            this.host.clearTimeout(call.timer);
            const { after, reply } = call.entry.onCancel;
            call.timer = this.later(after ?? 0, () => this.send(m.re, reply));
        }
    }
    hello() {
        const s = this.start;
        if (s.stderr)
            this.stderr.emit('data', Buffer.from(s.stderr));
        if (s.ready === false)
            return;
        this.later(s.ready ?? 0, () => {
            this.out('first' in s ? s.first : { v: s.readyV ?? 1, type: 'ready', ...(s.maxInflight !== undefined && { maxInflight: s.maxInflight }) });
            if (s.end)
                this.later(s.end.after ?? 0, () => {
                    if (s.end.how === 'closeStdout')
                        this.stdout.emit('end');
                    else if (s.end.how === 'error')
                        this.out({ type: 'error', message: 'scripted failure' });
                    else
                        this.die(1);
                });
        });
    }
    send(re, r) {
        this.inflight.delete(re);
        for (const m of r.also ?? [])
            this.out(typeof m === 'string' ? m : { re, ...m }); // other lines first (junk, unknown types, replies for other calls)
        if ('answer' in r)
            this.out({ type: 'answer', re, payload: r.answer, ...('replaces' in r && { replaces: r.replaces }), ...('to' in r && { to: r.to }) });
        else if (r.quiet)
            this.out({ type: 'quiet', re });
        else if ('error' in r)
            this.out({ type: 'error', re, message: r.error });
        else if ('junk' in r)
            this.out(String(r.junk));
        else if (r.crash)
            this.die(1);
        else if (r.overlong)
            this.stdout.emit('data', Buffer.alloc(BIG + 1, 97));
        else if ('send' in r)
            this.out({ re, ...r.send });
        // hang: nothing, ever
    }
}
//# sourceMappingURL=virtual.js.map