# agent-neuron specification, version 0

Status: draft. Version 0 may change in any way until it is called 1; a change
is one commit to this file, `spec/network.schema.json` and the fixtures
together. MUST, MUST NOT, SHOULD and MAY are used as in RFC 2119. Each section
is marked **normative** (rules) or **informative** (no rules). If they
disagree, the normative one wins. Reasons for the decisions are in
[decisions/](decisions/0001-spec-v0.md), not here.

Contents

0. Quick start for neuron authors (informative)
1. Terms (informative)
2. The network file (normative)
3. The neuron contract: level 0, level 1, http, human (normative)
4. Routing (normative)
5. Threshold, hops and staleness (normative)
6. Failures and event order (normative)
7. The trace (normative)
8. Conformance (normative)
9. Observers (normative)

Appendices (informative): A. Walk-through: one alert. B. Out of version 0.
C. How the TypeScript library differs.

## 0. Quick start for neuron authors (informative)

A neuron is a program. The engine starts it, gives it JSON on stdin, and reads
JSON from stdout. You do not link a library.

**Level 0** (default, `"run": [...]`): one process per signal. Read one JSON
value, print one JSON value, or print nothing to say nothing.

```python
import json, sys
alert = json.load(sys.stdin)                 # the payload: any JSON value
if isinstance(alert, dict) and alert.get("severity") == "crit":
    print(json.dumps({"service": alert["service"]}))   # an answer
# printing nothing is "quiet": nothing is forwarded
```

Try it by hand: `echo '{"severity":"crit","service":"gateway"}' | python3 flag.py`.

A neuron whose `threshold` is more than 1 gets one value shaped
`{"payload": P, "inputs": [P1, P2, ...]}` (`inputs` in arrival order, the
input that crossed last, `payload` that same input). Any other neuron gets the
bare payload:

```json
{"payload": {"d": 2}, "inputs": [{"d": 1}, {"d": 2}]}
```

**Level 1** (`"serve": [...]`): one long-lived process, JSON lines. Use it for
speed or for state kept between calls.

```python
import json, sys

def send(m):
    sys.stdout.write(json.dumps(m) + "\n")
    sys.stdout.flush()                       # always flush

json.loads(sys.stdin.readline())             # hello
send({"v": 1, "type": "ready", "maxInflight": 1})
for line in sys.stdin:                       # end of input = shut down
    m = json.loads(line)
    if m["type"] == "signal":
        # the payload may be any JSON value; "inputs" is present only if threshold > 1
        values = m.get("inputs", [m["payload"]])
        n = sum(v.get("n", 0) for v in values if isinstance(v, dict))
        if n:
            send({"type": "answer", "re": m["re"], "payload": {"n": n}})
        else:
            send({"type": "quiet", "re": m["re"]})
    elif m["type"] == "ping":
        send({"type": "pong", "n": m["n"]})
```

Rules you will hit:

- **Output is JSON, always.** A text answer is `print(json.dumps("some text"))`,
  with the quotes. A bare `print("some text")` is not JSON and fails the call
  (code `junk`). Input is the same: a text payload arrives as a JSON string.
- **Environment.** Your program sees only `PATH` and the variables named in the
  neuron's `env`. No `HOME`, no virtualenv variables, no API keys unless you
  list their names (`"env": ["OPENAI_API_KEY"]`). Use an absolute interpreter
  path, or name what the interpreter needs.
- **Logs go to stderr.** Level 1 stdout carries protocol lines only.
- **Quiet is a choice.** An error (non-zero exit) forwards nothing too, but is
  recorded as a failure.
- **Running a network.** There is no engine command yet: the TypeScript core
  does not read network files, and the Rust engine is not written. Today you
  can run a neuron by hand as above, and check a network file with the helper
  `checkFile` in `test/support/network.ts` (it validates against the schema and
  reports the findings of section 2.7).

## 1. Terms (informative)

- **Neuron**: an entry in a network file, with an `id` and one **kind**
  (`source`, `run`, `serve`, `http`, `human`, `network`). Kind means this and
  nothing else.
- **Program**: what a `run`, `serve`, `http` or `human` neuron wraps.
- **Source**: a neuron with no program. A **producer** outside the network
  **injects** payloads into it.
- **Payload**: any JSON value. A string is a text payload, anything else a JSON
  payload.
- **Signal**: a message. Fields `id`, `trace`, `from`, `to` (neuron ids),
  `payload`, `hops`, and when set `replyTo`, `cause`, `replaces`. The engine sets
  all of them; programs never do. Ids are opaque: compare, never parse.
