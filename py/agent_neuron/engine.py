"""A second agent-neuron engine, written from docs/SPEC.md and docs/CONFORMANCE.md only.

It runs a network file on the virtual clock of CONFORMANCE section 4 against scripted
programs (section 5).  Real processes are not spawned: no conformance fixture needs them
(CONFORMANCE: "A fixture never starts a process"), and this engine has no process layer.
Standard library only, Python 3.11+.
"""
import heapq
import json
from collections import deque

from .check import CheckError, load

UNSET = object()
GRACE = 2000
MIB = 1024 * 1024


class Refused(Exception):
    """The engine refuses to start (SPEC 3.2, 4.3) or refuses an injection (4.1, 10.2)."""


class Harness(Exception):
    """A scripted call had no matching entry (CONFORMANCE 5.1)."""


# ---------------------------------------------------------------- scheduler


class Sched:
    def __init__(self):
        self.heap = []
        self.seq = 0
        self.now = 0
        self.fg = 0
        self.grace = 0  # kill-grace timers: they keep the run open but are not work (see Server.restart)

    def at(self, t, fn, bg=False, grace=False):
        self.seq += 1
        item = [max(t, self.now), self.seq, fn, bg, True, grace]
        if grace:
            self.grace += 1
        heapq.heappush(self.heap, item)
        if not bg:
            self.fg += 1
        return item

    def cancel(self, item):
        if item and item[4]:
            item[4] = False
            if item[5]:
                self.grace -= 1
            if not item[3]:
                self.fg -= 1


# ---------------------------------------------------------------- data


class Sig:
    __slots__ = ("id", "trace", "frm", "to", "payload", "hops", "reply_to", "cause", "key", "has_payload")

    def __init__(self, id, trace, frm, to, payload, hops, key=None, reply_to=None, cause=None, has_payload=True):
        self.id, self.trace, self.frm, self.to = id, trace, frm, to
        self.payload, self.hops, self.key = payload, hops, key
        self.reply_to, self.cause, self.has_payload = reply_to, cause, has_payload


class Trace:
    def __init__(self, id, ctx, pool):
        self.id, self.ctx, self.pool = id, ctx, pool
        self.pending = 0
        self.exhausted = False
        self.finished = False
        self.failed = False


class Ctx:
    """The scope of one run of a network: the outer network, or one call of a composed neuron."""

    def __init__(self, inst, report, call=None):
        self.inst, self.report, self.call = inst, report, call
        self.registry = {}
        self.calls = set()
        self.held = set()
        self.closed = False


class Work:
    def __init__(self, sig, neuron, ctx):
        self.sig, self.neuron, self.ctx = sig, neuron, ctx
        self.state = "new"  # held | consumed | dropped | done
        self.call = None
        self.timer = None


class Call:
    def __init__(self, neuron, inputs, cross, ctx):
        self.neuron, self.inputs, self.cross, self.ctx = neuron, inputs, cross, ctx
        self.trace = cross.sig.trace
        self.state = "new"  # queued | running | done
        self.stale = False
        self.cancelled = False
        self.reserved = False
        self.committed = False
        self.t_reserve = 0
        self.timeout_item = None
        self.reply_item = None
        self.hang_item = None
        self.entry = None
        self.group = None
        self.re = None
        self.server = None
        self.inner_ctx = None


class Group:
    """A process group of a scripted program (CONFORMANCE 5.4)."""

    def __init__(self, ignore_term=False, leaves=False):
        self.proc = True
        self.survivor = False
        self.ignore_term = ignore_term
        self.leaves = leaves
        self.kill_item = None
        self.on_gone = None

    def live(self):
        return self.proc or self.survivor


class Neuron:
    def __init__(self, run, inst, spec):
        self.run, self.inst, self.spec = run, inst, spec
        self.id = spec["id"]
        self.path = inst.prefix + self.id
        self.kind = next(k for k in ("source", "run", "serve", "http", "human", "network") if k in spec)
        self.threshold = spec.get("threshold")
        self.within = spec.get("within", 10000)
        self.maxhops = spec.get("maxHops")
        self.timeout = spec.get("timeout", 30000)
        self.cost = spec.get("cost")
        self.effect = spec.get("effect") is True
        self.report = inst.report
        self.groups = {}
        script = run.script.get(self.path) or {}
        self.calls = [dict(c, _used=False) for c in script.get("calls", [])]
        self.starts = script.get("starts", [])
        self.server = None
        self.inner = None

    @property
    def out(self):
        return self.inst.out[self.id]


