# agent-neuron specification, version 0

Status: draft. Version 0 may change in any way until it is called 1; every
change to it is a commit to this file, to `spec/network.schema.json` and to
the conformance fixtures together.

The words MUST, MUST NOT, SHOULD and MAY are used as in RFC 2119. Every
section is marked **normative** (the rules an engine or a neuron has to
follow) or **informative** (explanation and examples; no rules). Where an
informative text and a normative one disagree, the normative one wins.

Contents

1. Purpose and scope
2. Terms
3. The network file
4. Routing
5. Threshold, within, maxHops and stale work
6. Level 0: the one-shot contract
7. Level 1: the line protocol
8. Errors, timeouts, crashes, junk and stderr
9. The trace an engine writes
10. Conformance
11. Out of version 0
12. Walk-through: one alert
13. Appendix: a Python neuron on each level; the TypeScript library

Decisions behind this document, with their reasons: [decisions/0001-spec-v0.md](decisions/0001-spec-v0.md).

## 1. Purpose and scope (informative)

An **engine** runs a network of **neurons**: programs, in any language, that
the engine starts, feeds signals and listens to. Three things are specified,
and nothing else is:

1. **The network file**: JSON that says which neurons exist, what each one
   wraps and how they connect. Schema: [spec/network.schema.json](../spec/network.schema.json).
2. **The neuron contract**: how an engine talks to the program inside a
   neuron. Level 0 (one process per signal, stdin and stdout) and level 1
   (one long-lived process, JSON lines).
3. **The routing rule**: where a neuron's answer goes next, and how stale
   work is cut, with no code outside the network.

The protocol, not a library, is the product. Any engine that follows this
file can run any network file and any neuron. The TypeScript core in `src/`
is the first engine; a Rust engine is expected to compete on the same
conformance fixtures (section 10).

The rule that keeps the file honest: **JSON holds literals and references
only. If a key has to read a payload to mean anything, it does not belong in
the file.** Gates, filters, transforms and prompt templates are programs, and
a filter is a small neuron.

## 2. Terms (normative)

- **Neuron**: an entry in a network file with an `id` and exactly one **kind**.
- **Kind**: `source`, `run`, `serve`, `http`, `human` or `network`.
- **Program**: what a `run`, `serve`, `http` or `human` neuron wraps.
- **Source**: a neuron with no program. A producer outside the network
  **injects** payloads into it.
- **Payload**: any JSON value. A string is a text payload; any other value is a
  JSON payload. This is the whole difference.
- **Signal**: one message in the network. Fields: `id`, `trace`, `from`, `to`
  (list of neuron ids), `payload`, `hops`, and when set `replyTo`, `cause`,
  `replaces`. The engine sets all of them; programs never do. Ids are opaque
  strings: compare them, never parse them.
- **Link**: a one-way connection `from -> to`. A neuron's **outgoing links**
  are those it is `from`. Two-way is two links.
- **Sender** of a signal: its `from`.
- **Input**: a signal delivered to a neuron.
- **Answer**: what a neuron says to an input. **Quiet** is an answer that
  says nothing.
- **Forward**: the signal the engine emits along a neuron's outgoing links
  because it answered.
- **Chain**: the signals connected by `replyTo` and `cause`. `trace` is the
  id of the chain's first signal.
- **Hops**: 0 for a root signal; an answer or a forward is its origin's hops
  plus one.
- **Replaces key**: an opaque string, at most 256 bytes, on a signal.
- **Work**: one input at one neuron, from delivery to answer. Work is **held**
  until its program starts (waiting for a threshold or a free slot) and
  **running** from then on.
- **Call**: one run of a program for one input, or for one threshold crossing.
- **Instance path**: the neuron's id, prefixed by the ids of the composed
  neurons it is inside, joined with `/` (`diagnosis/topology`). Used for
  scripting fixtures and, later, for state; never visible outside the
  composed neuron it is inside.

## 3. The network file (normative)

A network file is one JSON object, UTF-8. The schema is closed: an engine
MUST reject any field the schema does not list, at any level, and MUST reject
a `version` it does not implement (naming the versions it does). Values are
literals: a string is used exactly as written, with no interpolation, no
conditions and no expressions. The schema rejects the two common
interpolation syntaxes (`${...}` and `{{...}}`) in the places a literal is
expected; it cannot prove that a string is not an expression, so the rule
above, not the pattern, is what binds.

### 3.1 Top level

| Field | Type | Default | Meaning |
|---|---|---|---|
| `$schema` | string | none | Path or URL of the schema, for editors. Ignored by engines. |
| `version` | `0` | required | The file format version. |
| `name` | string, 1 to 64 chars | none | A name for the network; written to the trace as `demo`. |
| `description` | string | none | Prose for humans. JSON has no comments; this is where they go. |
| `neurons` | array of neurons, at least 1 | required | The neurons (3.2). |
| `links` | array of links | `[]` | The connections (3.5). |
| `entry` | neuron id | none | With `exit`: where a composed neuron's input goes in (3.6). |
| `exit` | neuron id | none | With `entry`: whose answer is the composed neuron's answer. |