- **Link**: one-way `from -> to`. A neuron's outgoing links are those it is
  `from`. The **sender** of a signal is its `from`; a signal delivered to a
  neuron is an **input** there.
- **Reply**: a neuron's non-failing response to an input: an **answer** (has a
  payload) or **quiet** (none). A failure is an **error**, not a reply.
- **Forward**: the signal the engine emits along outgoing links after an answer.
- **Chain**: signals connected by `replyTo` and `cause`; `trace` is the id of its
  first signal. **Hops**: 0 on a root signal, one more than the signal an
  answer or forward follows.
- **Replaces key**: an opaque string of at most 256 bytes on a signal.
- **Work**: one input at one neuron until its reply. Work is **waiting** until
  its program starts, then **running**. Waiting is **held** (threshold) or
  **queued** (a level 1 neuron with all its slots running).
- **Call**: one run of a program, for one input or one threshold crossing.
  A neuron **acts** when its threshold is met: that starts a call.
- **Instance**: one use of a network file. Its **instance path** is the ids of
  the composed neurons around a neuron, then its id, joined by `/`.

## 2. The network file (normative)

One UTF-8 JSON object. The schema ([spec/network.schema.json](../spec/network.schema.json))
is closed: an engine MUST reject any field it does not list, and a `version`
it does not implement. Values are literals: used exactly as written, with no
interpolation, conditions or expressions. The schema rejects `${...}` and
`{{...}}` where a literal is expected; it cannot prove a string is not an
expression, so the rule binds, not the pattern. Its `$id` is a URN
(`urn:agent-neuron:schema:network:0`) because the repository is private; it
becomes a URL when the repository is public.

### 2.1 Top level

| Field | Type | Default | Meaning |
|---|---|---|---|
| `$schema` | string | none | For editors; ignored. |
| `version` | `0` | required | Format version. |
| `name` | string, 1 to 64 | none | A name for the network (written to the trace as `demo`). |
| `description` | string | none | Prose; JSON has no comments. |
| `neurons` | array, at least 1 | required | 2.2. |
| `links` | array | `[]` | 2.5. |
| `entry`, `exit` | neuron id | none | Both or neither (schema). Needed when the file is used by `network` (2.6). |

### 2.2 A neuron

`id` (required, `^[A-Za-z][A-Za-z0-9_-]{0,63}$`, unique in the file),
`description` (string; SHOULD be set, it reaches the trace), exactly one kind
field (2.3), and the options its kind allows (2.4).

### 2.3 Kinds

| Field | Value | Program | Contract |
|---|---|---|---|
| `source` | `true` | none; receives nothing | injection, 4.1 |
| `run` | command | the command | level 0, 3.1 |
| `serve` | command | the command, long-lived | level 1, 3.2 |
| `http` | `http://` or `https://` URL | an endpoint | 3.3 |
| `human` | `true` | a person | 3.4 |
| `network` | path | another network file | 2.6 |

A command is a non-empty array of literal strings: argument 0 is the
executable (looked up on `PATH` unless it contains `/`), the rest are passed
verbatim, no shell. Relative paths inside a command are relative to the
program's working directory.

### 2.4 Options

| Option | Type | Default | On | Meaning |
|---|---|---|---|---|
| `threshold` | integer, at least 1 | none | run, serve, http, human | act when this many distinct senders have each sent an input on one trace (5.2) |
| `within` | integer ms | 10000 | with `threshold` | longest a held input waits; applies to holds only, never to queued or running work |
| `maxHops` | integer, at least 0 | none | all but source | refuse inputs with more hops (5.3) |
| `timeout` | integer ms | 30000 | all but source | longest a call may run, counted from its start; for `network`, the whole composed call |
| `env` | array of unique names | `[]` | run, serve | environment variables passed through: names only |
| `cwd` | path | the directory of the file that declares the neuron | run, serve | working directory |

A program's environment is empty except `PATH` and the `env` names (a name the
engine lacks stays unset). `threshold` and `within` are not allowed on `network`.

### 2.5 Links

`{ "from": id, "to": id }` and nothing else. Both ids MUST exist; the same
pair MUST NOT appear twice; a link into a `source` is an error. `a -> b` does
not imply `b -> a`. Strength is not a link field (see appendix B).

### 2.6 Composed neurons and sealing

`"network": "diagnosis.network.json"` (relative to the declaring file) makes
that whole network one neuron. The named file MUST have `entry` and `exit`,
MUST NOT contain a `source`, and `exit` MUST NOT have outgoing links.