class Instance:
    def __init__(self, run, data, prefix, report):
        self.run, self.prefix, self.report = run, prefix, report
        self.budget = data.get("budget")
        self.entry, self.exit = data.get("entry"), data.get("exit")
        self.out = {n["id"]: [] for n in data["neurons"]}
        for l in data.get("links", []):
            self.out[l["from"]].append(l["to"])
        self.neurons = {}
        for spec in data["neurons"]:
            n = Neuron(run, self, spec)
            self.neurons[spec["id"]] = n
            if n.kind == "network":
                n.inner = Instance(run, data["_inner"][spec["id"]], n.path + "/", False)
            if n.kind == "serve":
                n.server = Server(run, n)

    def servers(self):
        for n in self.neurons.values():
            if n.server:
                yield n.server
            if n.inner:
                yield from n.inner.servers()


def _payload(p):
    return {"type": "text", "text": p} if isinstance(p, str) else {"type": "json", "data": p}


# ---------------------------------------------------------------- level 1 server


class Proc1:
    def __init__(self, server, gen, cfg, t0):
        self.server, self.gen, self.cfg, self.t0 = server, gen, cfg, t0
        self.up = False
        self.dead = False
        self.ready_t = None
        self.group = Group(bool(cfg.get("ignoreTerm")), bool(cfg.get("leaves")))
        self.lines = 0
        self.ping_n = 0
        self.ping_pending = False
        self.items = []
        self.hang_item = None