### 3.2 A neuron

Every neuron has:

| Field | Type | Default | Meaning |
|---|---|---|---|
| `id` | string, `^[A-Za-z][A-Za-z0-9_-]{0,63}$` | required | Unique in the file. |
| `description` | string | none | What it is, in a sentence. SHOULD be set; it reaches the trace. |

and exactly one kind field (3.3), and the options (3.4) its kind allows.

### 3.3 Kinds

| Kind field | Value | Program | Contract |
|---|---|---|---|
| `source` | `true` | none | Receives nothing; payloads are injected (4.1). |
| `run` | command: non-empty array of literal strings | the command | Level 0 (section 6). |
| `serve` | command | the command, long-lived | Level 1 (section 7). |
| `http` | URL literal, `http://` or `https://` | an HTTP endpoint | Level 0 over HTTP (6.4). |
| `human` | `true` | a person | Level 0 through a person (6.5). Thin. |
| `network` | path literal | another network file | A composed neuron (3.6). |

A command is an argument vector: element 0 is the executable, found on
`PATH` unless it contains a `/`; the rest are passed verbatim. No shell is
involved. A relative path inside a command is relative to the working
directory of the process (see `cwd`), not to the file.

### 3.4 Options

| Option | Type | Default | Allowed on | Meaning |
|---|---|---|---|---|
| `threshold` | integer, at least 1 | none | run, serve, http, human | Act only when this many **distinct senders** have each sent an input on one trace (5.2). |
| `within` | integer ms, 1 to 3600000 | 10000 | with `threshold` only | How long a held input waits for the others. |
| `maxHops` | integer, at least 0 | none (no limit) | all but source | Refuse a signal with more hops than this (5.4). |
| `timeout` | integer ms, 1 to 3600000 | 30000 | all but source | Longest a call may take, from when the program starts (8.2). For `network`, the whole composed call. |
| `env` | array of unique variable names | `[]` | run, serve | Names of environment variables passed through from the engine. Names only: no values, no interpolation. |
| `cwd` | path literal | the directory of the network file | run, serve | Working directory of the program; relative paths are relative to the directory of the network file. |

A program's environment is empty except for `PATH` and the names in `env`
(a name the engine does not have is left unset).

`threshold` and `within` are not allowed on a `network` neuron: apply them to
the inner neuron that should hold inputs.

### 3.5 Links

A link is `{ "from": id, "to": id }` and nothing else in version 0. Both ids
MUST name neurons in the file. A link into a `source` is an error (a source
receives nothing). The same `from -> to` MUST NOT be listed twice. `from -> to`
does not imply `to -> from`: the other direction is its own entry.

Strength is not a link field. It is designed but not built; when it exists
it lives in a separate state file keyed by instance path, and the caller
adjusts it, so that the reviewed network file never changes by learning
(section 11).

### 3.6 Composed neurons and sealing

`"network": "diagnosis.network.json"` makes the whole network in that file
one neuron. The path is relative to the directory of the file that names it.
The named file MUST have `entry` and `exit`, MUST NOT contain a `source`, and
`exit` MUST NOT have outgoing links.

Behaviour of a composed neuron, call by call:

1. An input to the composed neuron is delivered to the inner `entry` neuron
   as if it were a root signal (hops 0, a new inner chain), with the same
   payload. The inner network then runs by the same rules as the outer.
2. The first answer of the inner `exit` neuron is the composed neuron's
   answer. Remaining inner work for that call is cancelled silently.
3. If the inner network goes idle (nothing held, nothing running) without
   `exit` answering, the composed neuron is quiet, unless an inner neuron
   failed, in which case it is an error: `no answer: an inner neuron failed`.
4. If the composed neuron's work is replaced or times out, all inner work for
   the call is cancelled, recursively.
5. The composed neuron's `maxHops` and `timeout` apply at the outer boundary.

**Sealing** (MUST): inner neuron ids, inner signals, inner traces and inner
error messages never appear in anything the engine reports about the outer
network. From outside, a signal goes in and an answer comes out. Inner
failures reach the outer trace only as the composed neuron's own error. An
engine MAY offer a separate way to inspect the inside, which is looking in,
not the outer network reaching in; it is not part of version 0.

**Each use is its own neuron** (MUST): every `network` entry is a separate
instance, even when two entries name the same file; instances share no
processes and no state.

### 3.7 What an engine's `check` reports (normative)

An engine MUST provide a way to check a network file without running it
(`check`). It MUST report, each with the file, line and column where it can:

| Finding | Severity |
|---|---|
| a field the schema does not allow, or a value of the wrong type | error |
| a duplicate neuron id | error |
| a link naming an unknown neuron (SHOULD suggest the closest id: "did you mean") | error |
| a link listed twice; a link into a source | error |
| a `network` file that is missing, invalid, or lacks `entry`/`exit`, contains a source, or whose `exit` has outgoing links | error |
| a cycle in which no neuron has `maxHops` | error |
| an executable that cannot be found for a `run` or `serve` neuron | warning |

`check` exits non-zero if there is any error.

### 3.8 The field budget (informative)