**One shared inner network per instance.** The inner network is started when
the engine starts and is shared by every call to that instance: its level 1
processes, their `maxInflight` slots and their state are shared; level 0
programs still run once per call. Two `network` entries are two instances even
if they name one file; instances share nothing.

Per call:

1. The input is delivered to `entry` as a new root signal: new inner trace
   (so threshold groups never mix calls), hops 0, the same payload, `from` the
   composed neuron's id (a name that exists only inside; a reply to it is
   recorded inside and is not an input to anything), and the outer input's
   replaces key.
2. The first reply of `exit`, an answer or quiet, is the composed neuron's reply.
   Remaining inner work for the call is cancelled without reports.
3. If the inner network has nothing running, queued or held for the call and
   `exit` has not replied, the composed neuron is quiet; if an inner neuron
   failed meanwhile, it is an error, code `inner`.
4. Calls are isolated: staleness inside an instance is scoped to one call, so
   one call can never cut another. A call is cut only when its outer work goes
   stale or times out; then all its inner work is cancelled, recursively.
5. `maxHops` and `timeout` apply at the outer boundary. Hops inside start at 0
   and do not add to the outer chain.

**Sealing** (MUST): inner ids, signals, traces and error text never appear in
anything reported about the outer network; from outside, a signal goes in and
a reply comes out, and an inner failure is only the composed neuron's own
error. A separate inspection tool for the inside, keyed by instance path, is an
operator's tool and is not in version 0.

### 2.7 `check` (normative)

An engine MUST offer a check that does not run the network. It MUST report,
with file, line and column where it can, and exit non-zero on any error:
schema violations; duplicate ids; a link naming an unknown id (SHOULD suggest
the nearest id); a duplicated link; a link into a source; a `network` file that
is missing, invalid, lacks `entry`/`exit`, has a source or an `exit` with
outgoing links (errors); a cycle in which no neuron has `maxHops` (error); an
executable that cannot be found (warning).

### 2.8 Field budget (informative)

8 top-level fields; per neuron `id`, `description`, one kind field, at most 6
options; 2 per link. A new field must answer one question: does it say how
neurons connect, or how one decides? Deciders are programs. `AGENTS.md` holds
the rule.

## 3. The neuron contract (normative)

### 3.1 Level 0: `run`

One process per call, in its own process group. The engine writes one JSON value
to stdin and closes it, ignoring write errors (a program need not read stdin).

- **Input:** the bare payload; but if `threshold` is greater than 1,
  `{"payload": P, "inputs": [P1, ...]}` as in section 0.
- **Output:** exit 0 and stdout empty or whitespace: quiet. Exit 0 and stdout
  one JSON value (whitespace around allowed): an answer. A level 0 program has
  no envelope: the answer goes to all outgoing links with the inherited key.
- **Failures:** see 6.1 (non-zero exit, killed by signal, not JSON, over 1 MiB).
- **Cancel:** SIGTERM to the process group; SIGKILL to it 2 seconds later if
  anything remains. Later output is discarded.
- **Stderr** is logs: never interpreted, never traced except as part of an
  error message (the first 500 characters, trimmed).

### 3.2 Level 1: `serve`

One process per neuron (per instance), started when the engine starts. If it
does not reach `ready` within `timeout`, the engine refuses to start.

**Framing.** UTF-8, one JSON object per line ended by `\n` (`\r\n` tolerated
on read), at most 1 MiB, blank lines ignored, every object has a string
`type`. Stdout carries protocol lines only; stderr is logs. Flush every line.

**Messages.** Engine to neuron:

| `type` | Fields | Meaning |
|---|---|---|
| `hello` | `v` (1), `neuron` (the neuron's id in its file) | first line |
| `signal` | `re`, `payload`, `inputs` only if threshold > 1 | an input; value shapes as 3.1 |
| `cancel` | `re` | advisory, no reply expected |
| `ping` | `n` | only when nothing is running |
| (stdin closed) | | shut down |

Neuron to engine:

| `type` | Fields | Meaning |
|---|---|---|
| `ready` | `v` (1), `maxInflight` optional integer at least 1, default 1 | MUST be the first line |
| `answer` | `re`, `payload`; optional `replaces` (string, or `null` to clear), optional `to` (array of ids) | an answer |
| `quiet` | `re` | quiet |
| `error` | `re`, `message`; without `re`, the process itself failed | failure |
| `pong` | `n` | reply to `ping` |

Unknown fields are ignored; a neuron's message of an unknown `type` is ignored.

**Versioning.** `v` is the protocol's major version; `ready.v` MUST equal
`hello.v`. A change an old peer would misread bumps it; an optional field or a
new message type does not.

**`re`** is a counter the engine keeps per process (1, 2, ...), not the signal
id: ids never reach programs. At most `maxInflight` calls are running at once;
the rest are queued in order. Replies may come in any order. A reply for an
`re` that is cancelled, finished or unknown is discarded.

**Cancel** is sent for stale work and timeouts. A cancelled `re` keeps its slot
until the neuron replies for it or `timeout` ms have passed, then the process
is treated as hung (6.1).

**`timeout`** is per call, from the moment the `signal` line is written. Ping
is sent only when nothing is running; no `pong` within `timeout` is a hang.

**Reply `to`.** A set of neuron ids, each an outgoing link of the neuron; the
forward lists them in link declaration order whatever the order given.
`to: []` forwards nothing but the answer is still reported. A reply's `replaces`
longer than 256 bytes, or a `to` that names a non-linked neuron, or an
`answer` without `payload`, is an error (6.1, code `protocol`).

**Shutdown.** The engine closes stdin; after 2 s it sends SIGTERM to the
process group, after 2 s more SIGKILL. Running work is cancelled silently.

**Restart.** If the process exits, closes stdout, sends `error` without `re`, or
hangs, every running call fails (code `crash`) and what remains of the process
group is killed. Before each restart the engine waits `min(100 * 2^f, 5000)` ms,
where `f` is the number of consecutive starts since the last `ready` that did
not reach `ready`. When `f` reaches 5 the neuron is **failed**: every input to it
is an error (code `start`) until the engine restarts.

**Junk.** A stdout line that is not a JSON object with a string `type` is junk:
the engine reports a note (7.2) with the line number (from 1, per process) and
the first 2000 characters. If exactly one call is running at that neuron it
fails (code `junk`); otherwise only the note is made. Junk never fails the neuron.

**A complete transcript** (`>` engine to neuron, `<` neuron to engine; a
neuron with `maxInflight` 2 and threshold 1):

```
> {"v":1,"type":"hello","neuron":"judge"}
< {"v":1,"type":"ready","maxInflight":2}
> {"type":"signal","re":1,"payload":{"n":1}}
> {"type":"signal","re":2,"payload":"plain text"}
< {"type":"answer","re":2,"payload":{"n":5},"replaces":null,"to":["next"]}
> {"type":"cancel","re":1}                       (re 1 went stale)
< {"type":"answer","re":1,"payload":{"n":9}}     (discarded: re 1 is cancelled)
> {"type":"ping","n":1}
< {"type":"pong","n":1}
> {"type":"signal","re":3,"payload":{"n":2}}
< {"type":"error","re":3,"message":"model unavailable"}
(engine closes stdin)
```

### 3.3 `http`

`POST` the stdin value (3.1) as `application/json`. A 2xx response with a JSON
body is an answer; a 204 or empty 2xx body is quiet; any other status, a
connection failure or a non-JSON body is an error (code `http`). Redirects are
not followed. Cancel aborts the request. There is no authentication in
version 0.

### 3.4 `human` (thin)

The engine shows the input to a person and takes a typed reply: empty is quiet,
anything else is a text payload. Cancel and `timeout` stop the wait. The
interface is the engine's business.

## 4. Routing (normative)

### 4.1 Injection

The engine MUST let a producer inject a payload into a source, with an optional
replaces key (at most 256 bytes; longer is refused to the producer). It creates
a root signal: new id, `trace` its own id, `hops` 0, `from` the source, `to`
the source's outgoing links in declaration order, `replaces` as given. Only the
producer knows what makes one input stale, so the key is its job; nothing else
reads a payload to compute one.

### 4.2 Delivery

A signal is delivered to each neuron in `to`, in order; each delivery is
independent work.

### 4.3 Reply

When work for input *s* at neuron *N* ends in a reply, the engine emits a reply
signal: `from` *N*, `to` `[s.from]`, `replyTo` `s.id`, `trace` `s.trace`, `hops`
`s.hops + 1`; reported as an `answer` or `quiet` event. It goes to the sender: it
is reported (and available to the producer if the sender is a source) and is
never an input to the sender's program. For a threshold call, *s* is the input
that crossed.

### 4.4 Forward

Right after an answer that is not stale, the engine emits one forward: `from`
*N*, `payload` the answer's, `cause` the answer's id, `trace` the answer's,
`hops` answer hops + 1, `to` *N*'s outgoing links in declaration order (or the
subset the reply names), `replaces` the reply's value (string sets, `null`
clears) or else *s*'s key, inherited. Quiet, an error, a refused input, stale
work and a neuron with no outgoing links forward nothing. Forwarding along
links is the only way a neuron reaches another neuron.