class Server:
    def __init__(self, run, neuron):
        self.run, self.n = run, neuron
        self.gen = 0
        self.proc = None
        self.f = 0
        self.state = "init"  # starting | up | down | failed
        self.slots = {}
        self.queue = deque()
        self.max_inflight = 1
        self.re = 0

    # --- start

    def ready_plan(self, cfg):
        """(latency or None if never ready/invalid, max_inflight)"""
        lat = cfg.get("ready", 0)
        if lat is False:
            return None, 1
        if "first" in cfg:
            line = cfg["first"]
            if isinstance(line, str):
                try:
                    line = json.loads(line)
                except ValueError:
                    return None, 1
        else:
            line = {"type": "ready", "v": cfg.get("readyV", 1)}
            if "maxInflight" in cfg:
                line["maxInflight"] = cfg["maxInflight"]
        if not isinstance(line, dict) or line.get("type") != "ready" or line.get("v") != 1 or isinstance(line.get("v"), bool):
            return None, 1
        mi = line.get("maxInflight", 1)
        if not isinstance(mi, int) or isinstance(mi, bool) or mi < 1:
            return None, 1
        if lat > self.n.timeout:
            return None, 1
        return lat, mi

    def first_latency(self):
        cfg = self.n.starts[0] if self.n.starts else {}
        lat, _ = self.ready_plan(cfg)
        if lat is None:
            raise Refused(f"{self.n.path}: no valid ready within timeout at the first start")
        return lat

    def spawn(self, first=False):
        r, n, s = self.run, self.n, self.run.sched
        self.gen += 1
        cfg = n.starts[self.gen - 1] if self.gen - 1 < len(n.starts) else {}
        p = Proc1(self, self.gen, cfg, s.now)
        self.proc = p
        self.state = "starting"
        lat, mi = self.ready_plan(cfg)
        if lat is None:
            p.items.append(s.at(s.now + n.timeout, lambda: self.start_failed(p)))
        else:
            p.items.append(s.at(s.now + lat, lambda: self.ready(p, mi)))

    def restart(self):
        s = self.run.sched
        if s.fg - s.grace <= 0 and not self.run.queued():
            return  # nothing but a kill grace is left: the run is over, programs stop silently (6.2 rule 6)
        self.run.note(self.n, "restart")
        self.spawn()

    def ready(self, p, mi):
        if p.dead:
            return
        s = self.run.sched
        p.up, p.ready_t, p.lines = True, s.now, 1
        self.state = "up"
        self.max_inflight = mi
        end = p.cfg.get("end")
        if end:
            p.items.append(s.at(s.now + end.get("after", 0), lambda: self.end(p, end.get("how", "exit"))))
        p.items.append(s.at(s.now + self.run.ping_every, lambda: self.tick(p), bg=True))
        self.dispatch()

    def start_failed(self, p):
        if p.dead:
            return
        self.process_down(p, "unready")

    # --- processes dying

    def end(self, p, how):
        if p.dead:
            return
        if how == "exit":
            p.group.proc = False
            if p.group.leaves:
                p.group.survivor = True
            self.process_down(p, "exit")
        elif how == "closeStdout":
            self.process_down(p, "closeStdout")
        else:
            self.process_down(p, "error")

    def process_down(self, p, why, skip=None):
        """The process failed (exit, hang, bad line, ...): fail the running calls, kill the rest, restart or give up."""
        r, s = self.run, self.run.sched
        if p.dead:
            return
        p.dead = True
        for it in p.items:
            s.cancel(it)
        s.cancel(p.hang_item)
        self.state = "down"
        # running calls fail with crash; cancelled ones are only freed
        for re, call in sorted(self.slots.items()):
            s.cancel(call.hang_item)
            s.cancel(call.reply_item)
            s.cancel(call.timeout_item)
            if call is skip:
                continue
            if call.cancelled:
                r.program_over(call)
            else:
                r.fail(call, "crash", "process failed")
                r.release(call)
        self.slots = {}
        # kill what remains of the group
        g = p.group
        # restart accounting
        if why == "hang" or why == "unready":
            self.f += 1
        elif p.ready_t is not None:
            life = s.now - p.ready_t
            if life < 1000:
                self.f += 1
            else:
                self.f = 0
        else:
            self.f += 1
        failed = self.f >= 5
        if failed:
            self.state = "failed"
            q, self.queue = self.queue, deque()
            for call in q:
                r.fail(call, "start", "neuron failed to start")
                r.release(call)

        if not failed:
            delay = min(100 * 2 ** self.f, 5000)
            s.at(s.now + delay, self.restart, bg=True)

        r.term_group(g, self.n, None)

    # --- pings

    def tick(self, p):
        if p.dead:
            return
        s = self.run.sched
        if not self.slots and not p.ping_pending:
            p.ping_pending = True
            p.ping_n += 1
            pong = p.cfg.get("pong", 0)
            if pong is False:
                p.hang_item = s.at(s.now + self.n.timeout, lambda: self.process_down(p, "hang"), bg=True)
            else:
                p.hang_item = s.at(s.now + pong, lambda: self.pong(p), bg=True)
        p.items.append(s.at(s.now + self.run.ping_every, lambda: self.tick(p), bg=True))

    def pong(self, p):
        if p.dead:
            return
        p.lines += 1
        self.pong_clear(p)

    def pong_clear(self, p):
        p.ping_pending = False
        self.f = 0
        self.run.sched.cancel(p.hang_item)

    # --- calls

    def submit(self, call):
        r = self.run
        if self.state == "failed":
            r.fail(call, "start", "neuron failed to start")
            r.release(call)
            return
        call.server = self
        call.state = "queued"
        self.queue.append(call)
        self.dispatch()

    def dispatch(self):
        r = self.run
        while self.state == "up" and self.queue and len(self.slots) < self.max_inflight:
            call = self.queue.popleft()
            if call.state != "queued":
                continue
            if not r.reserve(call):
                r.release(call)
                continue
            self.start_call(call)

    def start_call(self, call):
        r, s, p = self.run, self.run.sched, self.proc
        call.state = "running"
        self.re += 1
        call.re = self.re
        self.slots[call.re] = call
        value = r.input_value(call)
        call.entry = entry = r.pick(self.n, value)
        reply = entry.get("reply", {})
        call.reply_item = s.at(s.now + entry.get("after", 0), lambda: self.deliver(p, call, reply, True))
        call.timeout_item = s.at(s.now + self.n.timeout, lambda: r.timeout(call))

    def deliver(self, p, call, reply, main):
        if p.dead or p is not self.proc:
            return
        entry = call.entry
        re = call.re
        if main:
            for line in reply.get("also", []):
                if p.dead:
                    return
                self.line(p, line if isinstance(line, str) else dict({"re": re}, **line))
        if p.dead:
            return
        if "answer" in reply:
            m = {"type": "answer", "re": re, "payload": reply["answer"]}
            for k in ("replaces", "to"):
                if k in reply:
                    m[k] = reply[k]
            self.line(p, m)
        elif reply.get("quiet"):
            self.line(p, {"type": "quiet", "re": re})
        elif "error" in reply:
            self.line(p, {"type": "error", "re": re, "message": reply["error"]})
        elif "junk" in reply:
            self.line(p, reply["junk"] if isinstance(reply["junk"], str) else str(reply["junk"]))
        elif "crash" in reply:
            p.group.proc = False
            if p.group.leaves:
                p.group.survivor = True
            self.process_down(p, "exit")
        elif reply.get("overlong"):
            self.line(p, ("overlong",))
        elif "send" in reply:
            self.line(p, dict({"re": re}, **reply["send"]))

    def cancel(self, call):
        """Send `cancel` for a call that holds a slot (the note is the caller's)."""
        s, r = self.run.sched, self.run
        entry = call.entry or {}
        oc = entry.get("onCancel")
        if oc is not None and call.state == "running":
            s.cancel(call.reply_item)
            p = self.proc
            call.reply_item = s.at(s.now + oc.get("after", 0), lambda: self.deliver(p, call, oc.get("reply", {}), False))
        s.cancel(call.hang_item)
        call.hang_item = s.at(s.now + self.n.timeout, lambda: self.hung(call))

    def hung(self, call):
        if self.slots.get(call.re) is call and self.proc and not self.proc.dead:
            self.process_down(self.proc, "hang")

    def free_slot(self, call):
        self.run.sched.cancel(call.hang_item)
        if self.slots.get(call.re) is call:
            del self.slots[call.re]
        self.dispatch()

    # --- reading a line from the neuron

    def line(self, p, line):
        r = self.run
        p.lines += 1
        if isinstance(line, tuple):  # an over-long line
            running = list(self.slots.values())
            if len(running) == 1 and not running[0].cancelled:
                call = running[0]
                r.fail(call, "output", "output too large")
                r.release(call)
                self.slots.pop(call.re, None)
                r.sched.cancel(call.timeout_item)
                self.process_down(p, "overlong", skip=call)
            else:
                self.process_down(p, "overlong")
            return
        msg = line
        if isinstance(line, str):
            try:
                msg = json.loads(line)
            except ValueError:
                msg = None
            if not isinstance(msg, dict) or not isinstance(msg.get("type"), str):
                self.junk(p, line)
                return
        t = msg.get("type")
        if not isinstance(t, str):
            self.junk(p, json.dumps(msg))
            return
        if t == "pong":
            if p.ping_pending:
                self.pong_clear(p)
            return
        if t in ("answer", "quiet", "error"):
            if t == "error" and "re" not in msg:
                self.process_down(p, "error")
                return
            re = msg.get("re")
            call = self.slots.get(re) if isinstance(re, int) and not isinstance(re, bool) else None
            if call is None:
                return
            if call.cancelled:
                self.free_slot_cancelled(call)
                return
            self.reply(p, call, msg)

    def free_slot_cancelled(self, call):
        r = self.run
        r.sched.cancel(call.reply_item)
        del self.slots[call.re]
        r.program_over(call)
        self.dispatch()

    def junk(self, p, text):
        r = self.run
        r.note(self.n, "junk", line=p.lines, text=text[:2000])
        if len(self.slots) == 1:
            call = next(iter(self.slots.values()))
            if not call.cancelled:
                r.sched.cancel(call.timeout_item)
                r.sched.cancel(call.reply_item)
                del self.slots[call.re]
                r.fail(call, "junk", "junk on stdout: " + text[:100])
                r.release(call)
                self.dispatch()

    def reply(self, p, call, msg):
        r, s = self.run, self.run.sched
        n = self.n
        t = msg["type"]

        def finish():
            s.cancel(call.timeout_item)
            s.cancel(call.reply_item)
            del self.slots[call.re]

        if t == "error":
            finish()
            if not isinstance(msg.get("message"), str):
                r.fail(call, "protocol", "protocol: error without message")
                r.release(call)
                self.dispatch()
                return
            r.fail(call, "neuron", msg["message"])
            r.release(call)
            self.dispatch()
            return
        if t == "quiet":
            finish()
            r.do_quiet(call)
            self.dispatch()
            return
        # answer: validate
        problem = None
        if "payload" not in msg:
            problem = "answer without payload"
        rep = msg.get("replaces", UNSET)
        if rep is not UNSET and rep is not None:
            if not isinstance(rep, str):
                problem = "replaces is not a string"
            elif len(rep.encode("utf-8")) > 256:
                problem = "replaces longer than 256 bytes"
        to = msg.get("to")
        if "to" in msg:
            if not isinstance(to, list) or any(not isinstance(x, str) or x not in n.out for x in to):
                problem = "to names a neuron that is not linked"
        if problem:
            finish()
            r.fail(call, "protocol", "protocol: " + problem)
            r.release(call)
            self.dispatch()
            return
        finish()
        r.do_answer(call, msg["payload"], rep, to)
        self.dispatch()