The file stays small on purpose. Today: top level 8 fields; a neuron has `id`,
`description`, one kind field and at most 6 options; a link has 2 fields. A
proposed new field is tested with one question: **does it say how neurons
connect, or how one decides?** If it says how a neuron decides, it is a
program, not a field. A new field also has to fit the budget: add one only
with a decision record that says what it replaces or why the budget grows.
`AGENTS.md` carries the same rule for contributors.

## 4. Routing (normative)

Routing needs no code outside the network. Everything an engine needs to
send an answer on is in the network file (the links) and in two envelope
fields (`replaces` and `to`).

### 4.1 Injection

A producer outside the network injects a payload into a source, optionally
with a replaces key. The engine MUST offer this. It creates a root signal:
new id, `trace` equal to its id, `hops` 0, `from` the source, `to` the
source's outgoing links in declaration order (an empty list if there are
none), and `replaces` as given.

The key is supplied by the producer: only the producer knows what it means
for one input to make an older one stale (a newer alert for the same
service). No other place in the system reads a payload to compute a key.

### 4.2 Delivery

A signal is delivered to each neuron in `to`, in order. Each delivery is
independent work (section 5) and its outcome is independent of the others.

### 4.3 Answer

When the work for input *s* at neuron *N* finishes with an answer, the
engine emits an **answer** signal: `from` *N*, `to` `[s.from]`, `replyTo`
`s.id`, `trace` `s.trace`, `hops` `s.hops + 1`. The answer goes to the
sender: it is recorded, and for a source it is available to the producer.
It is **not** an input to the sender's program (programs do not receive
answers). A **quiet** answer is recorded as quiet and ends there.

For a threshold call, *s* is the input that crossed the threshold.

### 4.4 Forward

Right after an answer that is not quiet and not stale, the engine emits one
**forward** signal: `from` *N*, `payload` the answer's payload, `cause` the
answer's id, `trace` the answer's trace, `hops` `answer.hops + 1`, `to` *N*'s
outgoing links in declaration order, or the subset named by the reply's `to`.

- `to` in a reply envelope (level 1 only) MUST name only neurons *N* has
  outgoing links to; otherwise the reply is an error (8.1). `to: []`
  forwards nothing but the answer is still recorded and still goes to the
  sender.
- `replaces` of the forward: the reply's `replaces` if it has one (a string
  sets the key, `null` clears it); otherwise the key of *s*, inherited.
- A neuron with no outgoing links forwards nothing. A quiet answer, an error,
  a refused signal and stale work forward nothing.

The answer is reported before its forward. Forwarding is automatic and always
along links; there is no other way for a neuron to send to another neuron.

### 4.5 The consequences (informative)

- A cheap detector that exits without output is quiet, so nothing downstream
  runs. "Forward only non-empty answers" is a property of the program.
- A key set once at injection follows the chain without any neuron touching
  it.
- A run's result is not a return value. It is the answer events of the
  neurons that have no outgoing links, read from the trace.

## 5. Threshold, within, maxHops and stale work (normative)

### 5.1 Order of checks at a delivery

For input *s* at neuron *N*: (1) the hops check (5.4); (2) if *N* has a
`threshold`, hold *s* (5.2); (3) when the program may start, run it.

### 5.2 Threshold

A neuron with `threshold` *k* groups inputs by **trace**. Inputs on different
traces never combine. In one group, a sender counts once: a newer input from
the same sender on the same trace replaces its older held input (the older is
dropped as replaced, held). When *k* distinct senders have an input held, the
neuron **acts**: all those inputs leave the group and one call starts, with
those inputs in arrival order. The call answers the input that crossed the
threshold (4.3); the other inputs get no answer and no drop, even if the call
is cancelled or fails. A later input on a trace that has already acted starts
a new group.

With `threshold` 1 a neuron acts on every input, but still takes the threshold
form of the program input (6.1).

A neuron without `threshold` holds nothing and acts on every input at once.

Held inputs are waiting, not working: an engine's notion of idle (nothing
running) does not wait for them, and they leave on their own through `within`.

### 5.3 Within

Each held input has its own timer, started when it is held, of `within`
milliseconds (default 10000). When it runs out the input is dropped as
`expired`. A newer input from the same sender restarts the wait for that
sender. Inputs consumed by an acting call never report `expired`.

### 5.4 maxHops

A neuron with `maxHops` *m* refuses an input whose `hops` exceed *m*: the
signal is still reported, the program never runs, and the engine reports a
drop with reason `hops`. Hops are counted along the chain (4.3, 4.4), so a
loop is cut where it first exceeds the limit. A file with a cycle needs
`maxHops` on a neuron in it (3.7).

### 5.5 Stale work and `replaces`

Work is keyed by the pair (sender, key) when the input has a replaces key.
When a signal with key *K* is emitted by sender *S* (injected, or a
forward), the engine MUST immediately make every older signal with the same
(*S*, *K*) **stale**, and:

- work for a stale signal that is **held** (threshold hold, or waiting for a
  slot) is dropped: reason `replaced`, phase `held`;
- work that is **running** is cancelled (6.3, 7.5): reason `replaced`, phase
  `running`;