Each neuron on a path adds **two hops** (reply, then forward). A neuron at depth
`d` (the first neuron after the source is depth 1) receives hops `2(d-1)`; `maxHops` is a limit on that
count, not on neurons deep: a limit of 4 lets a chain reach its third neuron.

A run's result is not a return value: read the answers of neurons with no
outgoing links from the trace.

## 5. Threshold, hops and staleness (normative)

### 5.1 Order at a delivery

For input *s* at *N*: the hops check (5.3); then, if `threshold` is more than 1,
hold *s* (5.2); then, when the program may start, run it (queued first at
level 1 if all slots are running). Threshold 1 or none: no hold.

### 5.2 Threshold

Inputs are grouped by (neuron, trace); traces never combine. A sender counts
once: its newer input on the trace replaces its older held one (a drop,
reason `replaced`, phase `waiting`). When *k* distinct senders have an input
held, the neuron acts: the inputs leave the group and one call starts with all
of them in arrival order. The call answers the input that crossed (4.3); the
others get no reply and no drop, even if the call is cut or fails. A later
input on that trace starts a new group. Each held input has its own `within`
timer, started when held; at the end it is dropped as `expired` (phase
`waiting` MAY be written). Consumed inputs never expire. Held inputs do not
count as running.

### 5.3 maxHops

An input with `hops` above the neuron's `maxHops` is reported as a signal, never
run, and dropped with reason `hops` (no phase).

### 5.4 Staleness

Work is keyed by (sender, key) when its input has a key. Emitting a signal
with key *K* from sender *S* makes every older signal of the same (*S*, *K*)
**stale** at that moment, and its unfinished work:

- waiting (held or queued): dropped, reason `replaced`, phase `waiting`;
- running: cancelled, reason `replaced`, phase `running`, reported when it goes
  stale (the kill follows, 3.1, 3.2);
- an answer arriving for stale work is discarded and nothing is forwarded.
  A cancelled call is never an error.

Finished work is unaffected, and so is what it forwarded: staleness does not
travel down a chain. Each hop replaces on its own sender's key.

**Any replaced input cuts a running threshold call.** The call is stale when any
input it was built from is stale, not only the one that crossed. One drop is
reported, for the crossing input (which may itself not have been replaced),
phase `running`. The other inputs report nothing.

A 5.2 same-sender regroup replaces a *held* input only. It never cuts a running
call by itself; only (sender, key) staleness does.

## 6. Failures and event order (normative)

### 6.1 Failures

A failure fails one call: an `error` event for the signal the call answers (for
a threshold call, the crossing input), nothing forwarded, nothing else
affected. Failures of stale work are not errors.

| Condition | `code` | `message` starts | Effect beyond the error |
|---|---|---|---|
| level 0 exit not 0, or killed by a signal | `exit` | `exit N:` or `signal S:`, then stderr | |
| output over 1 MiB (level 0), line over 1 MiB (level 1) | `output` | `output too large` | process group killed |
| level 0 stdout not empty and not one JSON value | `junk` | `not JSON` | |
| level 1 junk line while exactly one call runs | `junk` | `junk on stdout` | note |
| http: non-2xx, connection failure, non-JSON body | `http` | `http` | |
| level 1 reply with bad shape, bad `to`, bad `replaces`, key over 256 bytes, no `payload`, or `ready.v` differing from `hello.v` | `protocol` | `protocol:` and the cause | reply unused |
| call longer than `timeout` | `timeout` | `timed out after N ms` | cancelled; note |
| level 1 process exit, hang, or `error` without `re` | `crash` | `process failed` | every running call fails; restart (3.2) |
| input to a failed neuron | `start` | `neuron failed to start` | |
| composed neuron with no reply after an inner failure | `inner` | `no answer: an inner neuron failed` | |
| level 1 `error` with `re` | `neuron` | the neuron's message | |

### 6.2 Event order

An engine MUST report events in this order, so that two engines produce the same
list.

1. Work due at the same instant is processed in the order it was scheduled.
2. A signal's event is reported first, then, at the same `t`, **immediately
   after it**: (a) the drops of the work that signal made stale, in the order
   that work was created; (b) for each target in `to` order, its delivery
   effects (a `hops` drop, or a held replacement drop).
3. A reply is reported before its forward.
4. A running drop is reported when the work goes stale; a `note` for the
   cancel follows it, and the later `term` and `kill` notes at their own times.