# ---------------------------------------------------------------- the engine


class Run:
    def __init__(self, path, inputs, script=None, ping_every=10000, observer=None, on_event=None):
        try:
            data, _ = load(path)
        except CheckError as e:
            raise Refused(str(e))
        self.script = script or {}
        self.ping_every = ping_every
        self.sched = Sched()
        self.events = []
        self.on_event = on_event
        self.stream_buf = []
        self.next_id = 0
        self.touched = []
        self.observer = [dict(r, _fired=False) for r in (observer or [])]
        self.root = Instance(self, data, "", True)
        self.ctx = Ctx(self.root, True)
        self.inputs = inputs
        self.epoch = 0
        self.stopped = False
        self._validate_inputs()

    # --- helpers

    def nid(self, p="n"):
        self.next_id += 1
        return f"{p}{self.next_id}"

    def emit(self, ctx, ev):
        if not ctx.report:
            return
        ev["t"] = self.sched.now - self.epoch
        self.events.append(ev)
        if self.on_event:
            self.stream_buf.append(json.loads(json.dumps(ev)))
        if "id" in ev or ev["type"] == "finished":
            for rule in self.observer:
                if rule["_fired"]:
                    continue
                if all(k in ev and ev[k] == v for k, v in rule["when"].items()):
                    rule["_fired"] = True
                    trig = ev.get("id", ev.get("trace"))
                    inj = rule["inject"]
                    self.sched.at(self.sched.now + inj.get("after", 0), lambda inj=inj, trig=trig: self.do_inject(inj, trig))

    def note(self, neuron, what, **kw):
        if not neuron.report:
            return
        ev = {"type": "note", "neuron": neuron.id, "what": what}
        ev.update(kw)
        self.emit(self.ctx, ev)

    def sigev(self, typ, s, key=False):
        ev = {"type": typ, "id": s.id, "from": s.frm, "to": list(s.to), "trace": s.trace.id, "hops": s.hops}
        if s.reply_to is not None:
            ev["replyTo"] = s.reply_to
        if s.cause is not None:
            ev["cause"] = s.cause
        if key and s.key is not None:
            ev["replaces"] = s.key
        if s.has_payload:
            ev["payload"] = _payload(s.payload)
        return ev

    def queued(self):
        """A call waiting for a slot keeps the run open (6.2 rule 6), even when only a restart delay is due."""
        return any(sv.queue for sv in self.root.servers())

    def touch(self, tr):
        self.touched.append(tr)

    # --- validation and start

    def _validate_inputs(self):
        for inp in self.inputs:
            n = self.root.neurons.get(inp["source"])
            if n is None or n.kind != "source":
                raise Refused(f"injection into {inp['source']!r}: not a source")
            k = inp.get("replaces")
            if k is not None and len(k.encode("utf-8")) > 256:
                raise Refused("replaces key longer than 256 bytes")
            b = inp.get("budget")
            if b is not None:
                ceil = self.root.budget
                if ceil is None or not isinstance(b, dict):
                    raise Refused("injection budget without a budget")
                for u, a in b.items():
                    if u not in ceil or not isinstance(a, int) or isinstance(a, bool) or a < 0 or a > ceil[u]:
                        raise Refused("an injection may only lower the budget")

    def start(self):
        def kinds(inst):
            for n in inst.neurons.values():
                yield n.kind
                if n.inner:
                    yield from kinds(n.inner)
        bad = sorted({k for k in kinds(self.root) if k == "http"})
        if bad:
            raise Refused(f"{bad[0]} neurons are not run by this engine")
        servers = list(self.root.servers())
        self.epoch = max([sv.first_latency() for sv in servers] + [0])
        for sv in servers:
            sv.spawn(first=True)
        for inp in self.inputs:
            self.sched.at(self.epoch + inp["at"], lambda inp=inp: self.inject(inp))

    def execute(self):
        self.start()
        s = self.sched
        while s.fg > 0 or (s.heap and self.queued()):
            item = heapq.heappop(s.heap)
            if not item[4]:
                continue
            item[4] = False
            if item[5]:
                s.grace -= 1
            if not item[3]:
                s.fg -= 1
            s.now = item[0]
            item[2]()
            self.settle()
        return self.events

    def settle(self):
        while self.touched:
            tr, self.touched = self.touched, []
            for t in tr:
                if t.pending == 0 and not t.finished:
                    ctx = t.ctx
                    if ctx.report:
                        t.finished = True
                        ev = {"type": "finished", "trace": t.id, "status": "exhausted" if t.exhausted else "done"}
                        if t.pool is not None:
                            ev["remaining"] = dict(t.pool)
                        self.emit(ctx, ev)
                    elif ctx.call is not None and not ctx.closed:
                        t.finished = True
                        call = ctx.call
                        self.close_ctx(ctx)
                        if t.failed:
                            self.fail(call, "inner", "no answer: an inner neuron failed")
                            self.release(call)
                        else:
                            self.do_quiet(call)
        self.flush_stream()

    def flush_stream(self):
        if self.on_event and self.stream_buf:
            buf, self.stream_buf = self.stream_buf, []
            for ev in buf:
                try:
                    self.on_event(ev)
                except BaseException:
                    pass

    # --- injection and observers

    def inject(self, inp):
        ctx = self.ctx
        src = self.root.neurons[inp["source"]]
        pool = None
        if self.root.budget is not None:
            pool = dict(self.root.budget)
            for u, a in (inp.get("budget") or {}).items():
                pool[u] = a
        sid = self.nid("s")
        tr = Trace(sid, ctx, pool)
        s = Sig(sid, tr, src.id, list(src.out), inp.get("payload"), 0, inp.get("replaces"))
        self.send(ctx, s)
        self.touch(tr)

    def do_inject(self, inj, trigger):
        src = self.root.neurons[inj["source"]]
        self.note(src, "inject", trigger=trigger)
        self.inject({"source": inj["source"], "payload": inj.get("payload"), "replaces": inj.get("replaces")})

    # --- sending a signal

    def send(self, ctx, s):
        if ctx.closed:
            return
        s.trace.pending += len(s.to)  # "still being delivered": the trace is not finished meanwhile
        self.touch(s.trace)
        self.emit(ctx, self.sigev("signal", s, key=True))
        if s.key is not None:
            old = ctx.registry.get((s.frm, s.key))
            if old:
                ctx.registry[(s.frm, s.key)] = []
                for w in old:
                    self.make_stale(w)
                self.settle()
        for tgt in s.to:
            self.deliver(ctx, s, ctx.inst.neurons[tgt])

    def make_stale(self, w):
        if w.state == "held":
            self.drop(w, "replaced", "waiting")
            self.unhold(w)
        elif w.state == "consumed":
            call = w.call
            if call.stale or call.state == "done":
                return
            call.stale = True
            if call.state == "queued":
                try:
                    call.server.queue.remove(call)
                except ValueError:
                    pass
                self.drop_call(call, "replaced", "waiting")
                self.release(call)
            elif call.state == "running" and not call.neuron.effect and not call.cancelled:
                self.cut(call)

    def drop(self, w, reason, phase=None):
        s = w.sig
        ev = self.sigev("drop", s)
        ev["neuron"] = w.neuron.id
        ev["reason"] = reason
        if phase:
            ev["phase"] = phase
        self.emit(w.ctx, ev)

    def drop_call(self, call, reason, phase):
        self.drop(call.cross, reason, phase)

    def unhold(self, w):
        grp = w.neuron.groups.get(w.sig.trace.id, [])
        if w in grp:
            grp.remove(w)
        self.sched.cancel(w.timer)
        w.ctx.held.discard(w)
        w.state = "dropped"
        w.sig.trace.pending -= 1
        self.touch(w.sig.trace)

    def deliver(self, ctx, s, n):
        w = Work(s, n, ctx)
        if s.key is not None:
            ctx.registry.setdefault((s.frm, s.key), []).append(w)
        if n.maxhops is not None and s.hops > n.maxhops:
            w.state = "dropped"
            self.drop(w, "hops")
            s.trace.pending -= 1
            self.touch(s.trace)
            return
        if n.threshold and n.threshold > 1:
            grp = n.groups.setdefault(s.trace.id, [])
            old = next((x for x in grp if x.sig.frm == s.frm), None)
            if old is not None:
                self.drop(old, "replaced", "waiting")
                self.unhold(old)
                grp = n.groups.setdefault(s.trace.id, [])
            grp.append(w)
            w.state = "held"
            ctx.held.add(w)
            if len(grp) >= n.threshold:
                inputs = list(grp)
                n.groups[s.trace.id] = []
                for x in inputs:
                    self.sched.cancel(x.timer)
                    ctx.held.discard(x)
                    x.state = "consumed"
                s.trace.pending -= len(inputs) - 1
                call = Call(n, inputs, w, ctx)
                for x in inputs:
                    x.call = call
                self.begin(call)
            else:
                w.timer = self.sched.at(self.sched.now + n.within, lambda: self.expire(w))
        else:
            w.state = "consumed"
            call = Call(n, [w], w, ctx)
            w.call = call
            self.begin(call)

    def expire(self, w):
        if w.state != "held":
            return
        self.drop(w, "expired")
        self.unhold(w)

    # --- starting a call

    def input_value(self, call):
        if call.neuron.threshold and call.neuron.threshold > 1:
            return {"payload": call.cross.sig.payload, "inputs": [w.sig.payload for w in call.inputs]}
        return call.cross.sig.payload

    def pick(self, n, value):
        for e in n.calls:
            if not e["_used"] and ("when" not in e or e["when"] == value):
                e["_used"] = True
                return e
        raise Harness(f"{n.path}: no scripted call matches {json.dumps(value)[:100]}")

    def begin(self, call):
        n = call.neuron
        call.ctx.calls.add(call)
        if n.kind == "serve":
            n.server.submit(call)
            return
        if not self.reserve(call):
            self.release(call)
            return
        if n.kind == "run":
            self.start_l0(call)
        elif n.kind == "network":
            self.start_composed(call)
        elif n.kind == "human":
            self.start_human(call)
        else:
            raise Refused(f"{n.kind} neurons are not run by this engine")

    def reserve(self, call):
        tr, n = call.trace, call.neuron
        call.state = "running" if n.kind != "serve" else call.state
        if tr.pool is None:
            return True
        cost = n.cost or {}
        short = [u for u, a in cost.items() if tr.pool.get(u, 0) < a]
        if short:
            tr.exhausted = True
            u = short[0]
            self.fail(call, "budget", f"budget: needs {cost[u]} {u}, {tr.pool.get(u, 0)} left", commit=False)
            return False
        for u, a in cost.items():
            tr.pool[u] -= a
        call.reserved = True
        call.t_reserve = self.sched.now
        if n.cost is not None:
            self.note(n, "reserve", units=dict(n.cost))
        return True

    def commit(self, call):
        if call.reserved and not call.committed:
            call.committed = True
            n = call.neuron
            if n.cost is not None:
                self.note(n, "commit", units=dict(n.cost), ms=self.sched.now - call.t_reserve, basis="estimated")

    def release(self, call):
        """The call is over for the trace."""
        if call.state == "done":
            return
        call.state = "done"
        call.ctx.calls.discard(call)
        for w in call.inputs:
            w.state = "done"
        call.trace.pending -= 1
        self.touch(call.trace)

    def program_over(self, call):
        """A cancelled call's program is gone."""
        self.commit(call)
        self.release(call)

    # --- outcomes

    def fail(self, call, code, message, commit=True, never_ran=False):
        n = call.neuron
        if commit:
            self.commit(call)
        typ = "unknown" if n.effect and code not in ("budget", "start", "neuron") and not never_ran else "error"
        ev = self.sigev(typ, call.cross.sig)
        ev["neuron"], ev["message"], ev["code"] = n.id, message, code
        self.emit(call.ctx, ev)
        call.trace.failed = True

    def do_quiet(self, call):
        n, cross = call.neuron, call.cross.sig
        self.commit(call)
        a = Sig(self.nid("s"), call.trace, n.id, [cross.frm], None, cross.hops + 1, reply_to=cross.id, has_payload=False)
        self.emit(call.ctx, self.sigev("quiet", a))
        ctx = call.ctx
        self.release(call)
        self.maybe_exit(ctx, n, "quiet", None, UNSET)

    def do_answer(self, call, payload, replaces=UNSET, to=None):
        n, cross, ctx = call.neuron, call.cross.sig, call.ctx
        self.commit(call)
        a = Sig(self.nid("s"), call.trace, n.id, [cross.frm], payload, cross.hops + 1, reply_to=cross.id)
        ev = self.sigev("answer", a)
        eff_stale = call.stale and n.effect
        if eff_stale:
            ev["forwarded"] = False
        self.emit(ctx, ev)
        if not eff_stale:
            targets = n.out
            if to is not None:
                targets = [t for t in n.out if t in to]
            if targets:
                key = cross.key if replaces is UNSET else replaces
                f = Sig(self.nid("s"), call.trace, n.id, list(targets), payload, a.hops + 1, key, cause=a.id)
                self.release(call)  # an answered call is finished: its own forward cannot cut it
                self.send(ctx, f)
        self.release(call)
        self.maybe_exit(ctx, n, "answer", payload, replaces)

    def maybe_exit(self, ctx, n, kind, payload, replaces):
        if ctx.call is not None and n.id == ctx.inst.exit and not ctx.closed:
            outer = ctx.call
            self.close_ctx(ctx)
            if outer.state == "done":
                return
            if kind == "answer":
                self.do_answer(outer, payload, replaces)
            else:
                self.do_quiet(outer)

    # --- cutting a running call (stale or timeout)

    def cut(self, call):
        """Stale, running, not an effect: drop(running), cancel note, stop the program."""
        self.drop_call(call, "replaced", "running")
        self.cancel_program(call)

    def cancel_program(self, call):
        n, s = call.neuron, self.sched
        call.cancelled = True
        s.cancel(call.timeout_item)
        self.note(n, "cancel")
        if n.kind == "run":
            self.term_group(call.group, n, lambda: self.program_over(call))
        elif n.kind == "serve":
            n.server.cancel(call)
        elif n.kind == "network":
            self.close_ctx(call.inner_ctx)
            self.program_over(call)
        elif n.kind == "human":
            s.cancel(call.reply_item)  # the question is withdrawn: its pending reply never comes
            self.program_over(call)

    def timeout(self, call):
        n = call.neuron
        if call.cancelled or call.state != "running":
            return
        if n.kind == "network" and call.inner_ctx.closed:
            return
        self.fail(call, "timeout", f"timed out after {n.timeout} ms", commit=False)
        self.cancel_program(call)

    # --- people (CONFORMANCE 5.5)

    def start_human(self, call):
        """The scripted person: the first matching call entry; its reply `after` ms later, set before the timeout."""
        n, s = call.neuron, self.sched
        entry = self.pick(n, self.input_value(call))
        reply = entry.get("reply", {})
        if not (reply.get("hang") is True or reply.get("quiet") is True or isinstance(reply.get("answer"), str)):
            raise Harness(f'{n.path}: a person replies {{"answer": text}}, {{"quiet": true}} or {{"hang": true}}, not {json.dumps(reply)[:100]}')
        call.reply_item = None if reply.get("hang") else s.at(s.now + entry.get("after", 0), lambda: self.human_outcome(call, reply))
        call.timeout_item = s.at(s.now + n.timeout, lambda: self.timeout(call))

    def human_outcome(self, call, reply):
        if call.cancelled or call.state != "running":
            return
        self.sched.cancel(call.timeout_item)
        if reply.get("quiet"):
            self.do_quiet(call)
        else:
            self.do_answer(call, reply["answer"])

    # --- level 0

    def start_l0(self, call):
        n, s = call.neuron, self.sched
        value = self.input_value(call)
        call.entry = entry = self.pick(n, value)
        reply = entry.get("reply", {})
        call.group = Group(bool(entry.get("ignoreTerm")), bool(entry.get("leaves")))
        if reply.get("spawnFails"):
            self.fail(call, "exit", "exit 127: cannot start the program", never_ran=True)
            self.release(call)
            return
        call.reply_item = s.at(s.now + entry.get("after", 0), lambda: self.l0_outcome(call, reply)) if not reply.get("hang") else None
        call.timeout_item = s.at(s.now + n.timeout, lambda: self.timeout(call))

    def l0_outcome(self, call, reply):
        n, g = call.neuron, call.group
        was_alive = g.proc
        g.proc = False
        if was_alive and g.leaves and call.cancelled:
            g.survivor = True
        if call.cancelled:
            if not g.live() and g.on_gone:
                self.sched.cancel(g.kill_item)
                cb, g.on_gone = g.on_gone, None
                cb()
            return
        if call.state != "running":
            return
        self.sched.cancel(call.timeout_item)
        if "answer" in reply:
            self.do_answer(call, reply["answer"], reply.get("replaces", UNSET), reply.get("to"))
        elif reply.get("quiet"):
            self.do_quiet(call)
        elif "error" in reply:
            self.fail(call, "exit", "exit 1: " + str(reply["error"])[:500].strip())
            self.release(call)
        elif "junk" in reply:
            self.fail(call, "junk", "not JSON: " + str(reply["junk"])[:100])
            self.release(call)
        elif "crash" in reply:
            sig = reply["crash"] if isinstance(reply["crash"], str) else "SIGKILL"
            self.fail(call, "exit", f"signal {sig}: ")
            self.release(call)
        elif reply.get("overlong"):
            self.fail(call, "output", "output too large")
            self.release(call)
            self.settle()
            self.note(n, "kill")

    def term_group(self, g, neuron, on_gone):
        """SIGTERM now, SIGKILL after the grace if anything remains.  Notes only for a live group."""
        s = self.sched
        if not g.live():
            if on_gone:
                on_gone()
            return
        self.note(neuron, "term")
        if g.proc and not g.ignore_term:
            g.proc = False
            if g.leaves:
                g.survivor = True
        if not g.live():
            if on_gone:
                on_gone()
            return
        g.on_gone = on_gone

        def kill():
            if g.live():
                self.note(neuron, "kill")
                g.proc = g.survivor = False
            cb, g.on_gone = g.on_gone, None
            if cb:
                cb()

        g.kill_item = s.at(s.now + GRACE, kill, grace=True)

    # --- composed neurons

    def start_composed(self, call):
        n, ctx = call.neuron, call.ctx
        inner = n.inner
        ictx = Ctx(inner, False, call)
        call.inner_ctx = ictx
        cost, ib = n.cost, inner.budget
        if cost is not None:
            pool = {u: (min(a, ib[u]) if ib and u in ib else a) for u, a in cost.items()}
        else:
            pool = dict(ib) if ib is not None else None
        sid = self.nid("s")
        tr = Trace(sid, ictx, pool)
        cross = call.cross.sig
        s = Sig(sid, tr, n.id, [inner.entry], cross.payload, 0, cross.key)
        call.timeout_item = self.sched.at(self.sched.now + n.timeout, lambda: self.timeout(call))
        self.send(ictx, s)
        self.touch(tr)

    def close_ctx(self, ctx):
        """Cancel everything still running inside a composed call, without reports."""
        if ctx.closed:
            return
        ctx.closed = True
        for w in list(ctx.held):
            self.unhold(w)
        for call in list(ctx.calls):
            if call.state == "queued":
                try:
                    call.server.queue.remove(call)
                except ValueError:
                    pass
                self.release(call)
            elif call.state == "running" and not call.cancelled:
                self.sched.cancel(call.timeout_item)
                call.cancelled = True
                n = call.neuron
                if n.kind == "run":
                    self.term_group(call.group, n, lambda call=call: self.program_over(call))
                elif n.kind == "serve":
                    n.server.cancel(call)
                elif n.kind == "network":
                    self.close_ctx(call.inner_ctx)
                    self.program_over(call)