- an answer that arrives for stale work MUST be discarded, and nothing is
  forwarded for it. A cancelled call is never an error, whatever the program
  did.

Work that already finished is unaffected, and so is anything it forwarded:
staleness does not travel down the chain. The new signal replaces on its own
(sender, key), so the next hop's forward replaces on its own sender's key.

**A running threshold call is stale when any of its inputs is stale**
(decision of issue #17). The call is cut at the moment any one of the inputs
it was built from is replaced, not only the input that crossed the threshold.
One drop is reported, for the crossing input, reason `replaced`, phase
`running`. The other inputs report nothing, as in 5.2.

Replacement in a group by the same sender (5.2) is a held replacement and is
reported as such; it does not cut a call that has already started.

## 6. Level 0: the one-shot contract (normative)

The default. One process per call. Anything that reads stdin and writes
stdout can be a neuron, and can be tested by hand:
`echo '{"id":5,"service":"checkout"}' | python3 neuron.py`.

### 6.1 Input

The engine starts the command (3.3), in its own process group, writes **one
JSON value** to stdin, and closes stdin.

- A neuron without `threshold`: the value is the payload.
- A neuron with `threshold`: the value is
  `{"payload": P, "inputs": [P1, P2, ...]}`: `payload` is the crossing input's
  payload, `inputs` are all the inputs the call was built from, in arrival
  order (the crossing one last).

### 6.2 Output and exit

- Exit 0, stdout empty (or only whitespace): **quiet**.
- Exit 0, stdout one JSON value (surrounding whitespace allowed): an
  **answer** with that payload. A level 0 program has no envelope: the answer
  is forwarded along all outgoing links, with the inherited key.
- Exit 0 and stdout anything else: an error (code `junk`).
- Non-zero exit, or killed by a signal: an error; the message names the exit
  status and ends with the first 500 characters of stderr, trimmed.
- Output over 1 MiB: an error.

Stderr is for logs; it never affects the outcome (8.5).

### 6.3 Cancel

Stale work (5.5) and timeouts (8.2) end a call by SIGTERM to the process
group, then SIGKILL to it 2 seconds later if anything is left. Output after
the cancel is discarded.

### 6.4 `http` neurons

The same contract over HTTP: `POST` the stdin value as `application/json`;
a 2xx response with a JSON body is the answer, a 204 or an empty 2xx body is
quiet, anything else is an error. Cancel aborts the request. Redirects are not
followed. There is no way to send a secret in version 0 (section 11).

### 6.5 `human` neurons (thin)

The engine shows the input to a person and waits for a typed reply: an empty
reply is quiet, any other reply is a text payload. Cancel and `timeout` stop
the wait. How the person is reached is the engine's business; the rest of
the contract is as for level 0.

## 7. Level 1: the line protocol (normative)

Opt-in, with `serve`. One long-lived process per neuron, started when the
engine starts, speaking JSON lines. Use it when starting a process per signal
is too slow or the program keeps state.

### 7.1 Framing

UTF-8. One JSON object per line, ended by `\n` (`\r\n` is tolerated on
reading). A line is at most 1 MiB. Blank lines are ignored. Every object has
a string `type`. The program's stdout carries protocol lines only; stderr is
for logs (8.5). A program MUST flush after each line.

### 7.2 Messages

Engine to neuron:

| Message | Fields | Meaning |
|---|---|---|
| `hello` | `v` (1), `neuron` (the id) | The first line. |
| `signal` | `re` (integer), `payload`, `inputs` only for a threshold neuron | An input. `inputs` as in 6.1; `payload` is the crossing input's. |
| `cancel` | `re` | Advisory: stop work for `re`. No reply is expected. |
| `ping` | `n` (integer) | Sent only when nothing is in flight. |
| (stdin EOF) | | Shut down (8.4). |

Neuron to engine:

| Message | Fields | Meaning |
|---|---|---|
| `ready` | `v` (1), `maxInflight` (integer at least 1, default 1) | MUST be the first line. |
| `answer` | `re`, `payload`; optional `replaces` (string, or `null` to clear the inherited key), optional `to` (array of neuron ids) | The answer for `re`. |
| `quiet` | `re` | Nothing to say for `re`. |
| `error` | `re`, `message` | The call for `re` failed. Without `re`: the process itself has failed (8.3). |
| `pong` | `n` | Reply to `ping` with the same `n`. |

An engine ignores fields it does not know. It ignores a message of a `type` it
does not know, from a neuron. A neuron SHOULD ignore engine messages it does
not know.

### 7.3 Handshake and versioning

The engine sends `hello` with `v` 1. The neuron's first line MUST be `ready`
with `v` equal to the engine's. Any other first line, a different `v`, or no
`ready` within `timeout` is a failed start (8.3). `v` is the major version of
this protocol: a change that an old peer would misread bumps it. Adding an
optional field or a message type does not.

### 7.4 Correlation and concurrency

`re` is a counter the engine keeps per process (1, 2, 3, ...), not the
signal id: signal and trace ids never reach neurons, so a neuron cannot name
or address anything outside itself. Replies carry the `re` they answer. The
engine MUST NOT have more than `maxInflight` signals outstanding at once; it
holds the rest in order (held, 5.5). Replies may come in any order. A second
reply for one `re`, and a reply for an `re` the engine has cancelled or does
not know, are discarded.

### 7.5 Cancel

The engine sends `cancel` when work for `re` is stale (5.5) or has timed out
(8.2), and discards any later reply for it. The slot of a cancelled `re`
stays occupied until the neuron replies for it (the reply is discarded) or
until `timeout` ms have passed since the cancel; then the engine treats the
process as hung (8.3).

### 7.6 Ping

When the engine has nothing in flight for a neuron it MAY send `ping`; a
neuron MUST answer `pong` within `timeout`. No answer is a hung process (8.3).

### 7.7 Reply checking

An `answer` without `payload`, with a `to` that names a neuron the sender is
not linked to, or with `replaces` that is neither a string nor `null`, is an
error for that `re` (8.1); the answer is not used.

## 8. Errors, timeouts, crashes, junk and stderr (normative)

### 8.1 Errors are local

An error fails the one call. It is reported as an `error` event for the
signal the call answers; nothing is forwarded; no other work is affected. The
network keeps running. A threshold call that fails reports one error, for the
crossing input.

### 8.2 Timeout

A call that takes longer than the neuron's `timeout` (counted from when the
program starts, not while held) is cancelled (6.3, 7.5) and reported as an
error with message `timed out after N ms` and code `timeout`. A cancelled
call that was replaced first is a drop, not a timeout.