5. A call that ends exactly when its input goes stale counts as stale.
6. A **run is over** when nothing is running or queued and no timer is pending.
   Held inputs keep their `within` timers, so an engine that waits for the end
   reports their `expired` drops. A fixture runs the virtual clock until nothing
   is due. A real engine's "wait until idle" MAY stop earlier, ignoring held
   inputs (the TypeScript `idle` does); then their drops are not reported.

## 7. The trace (normative)

An engine writes [trace format v1](TRACE.md), so the validity checks, the replay
module and the network view work unchanged.

### 7.1 Header

`format` and `version` as v1. `demo` is the network's `name`, or the file name
(TRACE.md defines `demo` as a free name). `library` stays the engine's version
string (`0.1.0`); an optional `engine` field names the engine. `description`
from the file. `neurons`: `id`, `description`, `kind` (the kind field's name,
`network` written `composed`) and `options` as set in the file
(`threshold`, `within`, `maxHops`; defaults are not written). `links`: one
`{from, to}` per link, directional. `summary` MAY be `{}`. A composed neuron's
inside never appears.

### 7.2 Events

| Event | Written for |
|---|---|
| `signal` | an injection or a forward; `via` absent |
| `answer`, `quiet` | a reply (4.3) |
| `drop` | stale work, expiry, a hops refusal |
| `error` | a failure (6.1) |
| `note` | process facts |

`t` is whole ms since the run started (virtual in fixtures). Payloads as v1:
text `{"type":"text","text":...}` for a string, else `{"type":"json","data":...}`
(a JSON string payload is therefore text: the one lossy spot). Every event but
`note` carries the signal's fields (`id`, `from`, `to`, `trace`, `replyTo` and
`cause` when set, `hops`). A `drop` or `error` describes the signal the work was
for, plus `neuron` and `reason` (drop) or `message` (error). `note` is
`{type, t, neuron, what}`.

Additions beyond v1 as written, all optional fields or a new event type (so
v1 readers, which ignore what they do not know, are unaffected):

| Addition | Rule |
|---|---|
| `phase` (`waiting` or `running`) on `drop` | MUST on reason `replaced`; MUST NOT on `hops`; MAY on `expired` |
| `replaces` on `signal` | MUST when the signal has a key |
| `code` on `error` | MUST, from the 6.1 table |
| `note` event: `what` is `junk` (with `line`, `text`), `cancel`, `term`, `kill`, `restart` or `inject` (with `trigger`) | SHOULD for those facts |
| `engine` header field | MAY |

Example, a running call cut because an *earlier* input was replaced. Detectors
`d1` and `d2` (20 ms) feed `h` (`threshold` 2, 100 ms); `s` feeds both. Ids are
normalised (8.3); the second injection at 40 ms makes `d1`'s first forward stale
when `d1`'s new forward is emitted at 60:

```
{"type":"signal","t":0,"id":"#1","from":"s","to":["d1","d2"],"trace":"#1","hops":0,"replaces":"k","payload":{"type":"json","data":{"n":1}}}
{"type":"answer","t":20,"id":"#2","from":"d1","to":["s"],"trace":"#1","replyTo":"#1","hops":1,"payload":{"type":"json","data":{"d":1}}}
{"type":"signal","t":20,"id":"#3","from":"d1","to":["h"],"trace":"#1","cause":"#2","hops":2,"replaces":"k","payload":{"type":"json","data":{"d":1}}}
{"type":"answer","t":20,"id":"#4","from":"d2","to":["s"],"trace":"#1","replyTo":"#1","hops":1,"payload":{"type":"json","data":{"d":2}}}
{"type":"signal","t":20,"id":"#5","from":"d2","to":["h"],"trace":"#1","cause":"#4","hops":2,"replaces":"k","payload":{"type":"json","data":{"d":2}}}
{"type":"signal","t":40,"id":"#6","from":"s","to":["d1","d2"],"trace":"#6","hops":0,"replaces":"k","payload":{"type":"json","data":{"n":2}}}
{"type":"answer","t":60,"id":"#7","from":"d1","to":["s"],"trace":"#6","replyTo":"#6","hops":1,"payload":{"type":"json","data":{"d":1}}}
{"type":"signal","t":60,"id":"#8","from":"d1","to":["h"],"trace":"#6","cause":"#7","hops":2,"replaces":"k","payload":{"type":"json","data":{"d":1}}}
{"type":"drop","t":60,"id":"#5","from":"d2","to":["h"],"trace":"#1","cause":"#4","hops":2,"replaces":"k","neuron":"h","reason":"replaced","phase":"running","payload":{"type":"json","data":{"d":2}}}
```