### 8.3 Crash, hang and failed start (level 1)

If the process exits, closes stdout, sends `error` without `re`, fails a
ping, or is found hung (7.5), the engine MUST report an error for every call
in flight (code `crash`), kill what is left of the process group, and restart
the process with backoff: wait 100 ms, then double the wait each consecutive
failure up to 5 s. A start that reaches `ready` resets the backoff. After 5
consecutive failed starts the neuron is **failed**: every input delivered to it
is an error (`neuron failed to start`, code `crash`) until the engine restarts.
At engine start, a `serve` neuron that cannot reach `ready` makes the engine
refuse to start.

### 8.4 Shutdown

The engine closes the neuron's stdin. The neuron SHOULD finish nothing new and
exit. If it is still running after 2 seconds the engine sends SIGTERM to the
process group, and SIGKILL 2 seconds after that. Work in flight is cancelled
silently.

### 8.5 Junk on stdout, and stderr

A line on a level 1 neuron's stdout that is not a JSON object with a string
`type` is **junk**. The engine records a `note` of kind `junk` (9.3) with the
neuron id, the line number counted from 1 for the process, and the first 2000
characters of the line. If exactly one call is in flight at that neuron, that
call fails with code `junk`; otherwise only the note is made. Junk never
fails the neuron. A message with a known `re` but a bad shape is an error for
that `re` (7.7), not junk.

Stderr is logs. The engine MAY keep the last 20 lines or 4 KiB of it per
process for error messages (6.2), MUST NOT interpret it, and MUST NOT write it
to the trace except inside that error message.

## 9. The trace an engine writes (normative)

An engine writes traces in [trace format v1](TRACE.md) so the same
validity checks, the replay module and the network view work for it. Section 9
says how this specification's events become v1 events, and what it adds.

### 9.1 Header