(The example stops at the drop; `d2` then replies at 60 too.) The drop names `#5`, the crossing input from `d2`, which was not replaced;
`#3` was. `h` acted at 20; its call would have ended at 120. An `error` event has
the same fields in place of `reason`/`phase`: `"neuron":"h","message":"timed out
after 100 ms","code":"timeout"`.

## 8. Conformance (normative)

An engine conforms when it passes every fixture in `spec/fixtures/` (to be
written with the first engine; once, for all engines). Recorded traces in
`traces/` stay as smoke tests through replay, not conformance tests.

### 8.1 A fixture

A directory with a network file (and the composed files it names) and
`fixture.json`:

```json
{ "fixture": 0, "description": "One line.", "network": "network.json", "notes": false,
  "inputs": [{ "at": 0, "source": "alerts", "payload": {"service": "gateway"}, "replaces": "gateway" }],
  "script": { "logs": [{ "when": {"service": "gateway"}, "after": 20, "reply": {"quiet": true} }] },
  "expect": [] }
```

- `inputs`: injections at virtual ms `at`.
- `script` replaces every program, keyed by instance path (the harness may name
  inside a sealed neuron). Each entry answers one call: the first unused entry
  whose `when` deep-equals the program's input value (3.1) is used (no `when`
  matches anything); no match is a harness failure. `after` is the latency in ms
  from the call's start. `reply` is `{"answer": P, "replaces"?: K or null, "to"?: [ids]}`,
  `{"quiet": true}`, `{"error": "message"}`, `{"junk": "text"}`, `{"crash": true}`
  or `{"hang": true}`. The script stands in for the process layer, so fixtures
  test what the engine does with an outcome, not byte parsing.
- `expect`: the events (section 7) in order. `note` events are compared only if
  `notes` is true.

### 8.2 Clock

Virtual time starts at 0 and jumps to the next due moment (an `at`, an `after`, a
`within`, a `timeout`); processing takes none. The engine MUST be deterministic
(6.2); the run is over as 6.2 rule 6 says.

### 8.3 Normalisation

Ids become `#1`, `#2`, ... by first appearance across the event list, scanning
each event's `id`, `trace`, `replyTo`, `cause` in that order. Events are then
compared one by one, every field, `t` included, `via` absent equal to `"emit"`.

### 8.4 Worked fixture

`s` (source) -> `a` -> `b`. Input at 0: `{"n":1}`, key `k`. `a` answers `{"n":2}`
after 10; `b` is quiet after 5. Expected:

```
{"type":"signal","t":0,"id":"#1","from":"s","to":["a"],"trace":"#1","hops":0,"replaces":"k","payload":{"type":"json","data":{"n":1}}}
{"type":"answer","t":10,"id":"#2","from":"a","to":["s"],"trace":"#1","replyTo":"#1","hops":1,"payload":{"type":"json","data":{"n":2}}}
{"type":"signal","t":10,"id":"#3","from":"a","to":["b"],"trace":"#1","cause":"#2","hops":2,"replaces":"k","payload":{"type":"json","data":{"n":2}}}
{"type":"quiet","t":15,"id":"#4","from":"b","to":["a"],"trace":"#1","replyTo":"#3","hops":3}
```

To write first: the behaviours `test/` pins today, plus replace/cut, held
replacement, threshold (same sender once, traces apart, `within`, consumed
inputs never expire), an earlier input cutting a running call, `maxHops` and a
loop, the alert storm at thresholds 1, 2 and 3, `to` limited to links, quiet
forwarding nothing, a sealed composed neuron, timeout, crash and junk.

### 8.5 The observer rule

A fixture MAY attach a scripted observer (one that logs, or injects into a
source at a given time). Removing it MUST NOT change the events, except those
that follow its injections (section 9).

## 9. Observers (normative)

An **observer** is a program outside the network that reads the engine's trace
events as a live stream. It is not in the network file (no field, kind or link
mode), is not addressable, is not on any path, and cannot send along links.

- Events reach it **buffered and asynchronously**. A slow or crashed observer
  never delays or alters the network: it **fails open**.
- It MAY: (a) return results to whoever runs it (for example the answers of
  neurons with no outgoing links); (b) write the strength state file (it is the
  caller that adjusts strength, using `cause` and `replyTo` chains for credit);
  (c) inject work, only into a `source` the network file declares, so the file
  still shows every way work enters. An injection MUST be recorded as a `note`
  (`what: "inject"`, `trigger`), SHOULD be rate-limited, and an observer SHOULD
  ignore traces rooted in its own injections.