| v1 field | Written as |
|---|---|
| `format`, `version` | `"agent-neuron-trace"`, `1` |
| `demo` | the network file's `name` (a free string: readers MUST NOT assume it is one of the three demo names), or the file name |
| `description` | the file's `description`, or one line |
| `library` | the engine's name and version, e.g. `"engine-name 0.1.0"` |
| `neurons` | one entry per neuron of the outer file: `id`, `description`, `kind` (the kind field's name, with `network` written as `"composed"`), and `options` = `{threshold, within, maxHops}` as set |
| `links` | one `{from, to}` per link; a link is one-way, and v1 readers do not assume otherwise (replay does not) |
| `events`, `summary` | below; `summary` MAY be `{}` |

A composed neuron's inner neurons and links never appear.

### 9.2 Events

| Specification | v1 event |
|---|---|
| a root signal (4.1), a forward (4.4) | `signal`; `via` absent (emit) |
| an answer (4.3) | `answer`, with `payload` |
| a quiet answer | `quiet` |
| a drop (5.2 to 5.5) | `drop`, `neuron`, `reason` one of `replaced`, `expired`, `hops` |
| an error (8) | `error`, `neuron`, `message` |

`t` is whole milliseconds since the run started (virtual in conformance
runs). A payload is written as v1 says: a string as `{"type":"text","text":...}`,
any other value as `{"type":"json","data":...}`. (A JSON payload that is a
string is therefore written as text: the one place the mapping loses a
distinction.)

An answer event follows from its input: `replyTo` the input's id, `to` its
sender. A forward carries `cause` (the answer's id). Hops are as in section 2.

### 9.3 What this adds, and why v1 stays v1

TRACE.md's rules: a reader ignores fields and event types it does not know;
adding an optional field or an event type stays version 1; changing what a
field means or the replay rules bumps `version`. Everything below is of the
first kind.

| Addition | Where | Rule for the engine |
|---|---|---|
| `phase`: `"held"` or `"running"` | optional field on `drop` events | MUST be written when `reason` is `replaced`; MUST NOT be written for `hops`; MAY be written for `expired` (always `"held"`) |
| `replaces`: the key | optional field on `signal` events | MUST be written when the signal has a key |
| `code`: `timeout`, `crash`, `junk` or `protocol` | optional field on `error` events | SHOULD be written for the errors in section 8 |
| `note` | new event type: `{type, t, neuron, kind}`, `kind` one of `junk` (with `line` and `text`), `cancel`, `term`, `kill`, `restart` | SHOULD be written for the process facts in section 8 |

**Issue #16, held versus running.** Today a `drop` with reason `replaced`
means two different things: an input that stopped waiting, and running work
that was cut. Version 1's `drop` already carries both under one reason, and
the replay summary relies on that: `heldReplaced` is the `replaced` drops of
`hypothesis` minus the calls cut. The distinction is made with the optional
`phase` field, which is additive: a v1 reader that knows nothing of it keeps
counting `replaced` drops exactly as before. A new reason value (say
`withdrawn` for held inputs) would not be additive: it would change what
`replaced` means to every reader that knows only the old value, and TRACE.md
bumps `version` for that. So the held and running cases keep one reason, and
`phase` tells them apart. When both ends know `phase`, the cut count is the
`replaced` drops with phase `running`, and no reader needs to remember which
signals started a call.

Recording the new fields and `note` events is also what lets a reader explain
a stale drop or a junk line from the trace alone. Two edits to TRACE.md follow
and are not made here: it should list `phase`, `replaces`, `code` and `note`
(they are optional; a line each), and its remark that "links are two-way in the
library" describes the TypeScript library, not the format.

## 10. Conformance (normative)

Behaviour is tested by **fixtures**, run by an engine on a **virtual clock**.
An engine conforms to version 0 when it passes every fixture in
`spec/fixtures/` (to be written alongside the first engine; a fixture is
written once, for all engines). Recorded traces in `traces/` stay as
smoke tests through the replay module; they are not conformance tests,
because they carry real timing.

### 10.1 What a fixture is

A directory with a network file (and any composed files it names) and a
`fixture.json`:

```json
{
  "fixture": 0,
  "description": "One line.",
  "network": "network.json",
  "inputs": [{ "at": 0, "source": "alerts", "payload": { "service": "gateway" }, "replaces": "gateway" }],
  "script": { "logs": [{ "when": { "service": "gateway" }, "after": 20, "reply": { "quiet": true } }] },
  "notes": false,
  "expect": [ ]
}
```

- **inputs**: injections (4.1), each at virtual time `at` ms; `replaces` optional.
- **script**: replaces every program. The key is a neuron's instance path
  (sealing is a rule about what an engine reports, not about the harness).
  Each entry answers one call: the first unused entry whose `when` equals
  (deep equality) the program's input value (6.1) is used; an entry without
  `when` matches any; no match is a harness failure. `after` is the latency in
  ms from when the call starts. `reply` is one of:
  `{"answer": P, "replaces"?: K or null, "to"?: [ids]}`, `{"quiet": true}`,
  `{"error": "message"}`, `{"junk": "text"}`, `{"crash": true}` or
  `{"hang": true}` (never replies). The engine's process layer is replaced by
  the script, so fixtures test what the engine does with an outcome, not how
  it parses bytes (byte-level fixtures are out of version 0).
- **notes**: when `false` (default), `note` events are not compared.
- **expect**: the trace events (section 9) the engine must report, in order.

### 10.2 Virtual clock and order

Time starts at 0 and jumps to the next due moment: an input's `at`, a
reply's `after`, a `within` expiry, a `timeout`. Processing takes no time. In
conformance mode an engine MUST be deterministic: work due at the same
instant is processed in the order it was scheduled, and the deliveries of one
signal happen in the order of `to`. The same fixture MUST give the same
events every time.

### 10.3 Normalisation and comparison

Ids are replaced, across the whole event list, by `#1`, `#2`, ... in order of
first appearance, scanning each event's `id`, `trace`, `replyTo`, `cause` in
that order. An id that appears twice (a `trace` equal to a first signal's id)
therefore gets one name. Events are compared one by one, in order, for every
field, `t` included, with `via` absent equal to `"emit"`. Events of types
`signal`, `answer`, `quiet`, `drop` and `error` are compared; `note` events
only when `notes` is `true`.

### 10.4 A worked fixture (informative)

Network: `s` (source) -> `a` -> `b`, with `a` and `b` being `run` neurons.
Input at 0: payload `{"n":1}`, key `k`. Script: `a` answers `{"n":2}` after
10; `b` is quiet after 5.

```json
"expect": [
  { "type": "signal", "t": 0,  "id": "#1", "from": "s", "to": ["a"], "trace": "#1", "hops": 0, "replaces": "k", "payload": { "type": "json", "data": { "n": 1 } } },
  { "type": "answer", "t": 10, "id": "#2", "from": "a", "to": ["s"], "trace": "#1", "replyTo": "#1", "hops": 1, "payload": { "type": "json", "data": { "n": 2 } } },
  { "type": "signal", "t": 10, "id": "#3", "from": "a", "to": ["b"], "trace": "#1", "cause": "#2", "hops": 2, "replaces": "k", "payload": { "type": "json", "data": { "n": 2 } } },
  { "type": "quiet",  "t": 15, "id": "#4", "from": "b", "to": ["a"], "trace": "#1", "replyTo": "#3", "hops": 3 }
]
```

The fixtures to write first are the behaviours the TypeScript tests pin
today, plus the new rules: emit without waiting; replace and cut; replace of
a held input; threshold, same sender once, different traces apart, `within`,
consumed inputs never expire; a replaced earlier input cuts a running call
(#17); `maxHops` and a loop cut; the alert storm at thresholds 1, 2 and 3;
`to` restricted to links; a quiet answer forwarding nothing; a composed
neuron sealed and quiet; timeout, crash and junk.

## 11. Out of version 0 (informative)

Deliberately left out, so that nobody reads their absence as an oversight:

- **Strength.** Designed, not built. Will live in a state file keyed by
  instance path (so learning stays inside a composed neuron and each use
  learns on its own), never in the reviewed network file; the caller adjusts
  it. Until then every sender counts 1 toward a threshold.
- **Passive and active channels.** Open. It would naturally be a link
  attribute or a reply-envelope field; no field is reserved.
- **Link attributes** of any kind (a link is `{from, to}`).
- **`http` and `human` detail.** Specified only as far as 6.4 and 6.5. There
  is no authentication or header support for `http`; `human` has no defined
  interface.
- **Wasm neurons**, sandboxing beyond the sealing of composed neurons, and
  Windows (process groups and signals are POSIX).
- **Concurrency limits at level 0** (it starts as many processes as there are
  calls).
- **A run-level result or budget**: a total budget per trace, or "the answer
  of the network". Read answers from the trace (4.5).
- **Inspecting the inside of a composed neuron**, and a trace format for it.
- **Byte-level conformance** for level 0 and level 1 parsing, and fixtures that
  use real processes.
- **Hot reload**, and changing a network while it runs.

## 12. Walk-through: one alert (informative)

The files: [alerts.network.json](../spec/examples/alerts.network.json) and
[diagnosis.network.json](../spec/examples/diagnosis.network.json); the programs
are in `spec/examples/neurons/` and `examples/alerts/`. The producer injects
alert 5, `{"id":5,"service":"checkout","severity":"warn",...}`, into `alerts`
with key `checkout`.

1. **`alerts` -> signal #1**, to `logs`, `deploys`, `metrics` (hops 0, key
   `checkout`). Each is run as a level 0 program with the alert on stdin.
2. **`metrics`** prints nothing: it only flags critical alerts. Quiet. Its
   answer goes to `alerts`, nothing is forwarded.
3. **`logs`** finds ERROR lines for `checkout` and prints a flag; **`deploys`**
   finds a deploy 22 minutes ago and prints a flag. Each answer goes back to
   `alerts` (hops 1) and each is forwarded along the single link out of the
   detector, to `hypothesis` (hops 2), with `cause` the answer and the key
   `checkout` inherited. These two forwards are on one trace from two distinct
   senders.
4. **`hypothesis`** has `threshold` 2: the first flag is held (`within` 500
   ms); the second crosses it. One call: the program gets `{"payload": ...,
   "inputs": [logs flag, deploys flag]}`, waits 500 ms standing in for a model
   and answers `{"alert":5,"service":"checkout","detectors":["deploys","logs"]}`.
   The answer goes to the sender of the crossing input (`deploys`), and is
   forwarded to `diagnosis`.
5. **`diagnosis`** is a composed neuron. Its input goes to `topology`, which
   adds `root: "gateway"` and passes everything else through. Its link takes
   that to `check`, which adds `deploy` and `minutes` and is the `exit`: its
   answer is the composed neuron's answer. The outer trace shows one signal
   in and one answer out.
6. The verdict `{"alert":5,"service":"checkout","detectors":[...],"root":"gateway","deploy":"dep-8b21","minutes":14}`
   is the answer event of `diagnosis`. `diagnosis` has no outgoing links, so
   nothing is forwarded: the run ends here.

If a newer alert for `checkout` is injected while step 4's call is running,
the injection makes any still-running detector work for alert 5 stale. When
`logs` then flags the newer alert, its forward (same sender, same key) makes
its earlier forward stale. That forward is one of the running call's inputs,
so the `hypothesis` call is cut (5.5, #17) and one `drop`, reason `replaced`,
phase `running`, is reported. If the newer alert leaves `logs` quiet, nothing
is forwarded and the call finishes.

### Where each piece of `examples/alerts-demo.mjs` went

| Today's caller code | Now |
|---|---|
| `alerts` neuron with a no-op adapter | `"source": true` |
| `net.emit('alerts', json(a), { replaces: a.service })` | injection with a key (4.1). **Finding 1**, below. |
| `net.connect('alerts', d)`, `net.connect(d, 'hypothesis')`, `connect('hypothesis', 'diagnosis')` | seven one-way `links` |
| `onSignal`: "only non-empty answers matter" | a quiet answer forwards nothing (4.4); detectors print nothing to be quiet |
| `net.emit(s.from, s.payload, { to: ['hypothesis'], cause: s, replaces: d.service })` for the three detectors, and again for `hypothesis` -> `diagnosis` | automatic forward along outgoing links, `cause` the answer, key inherited (4.4); `to` is the only outgoing link |
| `new Neuron('hypothesis', ..., { threshold, within: 500 })` | `"threshold": 2, "within": 500` |
| `hypothesis`: `flags[0]`, sorted detector names, the 500 ms wait | the program `hypothesis.mjs` (a decision, so a program); the 500 is its argument |
| `callOf`, `runs`, `stats`, `cutSince`, `heldReplaced`, `expired`, `verdicts`, the log lines | not part of the network. They are observers; every count is derived from the trace (the replay module already does it). Held versus running is `phase` (9.3). |
| `diagnosis`: an inner network, with `door`, `topology`, `check`, and the merge `{...v, call, alert, detectors}` | `diagnosis.network.json`; `door` is not needed (the engine delivers to `entry`); the merge is each inner program passing its input through (**Finding 3**) |
| `composedDiagnosis`'s `live()` abort check | engine rule: replaced or timed-out work is cancelled, inner work included (3.6) |
| `onError` log | `error` events |

**Findings.** Nothing in the alert storm needs a key in the file to read a
payload. Four things are not what the caller code was, said plainly:

1. **The root key is supplied at injection, by the producer.** "Replaces keyed
   by service" survives as an envelope field of the injected signal; the file
   cannot compute it, because that would be reading the payload. Whatever
   produces alerts must know which field makes an older alert stale. This is
   the only caller-side piece left, and it is at the edge of the network, where
   the signal is born.
2. **`call`, the per-run counter, cannot exist at level 0.** A fresh process
   per signal has no memory, so `hypothesis.mjs` does not number its calls.
   The counter was demo instrumentation (a log line, and a field the verdict
   copied); the number of calls is a count of `acting` in the trace. A neuron
   that truly needs state across calls uses `serve` (level 1).
3. **Merging is a convention between programs, not an engine feature.** The
   verdict has the alert, the detectors, the root and the deploy because
   `topology` and `check` copy their input and add to it. The engine does no
   merging. A neuron that forgets to pass the context through loses it; there
   is no field to ask the engine to keep it.
4. **There is no network result.** The verdict is read from the `answer` event
   of `diagnosis` in the trace (4.5), where the demo collected it into an
   array. A caller that wants it as a return value reads the trace.

One behaviour changes on purpose: under #17 a call is also cut when an
*earlier* counted input is replaced, so a recording of the storm made by an
engine following this version may cut more calls than the TypeScript demo
does today.

## 13. Appendix (informative)

### 13.1 A Python neuron on level 0 and on level 1

Level 0: `echo '{"service":"checkout","severity":"crit"}' | python3 flag.py`

```python
import json, sys
alert = json.load(sys.stdin)
if alert["severity"] == "crit":
    print(json.dumps({"detector": "severity", "service": alert["service"]}))
# printing nothing is "quiet"
```

Level 1, in a network file as `{"id": "judge", "serve": ["python3", "judge.py"]}`:

```python
import json, sys

def send(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()

hello = json.loads(sys.stdin.readline())
send({"v": 1, "type": "ready", "maxInflight": 1})
for line in sys.stdin:                      # end of input means shut down
    message = json.loads(line)
    if message["type"] == "signal":
        n = message["payload"].get("n", 0)
        if n > 0:
            send({"type": "answer", "re": message["re"], "payload": {"n": n + 1}})
        else:
            send({"type": "quiet", "re": message["re"]})
    elif message["type"] == "ping":
        send({"type": "pong", "n": message["n"]})
    # "cancel" is advisory: a single-threaded program has nothing to stop
```

### 13.2 How the TypeScript library maps

The library in `src/` predates this document and is the first engine to be
brought into line. Differences to remove, none of them hidden:

- `Neuron.connect` links both ways; the spec's links are one-way. The engine
  needs a one-way link primitive, and `connect(a, b)` becomes two links. A
  network file maps to one-way primitives only.
- Nothing forwards answers today: callers do, with `emit(..., { cause })`. The
  engine does it from the links, and the `send` and `Neuron.query` paths stay
  as direct asks outside this specification.
- Only the crossing input's replacement cuts a running threshold call; #17
  makes any counted input do it.
- `onDrop` reports held and running work under one reason; `phase` (9.3) is
  the trace-level fix, and what the callback does is the library's own API.
- `ShellAdapter` writes a text payload raw and parses non-JSON stdout as text;
  level 0 here is JSON both ways.
- Its trace `neurons[].kind` strings (`js`, `shell script`, `model stand-in`)
  stay valid in v1 (the field is descriptive).