- It MUST NOT stop, throttle, route or delay any trace, set `to` or `replaces`
  on other work, or see inside a composed neuron (an outer observer sees outer
  events only). It may alert, or ask the operator's normal stop as a person
  would, and that request is recorded.
- **Limits live in the engine**: budgets and loop ceilings are declared in the
  network file, checked before a call starts, and fail closed. In version 0
  the only such limits are `maxHops` and `timeout`.
- **Test:** remove the observer and rerun; if answers change (not strengths,
  logs or alerts) it was orchestrating, and the engine is wrong or the observer
  is a neuron. Conformance (8.5) applies this.
- Not observer jobs: merging (a convention between programs on the path), the
  root replaces key (the producer), memory across level 0 calls (use `serve`),
  not diagnosing a known cause again (an ordinary neuron on the path, quiet for
  a known cause).
- The TypeScript library's listeners (`onSignal`, `onDrop`, `onError`) are the
  in-process observer. Routing done inside them, as the alert storm demo does,
  belongs to the routing rule (section 4); an engine does it itself.
- There is no `watch` link: it would settle the open passive/active question by
  the back door.

## Appendix A. Walk-through: one alert (informative)

Files: [alerts.network.json](../spec/examples/alerts.network.json),
[diagnosis.network.json](../spec/examples/diagnosis.network.json); programs in
`spec/examples/neurons/` and `examples/alerts/`. The producer injects, at their
`at` times, the 40 alerts of `examples/alerts/alerts.json` (a timed schedule that
is the producer's, not the file's), each with its service as key. Alert 5:
`{"id":5,"service":"checkout","severity":"warn",...}`.

1. Signal #1 goes from `alerts` to `logs`, `deploys`, `metrics` (hops 0).
2. `metrics` prints nothing: quiet, forwarded nowhere.
3. `logs` and `deploys` each flag; each answer goes back to `alerts` (hops 1) and
   is forwarded to `hypothesis` (hops 2), `cause` the answer, key `checkout`.
4. `hypothesis` (`threshold` 2): the first flag is held; the second flag's sender
   crosses. The program gets `{"payload": ..., "inputs": [flag, flag]}`, waits
   500 ms and answers `{"alert":5,"service":"checkout","detectors":["deploys","logs"]}`
   to the crossing input's sender, forwarded to `diagnosis`.
5. `diagnosis` is composed: `topology` adds `root`, the link takes it to
   `check` (the `exit`), which adds `deploy` and `minutes`. Outside, one signal
   in, one answer out.
6. The verdict is `diagnosis`'s answer event. It has no outgoing links, so the run
   ends. A newer `checkout` alert during step 4 cuts the call as in the section 7 example.

How a run ends: the producer has injected everything; the engine waits until
nothing runs, and the demo then also waits `within` (500 ms) so lone flags
expire; an engine's idle wait may do the same (6.2). The file hard-codes
`threshold` 2; the demo's variants 1 and 3 are two more files, not a parameter.
Where each piece of the demo's caller code went, and what could not be
expressed, is in [decisions/0002](decisions/0002-alert-storm-mapping.md).

## Appendix B. Out of version 0 (informative)

- Strength: a state file keyed by instance path, adjusted by the caller. Until
  then every sender counts 1.
- Passive and active channels (open; no field reserved); link attributes.
- `http` authentication and headers; `human` interface; Wasm neurons;
  Windows; concurrency limits at level 0; a run-level result; inspecting inside
  a composed neuron; byte-level fixtures and real-process fixtures; hot reload.
- Distribution: the engine will ship as a binary inside the npm and pip
  packages (the esbuild and ruff pattern).
- **Plan for 0.1**, each with a conformance fixture: a per-trace `finished` event
  (with the count still held); per-trace budget limits declared in the network
  file; a normative live trace stream for observers.

## Appendix C. How the TypeScript library differs (informative)

The library in `src/` is the first engine to be brought into line. Deliberate
differences:

- one-way links (`connect` links both ways today);
- automatic forwarding along links (callers forward today); `emit` `to` limited
  to outgoing links;
- a running threshold call is cut by any replaced input (#17), not only the
  crossing one;
- a composed neuron is one shared inner network per instance (the demo builds a
  fresh one per call);
- a default `timeout` of 30 s (none today);
- a minimal environment (the shell adapter inherits the engine's);
- process-group kill with SIGKILL after 2 s (the adapter kills the child only);
- level 0 is JSON both ways (the shell adapter writes text raw and reads
  non-JSON as text);
- `phase` on drops (`onDrop` reports one reason today);
- the threshold stdin shape applies above 1 only.
