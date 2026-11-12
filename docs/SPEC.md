# agent-neuron specification, version 0.1

Status: draft. Version 0.1 is version 0 plus budgets and the `finished` event
(section 10), effects with the `unknown` outcome (section 11), the live trace stream (9.1), the `may`
declaration (2.4), what confining a program to it means (2.9) and a neuron described in a file of its own (2.10); a network file's `version` stays `0`, because the new fields are
additions. The spec may change in any way until it is called 1; a change
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
8. Conformance (normative; the format is in [CONFORMANCE.md](CONFORMANCE.md))
9. Observers (normative)
10. Budgets (normative)
11. Effects (normative)

Appendices (informative): A. Walk-through: one alert. B. Out of version 0.1.
C. How the TypeScript library differs. D. A page for `human` neurons. E. Where each rule lives.

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
  path, or name what the interpreter needs. A value comes from `.env` beside the file that declares
  your neuron when that file has the name (`OPENAI_API_KEY=...`), and from the engine's environment
  otherwise (2.4): a neuron can carry its own settings, and your program just reads its environment.
- **What your program may touch is declared, and held only when a run asks.** A neuron's `may` names
  the hosts it may connect to, the addresses it may listen on and the paths it may read and write;
  without one it is declared to touch nothing (2.4). An engine may run your program confined to that
  (2.9), and then a program that reads its own script must say so (`"read": ["."]`), and one that
  calls a named host must use the proxy it is told about in `HTTP_PROXY` and `HTTPS_PROXY`. Not every
  engine confines, and none does unless asked, so write a `may` as the truth about your program and
  do not read one as a jail.
- **Logs go to stderr.** Level 1 stdout carries protocol lines only.
- **Answer a `cancel`.** The engine keeps a cancelled call's slot until you reply for
  its `re`; send `quiet` (the reply is discarded). A neuron that never does is
  killed and restarted after `timeout`.
- **Cost is not your program's business.** A neuron may carry a `cost` and a network a
  `budget`; the engine reserves, refuses and counts, and your program neither sees nor reports it
  (section 10). A call it refuses never starts.
- **Quiet is a choice.** An error (non-zero exit) forwards nothing too, but is
  recorded as a failure.
- **A neuron that does something in the world** (sends a message, writes a record) is
  declared `"effect": true`. The engine then never cuts a call that has started because
  its input went stale, and if the call crashes or times out it records `unknown`, not
  an error: nobody can tell whether it happened. Make such a program idempotent by an id
  in its payload (section 11).
- **Running a network.** `npm run engine -- check <file>` validates a network file
  (schema and the findings of section 2.7, with line and column). `npm run engine --
  run <file> --inputs <jsonl> [--trace <out.json>]` runs it: the inputs file holds one
  JSON object per line, `{"at": ms, "source": id, "payload": ..., "replaces": key, "budget": units}`
  (`replaces` and `budget` optional), and the run prints the answers of the neurons with no
  outgoing links. Try `npm run demo:network` (the alert storm below). The TypeScript
  library does not read network files; the reference engine is `src/engine/`
  (Appendix C). `http` and `human` neurons are not run yet.
- **Running a level 1 neuron.** Put the quick-start program above in a file,
  say `flag.py`, and name it in a network file as
  `{ "id": "flag", "serve": ["python3", "flag.py"] }`; the engine starts it with the
  engine and talks to it over the pipe, so there is nothing to start by hand. A complete
  one (a threshold neuron that handles `cancel`, 29 lines) is
  `spec/examples/neurons/hypothesis_serve.py`; `spec/examples/alerts-python-serve.network.json`
  runs the alert storm with it: `npm run demo:network -- spec/examples/alerts-python-serve.network.json`.
  By hand, `printf '{"v":1,"type":"hello","neuron":"x"}\n{"type":"signal","re":1,"payload":{"n":1}}\n' | python3 flag.py`
  prints `ready` and then the answer.

## 1. Terms (informative)

- **Neuron**: an entry in a network file, with an `id` and one **kind**
  (`source`, `run`, `serve`, `http`, `human`, `network`). Kind means this and
  nothing else. An entry may instead name the neuron's own file (`neuron`, 2.10);
  the neuron then has the kind its file gives it.
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
(`urn:agent-neuron:schema:network:0`) because the repository was private when
it was written; it may become a URL later.

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
| `budget` | units (10.1) | none | The most one trace may spend (10). |

### 2.2 A neuron

`id` (required, `^[A-Za-z][A-Za-z0-9_-]{0,63}$`, unique in the file),
`description` (string; SHOULD be set, it reaches the trace), `at` (optional), exactly one kind
field (2.3), and the options its kind allows (2.4). An entry whose kind field is `neuron` takes the kind
and the rest from the neuron's own file (2.10).

`at` is `[x, y, z]`, three numbers from -1000000 to 1000000 in any unit the author
picks: where a drawing of the network puts the neuron. It is a **descriptive
field**, like `description`: for people and tools, never for an engine. An engine
MUST accept it on every kind of neuron, MUST check its shape, and MUST NOT let it
change anything it does: routing, timing, events, the trace or what a program
receives ([decision 0011](decisions/0011-descriptive-fields-and-at.md)). It is not
an option and does not count against 2.8's budget of options.

### 2.3 Kinds

| Field | Value | Program | Contract |
|---|---|---|---|
| `source` | `true` | none; receives nothing | injection, 4.1 |
| `run` | command | the command | level 0, 3.1 |
| `serve` | command | the command, long-lived | level 1, 3.2 |
| `http` | `http://` or `https://` URL | an endpoint | 3.3 |
| `human` | `true` | a person | 3.4 |
| `network` | path | another network file | 2.6 |
| `neuron` | path | what the neuron's own file names | 2.10: the neuron is the one its file describes, with that file's kind |

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
| `timeout` | integer ms | 30000 | all but source | longest a call may run, counted from its start; for `network`, the whole composed call. At most 3600000 (an hour); on `human`, at most 86400000 (a day: a person is slower than a program; [decision 0008](decisions/0008-human-neurons-on-a-page.md)) |
| `env` | array of unique names | `[]` | run, serve | environment variables passed through: names only |
| `cwd` | path | the directory of the file that declares the neuron | run, serve | working directory |
| `cost` | units (10.1) | none | all but source | what one call reserves, and the most it may consume (10) |
| `effect` | `true` | none | run, serve, http, human | a started call is never cut by staleness; its failures are `unknown` (11) |
| `may` | object (below) | none: nothing | run, serve, network | what the neuron's program is allowed to touch |

A program's environment is empty except `PATH` and the `env` names (a name the
engine lacks stays unset). `threshold` and `within` are not allowed on `network`.

**A neuron's own settings** ([decision 0022](decisions/0022-what-a-swarm-needs.md)). The value of an `env`
name comes first from the neuron's settings file, and from the engine's environment only when the file
lacks the name or there is no file. The settings file is `.env` in the directory of the file that
declares the neuron's program: the neuron's own file for an entry that names one (2.10), the network
file for a neuron written inline. It is lines of `NAME=value`; blank lines and lines that start with
`#` are skipped; a value wholly inside single or double quotes loses them; there is no interpolation
and no escape. A line that is anything else is an error (2.7), and the engine refuses to start. Only
the names in `env` are passed: a name the file has and the neuron does not list reaches no program,
and a neuron that lists none never has the file read. An engine MUST NOT report a value: not in an
event, a note, the trace or a finding of `check`, which names the file and the line and never the
text. The file is no field of the network: nothing names it, and an engine that starts no programs
ignores it. The engine reads it, so the program needs nothing in its `may` for it.

**`may`** ([decision 0016](decisions/0016-the-self-describing-neuron.md)) is a closed object of four
members, each optional, each an array of unique literal strings:

| Member | Entries | Declares |
|---|---|---|
| `connect` | `host:port` | the outgoing connections the program may make |
| `listen` | `address:port` | the addresses the program may accept connections on |
| `read` | paths | the files or trees the program may read |
| `write` | paths | the files or trees the program may write |

- **Absent grants nothing.** A neuron with no `may`, a `may` of `{}`, a member left out and a member
  that is `[]` all declare the same for what they leave out: none of it.
- **`host:port`** is a host or address of lower-case letters, digits, dots and hyphens that starts and
  ends with a letter or digit, at most 253 characters, then `:` and a port from 1 to 65535 with no
  leading zero. There is no wildcard and no bracketed address in this version.
- **A path** is a literal that is not empty, written as the program would name it: relative to the
  neuron's working directory, or absolute.
- **On a `network` neuron** `may` declares what its inside may touch, and it MUST cover what the
  neurons of its file declare: for each member, every string a neuron of that file lists is listed
  by the `network` neuron too, compared as written. A composed neuron inside counts by its own
  `may`. `check` reports the first thing it lacks as an error (2.7). It MAY list more.
- **An engine MUST check `may`, and MAY confine a program to it** (2.9). An engine that does not
  confine, or a run that did not ask it to, MUST NOT be described as confined. `may` changes
  nothing an engine reports: no event, no note, no field of the trace.

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
2. The first reply of `exit`, an answer or quiet, is the composed neuron's reply, with the
   `replaces` it carries: a string sets the key of the forward (4.4), `null` clears it, absent
   inherits the outer input's key.
   Remaining inner work for the call is cancelled without reports.
3. If the inner network has nothing running, queued or held for the call and
   `exit` has not replied, the composed neuron is quiet; if an inner neuron
   failed meanwhile, it is an error, code `inner`.
4. Calls are isolated: staleness inside an instance is scoped to one call, so
   one call can never cut another. A call is cut only when its outer work goes
   stale or times out; then all its inner work is cancelled, recursively.
5. `maxHops` and `timeout` apply at the outer boundary. Hops inside start at 0
   and do not add to the outer chain.
6. The composed neuron's `cost` and the inner file's `budget` bound what the inside may spend (10.5).
7. A composed neuron is never an `effect`, and its file contains none (11.5).

**Sealing** (MUST): inner ids, signals, traces and error text never appear in
anything reported about the outer network; from outside, a signal goes in and
a reply comes out, and an inner failure is only the composed neuron's own
error. A separate inspection tool for the inside, keyed by instance path, is an
operator's tool and is not in version 0.

Sealing is between neurons, not against whoever holds the file. A composed network run from a
local file is sealed from the other neurons of the outer network and from the outer trace; the
person or program that holds the file and runs the engine can read it, run it, and inspect the inside.
A boundary that must hold against the holder of the inside (one run by another party, say) is not a
composed neuron: it is an `http` neuron (3.3) to that party's own engine, which keeps its inside and
its accounting (10.5) to itself.

### 2.7 `check` (normative)

An engine MUST offer a check that does not run the network. It MUST report,
with file, line and column where it can, and exit non-zero on any error:
schema violations; duplicate ids; a link naming an unknown id (SHOULD suggest
the nearest id); a duplicated link; a link into a source; a `network` file that
is missing, invalid, lacks `entry`/`exit`, has a source or an `exit` with
outgoing links (errors); a cycle in which no neuron has `maxHops` (error); a `budget`
or `cost` amount that is not a whole number from 0 to 2^53 - 1, a `cost` naming a unit the
file's `budget` lacks, an empty `budget` or `cost`, or a non-source neuron with no `cost` in a file that
has a `budget` (10.1) (errors); a `network` file that contains a neuron with `effect` (11.5) (error); a `cost` above the file's ceiling in some unit (the neuron can never run),
or a `network` neuron whose file has neurons costing a unit its `cost` does not name (those calls are always
refused, 10.5) (warnings); an executable that cannot be found (warning); a `network` neuron whose `may` does not cover what the
neurons of its file declare (2.4) (error); for an entry that names a neuron's file (2.10): a file that is
missing or invalid, an option on the wrong side (one that is the network's in the file, or one that is the
neuron's own on the entry), and a `timeout`, `env` or `may` on the entry that is wider than the file's (errors); a settings file of a neuron that lists `env` names with a line that is not `NAME=value` (2.4) (error, naming the file and the line, never the text).

### 2.8 Field budget (informative)

10 top-level fields (`budget` is the ninth, `ground` the tenth — [decision
0012](decisions/0012-the-ground.md)); per neuron `id`, `description`, `at`, one kind field, at most 9
options; 2 per link. The kinds are seven: `neuron` is the seventh, admitted by
[decision 0019](decisions/0019-the-neuron-file.md) and costing no option. A new field must answer one question: does it say how
neurons connect, or how one decides? Deciders are programs. A second, closed
class of field is descriptive (for people and tools, acted on by no engine):
`description` and `at` are the only two, and another needs its own decision record
([decision 0011](decisions/0011-descriptive-fields-and-at.md)). `CONTRIBUTING.md` holds
the rule. Version 0.1 added `budget` (the ninth top-level field), `cost` (the seventh option) and `effect` (the
eighth): a limit on spend is the engine's to enforce, not a program's to declare for itself, the same class
as `timeout` and `maxHops` ([decision 0006](decisions/0006-budgets.md)); whether staleness may cut a call,
and what a failure of it means, is likewise the engine's to apply, not something a program can do for itself
([decision 0007](decisions/0007-effects-and-the-live-stream.md)). `may` is the ninth, admitted when the budget was
reopened for it: what a program may touch is a limit of the same class, since a program cannot be trusted to
confine itself ([decision 0016](decisions/0016-the-self-describing-neuron.md)).

### 2.9 Confinement (normative)

An engine MAY offer to **confine** a run: to start every `run` and `serve` program of the network
tree so that it can touch what its neuron's `may` names (2.4), a fixed baseline, and nothing else.
The reasons are in [decision 0018](decisions/0018-confining-a-program-to-its-may.md). An engine
that offers it MUST follow this section. How a run asks is the engine's business (the Rust
engine's is `run --confine <file>`); a run that does not ask is not confined, and whether
confinement should be the default is not decided in this version.

**It fails closed.** A confined run MUST be refused, before any process starts, when the engine
cannot hold some program to its `may`: on a platform where it has no mechanism, for a `may` entry
its mechanism cannot enforce, or for a program it would start somewhere it cannot confine. An
engine MUST NOT run part of a network confined and the rest not.

**The baseline.** Whatever its `may` says, a confined program has:

- what the platform itself requires for a process to start;
- its own stdin, stdout and stderr;
- the right to start processes, and to signal the processes it started and no others;
- the metadata of any path (that it exists, its size and dates), never its contents;
- the trees the operator names as holding the machine's interpreters and tools. These are given to
  the engine outside the network file: they differ from one machine to the next;
- the platform's TLS configuration and trust roots;
- its working directory as a directory and no deeper: it can learn where it is and see the names
  beside it, and cannot read one of them or look inside a directory below.

A process a confined program starts is confined as it is. Nothing else is in the baseline: not
the files of the working directory, not a temporary directory, not a home directory. A neuron with
no `may` has the baseline alone.

**Paths.** A `read` or `write` entry is resolved against the neuron's working directory when
relative. It grants the file it names, or, if it names a directory, the whole tree below it. It is
resolved through symbolic links when the program starts; an entry whose last part does not exist
yet is granted as written under its resolved parent, so a program may create the file it declares.
`write` does not include `read`: a neuron that does both declares both.

**Connections.** A confined program can open a connection only to a `host:port` its `connect`
lists, and accept connections only on an `address:port` its `listen` lists. An engine MAY hold a
named host through a proxy it runs for that neuron: the program can then reach that proxy and no
other address, the proxy opens a way out only to what the neuron lists, and the program is told
where it is in `HTTP_PROXY` and `HTTPS_PROXY` (and their lower-case forms). A program that does not
use the proxy then reaches no named host, and that conforms: the rule is that nothing unlisted is
reached, not that everything listed is reachable by every program. One neuron MUST NOT be able to
use what another neuron's `may` grants.

**Nothing is reported.** A confined program that tries what it may not is refused by the platform
as it would be by any permission, and what follows is the program's own outcome (an `exit` error,
say, 6.1). Confinement adds no event, note or field to the trace (2.4), and a confined run of a
program that stays inside its `may` reports what an unconfined run does.

**Conformance does not reach it.** Fixtures start no process (8.1), so no fixture pins this
section; an engine that confines tests it on real processes, for each platform it claims.

### 2.10 A neuron in a file of its own (normative)

An entry may name the neuron's own file instead of writing the neuron inline:
`{ "id": "notify", "neuron": "kit/notify.neuron.json" }`, the path relative to the declaring file. The
reasons are in [decision 0019](decisions/0019-the-neuron-file.md).

**It is not a boundary.** The engine reads the file, applies the entry on top, and the neuron is from
then on the one neuron it would be written inline: no sealing, no trace of its own, no instance. A run
MUST report exactly what a run of the same network with that neuron written inline reports, the trace
header included: the neuron appears with the kind its file gives it, and nothing reported says it was
named by a file.

**The neuron file** is one UTF-8 JSON object with a closed schema of its own
([spec/neuron.schema.json](../spec/neuron.schema.json)): `version` (`0`), `description`, exactly one of
`run`, `serve`, `http` and `human`, and the options that are the neuron's own (below). It has no `id`,
and it is never a `source`, a `network` or another `neuron`: a neuron whose inside is other neurons is
named by `network` (2.6). A command's relative paths and the neuron's `cwd` are relative to **the neuron
file's directory**; its working directory defaults to that directory, and the paths of its `may` are read
from there (2.9).

**What is the neuron's, and what is the network's.** A fact that holds wherever the program runs is the
neuron's and is written in its file. A fact that holds only in this network is the network's and is
written on the entry.

| | In the neuron file | On the entry that names it |
|---|---|---|
| the kind and its command or URL; `cwd`; `effect` | yes | never |
| `may`, `env`, `timeout` | yes | to narrow only |
| `description` | yes | yes; the entry's is the neuron's when both have one |
| `id`, `at`, `threshold`, `within`, `maxHops`, `cost` | never | yes |

Each option is allowed only where it is on an inline neuron of the file's kind (2.4). An option on the
wrong side is an error (2.7).

**Narrowing.** The entry may restate three of the neuron's options, each only to ask for less:
`timeout` at most the file's (its default of 30000 when the file sets none); `env` a subset of the
file's names; `may`, on a `run` or `serve` neuron, for each member a subset of the file's entries,
compared as written. An option the entry leaves out stands as the file wrote it; one the entry states
replaces the file's. An entry that asks for more is an error (2.7), and the engine refuses to start.

The fold of 2.4 reads the neuron as narrowed: a composed neuron's `may` covers what its neurons may
touch after their own entries have narrowed them. A `network` neuron's own `may` is not narrowed by the
file that uses it: the fold forbids going below its inside.

A neuron written inline remains valid, and means what it meant.

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

One process per neuron (per instance), started when the engine starts. It is
**ready** when its process has said `ready`; a neuron of any other kind is ready at once.

**Readiness folds outward** ([decision 0017](decisions/0017-the-cut-a-link-carries-and-readiness.md)).
A network is ready when every neuron it contains is ready, a composed neuron (2.6) when its inside
is. A `serve` neuron that does not reach `ready` within its `timeout` has failed to start, and so
has every network that contains it, outward to the network the engine was asked to run: the engine
refuses to start. Nothing is delivered to a network that is not ready. Only what a network contains
is folded: an `http` neuron names another party's endpoint, and is ready whatever that party is
doing. After the start nothing folds: a neuron whose later restarts fail is failed (below), and the
network around it keeps running.

**Framing.** UTF-8, one JSON object per line ended by `\n` (`\r\n` tolerated
on read), at most 1 MiB, blank lines ignored, every object has a string
`type`. Stdout carries protocol lines only; stderr is logs. Flush every line.

**Messages.** Engine to neuron:

| `type` | Fields | Meaning |
|---|---|---|
| `hello` | `v` (1), `neuron` (the neuron's id in its file) | first line |
| `signal` | `re`, `payload`, `inputs` only if threshold > 1 | an input; value shapes as 3.1 |
| `cancel` | `re` | advisory; the neuron SHOULD stop, and MUST reply for `re` (any reply, such as `quiet`) to free its slot |
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

**Cancel** is sent for stale work (never for a started effect call, 11.2) and timeouts. A cancelled `re` keeps its slot
until the neuron replies for it or `timeout` ms have passed, then the process
is treated as hung (6.1). So a neuron that stops cancelled work still replies
for it (the reply is discarded, `quiet` is enough); one that drops it silently
is killed and restarted when the time is up.

**`timeout`** is per call, from the moment the `signal` line is written. Ping
is sent only when nothing is running; no `pong` within `timeout` is a hang.

**Reply `to`.** A set of neuron ids, each an outgoing link of the neuron; the
forward lists them in link declaration order whatever the order given.
`to: []` forwards nothing but the answer is still reported. With the network's
`ground` (4.5), an omitted `to` is the draw: one outgoing link by weight. A reply's `replaces`
longer than 256 bytes, or a `to` that names a non-linked neuron, or an
`answer` without `payload`, is an error (6.1, code `protocol`).

**Shutdown.** The engine closes stdin; after 2 s it sends SIGTERM to the
process group, after 2 s more SIGKILL. Running work is cancelled silently.

**Restart.** If the process exits, closes stdout, sends `error` without `re`, or
hangs, every running call fails (code `crash`) and what remains of the process
group is killed. Before each restart the engine waits `min(100 * 2^f, 5000)` ms,
where `f` is the number of consecutive failed starts. A start **fails** if it does not reach
`ready`, if the process ends less than 1000 ms after `ready`, or if it is killed as hung (a `ping`
unanswered); a `pong`, or a life of 1000 ms or more that does not end in a hang, clears `f` (so a neuron that dies
right after `ready` does not restart at full speed for ever). When `f` reaches 5 the neuron
is **failed**: every input to it is an error (code `start`) until the engine restarts; a
call queued when that happens is failed the same way. The interval between pings is the
engine's; a conformance fixture sets it (8.1).

**Over-long line.** A stdout line over 1 MiB fails the call with `output` if exactly one call
is running, else every running call fails with `crash`; the process group is killed and the
neuron restarted either way.

**Junk.** A stdout line that is not a JSON object with a string `type` is junk:
the engine reports a note (7.2) with the line number (from 1, per process) and
the first 2000 characters. "Running" counts every call that holds a slot, a
cancelled one included; a cancelled call is never failed. If exactly one call is running at that neuron it
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
interface is the engine's business; the reference engine's is a page (Appendix D).

## 4. Routing (normative)

### 4.1 Injection

The engine MUST let a producer inject a payload into a source, with an optional
replaces key (at most 256 bytes; longer is refused to the producer). It creates
a root signal: new id, `trace` its own id, `hops` 0, `from` the source, `to`
the source's outgoing links in declaration order, `replaces` as given. It may also lower the
trace's budget (10.2). Only the
producer knows what makes one input stale, so the key is its job; nothing else
reads a payload to compute one.

**A live producer** ([decision 0020](decisions/0020-one-loop-fed-from-outside.md)). A producer need
not know its injections in advance. An engine MAY take injections while a run is under way; each is
then injected at the instant the engine takes it, and the rules above are unchanged. While such a
producer may still inject, an injection is still to come, so the run is not over (6.2 rule 6); it
is over when the producer has said it is done and nothing is left. An injection the engine refuses
(an unknown source, a key over 256 bytes, a budget it may not carry) is refused to that producer
and to nothing else: the run goes on. A moment counted from the start of the run (the `at` of a
file of inputs, section 0) belongs to injections known in advance, and a live one carries none.

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
subset the reply names; a network that declares `ground` and a reply that names
no `to` route by the draw of 4.5 instead), `replaces` the reply's value (string sets, `null`
clears) or else *s*'s key, inherited. Quiet, an error, a refused input, stale
work and a neuron with no outgoing links forward nothing. Forwarding along
links is the only way a neuron reaches another neuron.

Each neuron on a path adds **two hops** (reply, then forward). A neuron at depth
`d` (the first neuron after the source is depth 1) receives hops `2(d-1)`; `maxHops` is a limit on that
count, not on neurons deep: a limit of 4 lets a chain reach its third neuron.

A run's result is not a return value: read the answers of neurons with no
outgoing links from the trace. An engine MAY also write them out when the run ends, with each
trace's failures and drops, as one object read off the trace and adding nothing to it (Appendix C,
[decision 0022](decisions/0022-what-a-swarm-needs.md)).

### 4.5 The ground (normative)

A network file MAY declare `ground` ([decision 0012](decisions/0012-the-ground.md)): the link layer's strength — a
value kept per *place* (a place is a neuron id), a rule that deposits on places,
and, when chosen, one-link routing for a reply that names no `to`. Without the
field this section does not apply and every out-link counts 1. `ground` is
`{deposit, evaporate, select, base, power, seed}`, every field optional. A
composed network's inner file MUST NOT declare one; the scope question waits.

- **State.** A place's entry is `(v, at)`: the value and the instant of its last
  write. A place never written starts absent: an effective value of 0 at every
  instant. An entry's effective value at `t` is `v·exp(-evaporate·(t − at)/1000)`;
  a read never changes the entry. The state lives for the run; carrying it
  between runs is durable state (Appendix B).
- **Deposit.** With `deposit` declared, the engine checks every reply's payload
  for the fields `deposit.on` (a string) and `deposit.add` (a finite number).
  Both present, and `deposit.by` absent or the replying neuron: one write —
  entry present: `v := v·exp(-evaporate·(t − at)/1000) + add`; entry absent:
  `v := add`; `at := t`. A negative `add` lowers a place (a trail marked dead).
  `evaporate` is a per-second rate; 0 when absent. Every write is one `ground`
  event (7.2), emitted immediately after its answer's event; a payload missing
  either field deposits nothing, and a reply deposits at most one place.
- **Selection.** With `select` true, a reply that names no `to` forwards along
  exactly one of *N*'s outgoing links, in declaration order, chosen by weight
  `max(0, base + effectiveValue(to))^power`; `base` 1, `power` 1, `seed` 0 as the
  defaults. A total weight of 0 forwards nothing, reported like a `to` of `[]`
  (4.4). A reply that names a `to` never draws, nor does an injection (4.1).
- **The generator is pinned.** The engine keeps one 32-bit state, started at
  `seed`, drawn exactly once per omitted-`to` reply — whatever the outcome,
  however few the links. The word, with 32-bit wrap add and multiply and
  zero-fill right shifts:

      s := s + 0x6D2B79F5
      t := s
      t := (t xor (t >> 15)) × (1 | t)
      t := ((t + ((t xor (t >> 7)) × (61 | t))) xor t)
      u := (t xor (t >> 14)) (unsigned) / 2^32

  The pick is the first link, in declaration order, whose cumulative weight is
  greater than `u·total`; if rounding leaves none, the last link.

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
timer, started when held; at the end it is dropped as `expired` (no phase: 7.2). Consumed inputs never expire. Held inputs do not
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

**A key reaches every outgoing link of its sender** ([decision 0017](decisions/0017-the-cut-a-link-carries-and-readiness.md)).
The older signal may have gone where the newer one does not: a level 1 reply may name some of its
links (3.2), and the ground forwards along one (4.5). The older work is stale all the same. So a
signal with a key is delivered to the neurons in its `to`, and every other neuron its sender has an
outgoing link to receives a **cut**: the sender and the key, and nothing else. A neuron treats a cut
as the arrival of a newer input of that (sender, key) with no input in it: its older work for the
pair is made stale by the rules above, and nothing is held, queued or run. A cut for a pair it has
no work for does nothing. A cut is not a signal and not an event: the `drop` it causes is the record
(6.2 rule 2 places it), and it is delivered at the instant its signal is emitted.

An engine that holds the work of every neuron in one place applies this by holding it, and sends
nothing. Where the sender and a target are held apart, the cut is what crosses between them. If it
cannot be delivered, the work there is not made stale: it ends as it would have, and its answer is
forwarded. That is the one case in which this section is best effort, and it is never an error.

**Any replaced input cuts a running threshold call.** The call is stale when any
input it was built from is stale, not only the one that crossed. One drop is
reported, for the crossing input (which may itself not have been replaced),
phase `running`. The other inputs report nothing.

**An effect call is not cut** (11.2): staleness marks it, and only its forward is suppressed. A waiting
effect call (held or queued) has not started and is dropped as above.

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
| a call whose `cost` the pool cannot cover in every unit (10.3) | `budget` | `budget:`, then what was short | nothing reserved; the call never starts |
| composed neuron with no reply after an inner failure | `inner` | `no answer: an inner neuron failed` | |
| level 1 `error` with `re` | `neuron` | the neuron's message | |

`S` in `signal S:` is the signal's name (`SIGKILL`, `SIGUSR1`), never its number.

**Effect calls.** For a neuron with `effect` (11), every failure above except codes `budget`, `start` and
`neuron` is not an `error` but an `unknown` event with the same fields and the same `code` (11.3): the
call started, and nobody can tell whether the effect happened. `budget` and `start` mean the call never
started, `neuron` is the neuron saying itself that it failed; they stay errors.

### 6.2 Event order

An engine MUST report events in this order, so that two engines produce the same
list.

1. Work due at the same instant is processed in the order it was scheduled ([CONFORMANCE.md](CONFORMANCE.md) 4.1 says what
   that means for injections and for calls started by one event). A call's timeout and a held input's
   `within` expiry are scheduled like any other wake-up and have no place of their own at an instant: a timeout
   scheduled before another call's reply or before an expiry goes before it, one scheduled after goes after.
   A timeout is scheduled when its call starts; rule 7 is the one exception to this ordering.
2. A signal's event is reported first, then, at the same `t`, **immediately
   after it**: (a) the drops of the work that signal made stale, in the order
   that work was created; (b) for each target in `to` order, its delivery
   effects (a `hops` drop, or a held replacement drop).
3. A reply is reported before its forward.
4. A running drop is reported when the work goes stale; a `note` for the
   cancel follows it, and the later `term` and `kill` notes at their own times.
5. A call that ends exactly when its input goes stale counts as stale.
6. A **run is over** when nothing is running or queued, no input is held, no injection is still to come, no observer injection is pending, and no
   cancelled work is left behind: a cancelled level 0 program's process group must be gone
   (after the kill, if it ignored SIGTERM) and a cancelled level 1 call's slot must be free
   (the neuron replied for it, or was treated as hung). The same holds for a process group the engine terminated
   after a crash, hang or over-long line: a kill grace already under way keeps the run open until the group is gone,
   and its `kill` note is recorded. The trace records those kills.
   Held inputs keep their `within` timers, so an engine that waits for the end
   reports their `expired` drops. A pending restart delay or idle ping is not work: it does not keep the run
   open, and the engine stops the process silently when the run ends (no `restart` note). A fixture runs the virtual clock until nothing
   is due. A real engine's "wait until idle" MAY stop earlier, ignoring held
   inputs (the TypeScript `idle` does); then their drops are not reported.
7. A call's outcome due at the same instant as its `timeout` is processed first: a call
   times out only if it is strictly longer than `timeout`. (A level 0 program's reply, a
   level 1 reply, the exit of a composed neuron: whatever ends the call at that instant.) A timeout has no
   other privilege: against anything else due at that instant, rule 1 decides.
8. A trace's `finished` event (7.2) is reported right after the last event its last unfinished
   work caused: the reply and its forward and their delivery effects, or the `error`, the `unknown`, or the `drop`.
   A process note that belongs to that instant and follows its cause (the `kill` after the `error` of
   an over-long output) comes after `finished`.

### 6.3 Process notes

A `note` (7.2) records a fact about a process. An engine MUST write these, and conformance
fixtures with `notes` compare them. Order at one instant: the event that reports what
happened to the call (the `drop` of a cut, the `error` or `unknown` of a timeout or a crash, ...) first,
then the notes of what the engine does to the process, in the order `cancel`, `term`, `kill`.
The notes of a budget (10) are written only when the network has a `budget`:

- `reserve`: when a call with a `cost` starts (10.3), with `units`: the cost. Before anything else
  the call causes. A call that is refused writes none.
- `commit`: when the call's program is over (10.4), with `units`, `ms` and `basis`. Just before the
  event that reports the outcome (`answer`, `quiet`, `error`, `unknown`); a cut call has no such event, so its
  `commit` comes after its `cancel`, `term` and `kill` notes, when the program is gone. A timeout's
  `error` (or `unknown`) comes at the timeout and the `commit` when the program is over.

- `cancel`: when the engine cancels a call that has started (stale, timeout), after the drop or
  error. A call that is only queued for a slot, or a held input, has not started: it is dropped
  without a `cancel` note.
- `term`: when the engine sends SIGTERM to a process group that still has a live member (a
  cancelled level 0 call; a level 1 process that crashed, hung, or wrote a line over 1 MiB).
  Nothing is written for a group that is already empty. A group's members are the process and whatever it
  left running in the group: a program that exits on SIGTERM but leaves a child that ignores it still has a
  live member.
- `kill`: when the engine sends SIGKILL to a group that still has a live member: 2000 ms
  after `term` if anything remained (so also when the process itself ended on SIGTERM and left a member), or at once for level 0 output over 1 MiB (the `error`
  first, then the note).
- `restart`: when the new level 1 process is spawned, after the backoff delay, not when it is
  scheduled.
- `junk`: when the junk line is read, so before the `junk` error it causes.
- `inject`: an observer's injection, just before the injected signal (9), with `trigger`: the `id` of the event that fired it, or, when that event is a `finished` (which has no `id`), its `trace`.

## 7. The trace (normative)

An engine writes [trace format v1](TRACE.md), so the validity checks, the replay
module and the network view work unchanged.

### 7.1 Header

`format` and `version` as v1. `demo` is the network's `name`, or the file name
(TRACE.md defines `demo` as a free name). `library` stays the engine's version
string (`0.0.0` today); an optional `engine` field names the engine. `description`
from the file (the engine writes an empty string if the file has none); a neuron
with no `description` is written as `<kind> neuron`, so every neuron has one. `neurons`: `id`, `description`, `kind` (the kind field's name,
`network` written `composed`), `options` as set in the file
(`threshold`, `within`, `maxHops`; defaults are not written) and `cost` as set in the file (an
object of units, apart from `options`). A header `budget` holds the file's `budget`, when it has one. `links`: one
`{from, to}` per link, directional. `summary` MAY be `{}`. A composed neuron's
inside never appears.

### 7.2 Events

| Event | Written for |
|---|---|
| `signal` | an injection or a forward; `via` absent |
| `answer`, `quiet` | a reply (4.3) |
| `drop` | stale work, expiry, a hops refusal |
| `error` | a failure (6.1) |
| `unknown` | a failure of a started effect call whose outcome cannot be known (11.3) |
| `finished` | a trace has no unfinished work left (below) |
| `note` | process facts |
| `ground` | a deposit wrote a place (4.5): `place`, `v`, `cause`; emitted immediately after its answer's event |

`t` is whole ms since the run started (virtual in fixtures). Payloads as v1:
text `{"type":"text","text":...}` for a string, else `{"type":"json","data":...}`
(a JSON string payload is therefore text: the one lossy spot). Every event but
`note` carries the signal's fields (`id`, `from`, `to`, `trace`, `replyTo` and
`cause` when set, `hops`). A `drop` or `error` describes the signal the work was
for, plus `neuron` and `reason` (drop) or `message` (error, unknown). `note` is
`{type, t, neuron, what}`.

Additions beyond v1 as written, all optional fields or a new event type (so
v1 readers, which ignore what they do not know, are unaffected):

| Addition | Rule |
|---|---|
| `phase` (`waiting` or `running`) on `drop` | MUST on reason `replaced`; MUST NOT on `hops` or `expired` |
| `replaces` on `signal` | MUST when the signal has a key; on `signal` events only, never on `drop`, `error`, `answer` or `quiet` |
| `code` on `error` | MUST, from the 6.1 table (`budget` included) |
| `unknown` event: the fields of an `error` (`neuron`, `message`, `code`, the signal's fields and payload) | MUST, in place of the `error`, for the failures of a started effect call (6.1, 11.3); MUST NOT for any other neuron |
| `forwarded` (`false`) on `answer` | MUST when the answer of an effect call was not forwarded because its input went stale (11.2); MUST NOT otherwise |
| `note` event: `what` is `junk` (with `line`, `text`), `cancel`, `term`, `kill`, `restart` or `inject` (with `trigger`) | MUST, as 6.3 says (fixtures with `notes` compare them) |
| `note` event: `what` is `reserve` (with `units`) or `commit` (with `units`, `ms`, `basis`) | MUST for every call when the network has a `budget`; otherwise MUST NOT (10) |
| `finished` event: `trace`, `status`, and `remaining` | MUST, once per trace of the outer network, as below |
| `ground` event: `place` (the key string), `v` (the stored value after the write), `cause` (the depositing answer's id) | MUST on every deposit of a network that declares `ground` (4.5); MUST NOT otherwise; carries no signal fields |
| `budget` and `neurons[].cost` header fields | MUST when the file has them |
| `engine` header field | MAY |

**Fields of the new events.** All amounts are JSON integers (10.1).

- `reserve`: `units`, an object of unit to amount: the neuron's `cost`, as declared.
- `commit`: `units`, the amounts consumed (10.4); `ms`, the whole milliseconds the call took,
  measured by the engine from its `reserve` to the end of its program (virtual in fixtures; it is
  always a measurement, so `ms` carries no label); `basis`, which says how the `units` were
  found: `estimated` when the engine took the reservation because the real figure is unknown,
  which is every commit in version 0.1 (a neuron cannot report usage yet). A later version adds
  `measured` for amounts a neuron or the engine measured.
- `finished`: `{"type":"finished","t":T,"trace":ID,"status":S}` and, when the network has a
  `budget`, `"remaining"`: what is left of the trace's pool in every unit of the budget. It is
  written once for every trace of the outer network, with a budget or without, when the trace
  has no unfinished work: nothing running, queued or held (a held input that expires is
  reported first), and no signal still being delivered (the inside of a composed neuron counts as
  over when the composed call ends, 2.6). A trace nothing took (a source with no
  links) is finished at the instant it is injected. `status` is `exhausted` if some call of the
  trace was refused with code `budget` (10.3) at this level, else `done`; a refusal sealed inside a
  composed neuron shows as `done` outside (10.5). Without a budget it is always `done`.
  It carries no signal fields and no `id`. Observers are shown it (9.1). An `unknown` outcome (11.3) does not
  change `status`, which speaks only of the budget; the `unknown` event is in the trace. The inside of a composed
  neuron never has one (2.6).

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
{"type":"drop","t":60,"id":"#5","from":"d2","to":["h"],"trace":"#1","cause":"#4","hops":2,"neuron":"h","reason":"replaced","phase":"running","payload":{"type":"json","data":{"d":2}}}
```

(The example stops at the drop; `d2` then replies at 60 too.) The drop names `#5`, the crossing input from `d2`, which was not replaced;
`#3` was. `h` acted at 20; its call would have ended at 120. An `error` event has
the same fields in place of `reason`/`phase`: `"neuron":"h","message":"timed out
after 100 ms","code":"timeout"`.

## 8. Conformance (normative)

An engine conforms when it passes every fixture in `spec/fixtures/` that is not pending (once,
for all engines). The format of a fixture, the scripted programs, the virtual clock, the
comparison and the command-line contract (`<engine> conform <dir>`) are specified in
[CONFORMANCE.md](CONFORMANCE.md), which is part of this section. Recorded traces in
`traces/` stay as smoke tests through replay, not conformance tests. The fixtures are the
contract: one changes only together with the text it pins.

### 8.1 A fixture

A directory with a network file (and the composed files it names) and `fixture.json`:

```json
{ "fixture": 0, "description": "One line.", "network": "network.json",
  "inputs": [{ "at": 0, "source": "alerts", "payload": {"service": "gateway"}, "replaces": "gateway" }],
  "script": { "logs": { "calls": [{ "when": {"service": "gateway"}, "after": 20, "reply": {"quiet": true} }] } },
  "expect": [] }
```

- `inputs`: injections at virtual ms `at`.
- `script` replaces every program, keyed by instance path (the harness may name inside a sealed
  neuron). A `calls` entry answers one call: the first unused entry whose `when` deep-equals the
  program's input value (3.1) is used (no `when` matches anything); no match is a harness
  failure. `after` is the latency in ms from the call's start. `reply` is
  `{"answer": P, "replaces"?: K or null, "to"?: [ids]}`, `{"quiet": true}`, `{"error": "message"}`,
  `{"junk": "text"}`, `{"crash": true}` (or `{"crash": "SIGUSR1"}`: killed by that named signal), `{"hang": true}` or `{"overlong": true}`; a level 1
  neuron also has `starts` (ready latency, `maxInflight`, pong, when the process ends) and
  `onCancel`. The script stands in for the process layer, so fixtures test what the engine does
  with an outcome, not byte parsing.
- `expect`: the events (section 7) in order. `note` events are compared only if `notes` is true, and
  `finished` events only if `finished` is true.
- The runner also collects the live stream (9.1) of every run and the fixture fails if it differs from the
  events of that run; `streamFails` makes the consumer throw on every event.
- `pingEvery`: the idle ping interval of the run (the interval is the engine's, 3.2).

### 8.2 Clock

Virtual time starts at 0 and jumps to the next due moment (an `at`, an `after`, a `within`, a
`timeout`); processing takes none. The engine MUST be deterministic (6.2); the run is over as
6.2 rule 6 says. Wake-ups due at one instant run in the order they were set, and injections are
set first, in the order the fixture lists them.

### 8.3 Normalisation

Ids become `#1`, `#2`, ... by first appearance across the event list, scanning each event's
`id`, `trace`, `replyTo`, `cause` (and a note's `trigger`) in that order. Events are then
compared one by one, every field, `t` included, `via` absent equal to `"emit"`; an `error`'s
`message` is compared as a prefix, since 6.1 fixes only how a message starts.

### 8.4 Worked fixture

`s` (source) -> `a` -> `b`. Input at 0: `{"n":1}`, key `k`. `a` answers `{"n":2}`
after 10; `b` is quiet after 5. Expected (this is `spec/fixtures/routing/chain-forward-with-cause`):

```
{"type":"signal","t":0,"id":"#1","from":"s","to":["a"],"trace":"#1","hops":0,"replaces":"k","payload":{"type":"json","data":{"n":1}}}
{"type":"answer","t":10,"id":"#2","from":"a","to":["s"],"trace":"#1","replyTo":"#1","hops":1,"payload":{"type":"json","data":{"n":2}}}
{"type":"signal","t":10,"id":"#3","from":"a","to":["b"],"trace":"#1","cause":"#2","hops":2,"replaces":"k","payload":{"type":"json","data":{"n":2}}}
{"type":"quiet","t":15,"id":"#4","from":"b","to":["a"],"trace":"#1","replyTo":"#3","hops":3}
```

The fixtures cover routing, threshold, staleness (issue 17 included, and a newer signal sent elsewhere than the older one), `maxHops`, composed
neurons and sealing, every row of the 6.1 table that an engine can reach, the 6.2 order, the
end of a run, level 1 (handshake, queueing, cancel, ping, restart), observers and
the version 0.1 budget (`budget/`: reserve, commit, refusal, the pool of a trace, a composed cap, `finished`),
effects (`effect/`: a call staleness does not cut, an answer recorded and not forwarded, `unknown` after a crash, a hang or a timeout, cost committed) and
the live stream (`stream/`), `may` (`may/`: the shape, the kinds it is allowed on, a composed neuron that
covers its inside and two that do not) and a neuron named by its own file (`neuronfile/`: the same events as
the inline neuron, the network's options on the entry, the file's own options applied, a narrowed `timeout`
that fires, the fold reading the narrowed `may`, and each refusal).

### 8.5 The observer rule

A fixture MAY attach a scripted observer (one that logs, or injects into a
source at a given time). Removing it MUST NOT change the events, except those
that follow its injections (section 9). CONFORMANCE.md 6 says how it is scripted and
checked.

## 9. Observers (normative)

An **observer** is a program outside the network that reads the engine's trace
events as a live stream (9.1). It is not in the network file (no field, kind or link
mode), is not addressable, is not on any path, and cannot send along links.

- Events reach it **buffered and asynchronously** (9.1). A slow or crashed observer
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
  network file, checked before a call starts, and fail closed. The limits are
  `maxHops`, `timeout` and the budget (10).
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

### 9.1 The live stream (normative)

An engine MUST offer the events of the trace as they happen, to a consumer of the caller's choosing, in
two forms: a library option (the reference engine's is `onEvent` on `runNetwork`) and a command-line flag
that writes one JSON object per line as they happen (the reference engine's is `run --stream <path>`).

- **The stream is the trace.** It carries every event of the trace of the outer network (7.2: signals, replies,
  drops, errors, `unknown`, `finished` and notes), each once, in the order they are written (6.2), and
  nothing else: no event the trace lacks, none missing, none reordered. A consumer that has read the
  stream has read the trace. The inside of a composed neuron is not in it (2.6). The stream is complete
  when the run is: the last event, a trace's `finished` or a process note, is offered before the run returns.
- **Buffered.** An event is offered to the consumer after the engine has finished the step that wrote it,
  never inside the routing: the engine does not wait for the consumer.
- **Fails open.** A consumer that is slow, throws, rejects or closes its end never changes the run: not the
  events, their order, their times, a call or its cost. The consumer gets a copy of each event; what it does to
  the copy is its own. (An engine in the same thread as its consumer cannot isolate a consumer that blocks the thread
  for long; a consumer that must not hold the engine up reads the command-line form through a pipe.)
- **Observers get `finished`.** An observer (above) is shown, besides events with an `id`, the `finished`
  event of every trace, so it can act when a trace ends (collect its spend, reconcile, start the next).
  The scripted observer of the fixtures is shown it too (CONFORMANCE 6).
- **Pipes.** The command-line form, given a pipe, needs a reader that has it open when the run starts (else the
  stream is off and says so). It never holds the run back: the trace and results are written first, and a reader that
  does not take the rest within a short grace is cut off, with one message on stderr.

The stream is not a second record: it adds no information to the trace and is not replayed.

## 10. Budgets (normative)

A budget limits what one trace may spend. The engine, not a program, keeps the books: a
program cannot raise, hide or skip its own cost. The reasons are in
[decision 0006](decisions/0006-budgets.md).

### 10.1 Declaring

`budget` (top level) is an object of **units** to ceilings: `{"credits": 10, "calls": 4}`. A
unit is a name matching the id pattern of 2.2; the engine gives units no meaning, so
they may stand for money, model calls, seconds or anything a person counts. `cost` (a neuron option,
every kind but `source`) is an object of units to amounts: what one call of the neuron reserves
(10.3) and the most it may consume (10.4). Both are literals, and both need at least one unit.

Every amount is a **whole number from 0 to 2^53 - 1** (9007199254740991), a JSON integer: the
largest value every JSON reader holds exactly. A larger or fractional amount is an error in
`check`. There are no string amounts; a unit that needs finer steps is counted in smaller
ones (cents, not dollars). A `cost` may name only units the same file's `budget` has.

**Under a budget every neuron but a source MUST declare a `cost`**, a composed neuron included;
`check` refuses a file that has a `budget` and a non-source neuron without one (2.7). A cost of 0
in every unit is allowed: the neuron is then metered and timed (it reserves and commits nothing, and its
`commit` carries `ms`). The rule fails closed: a neuron that could spend without declaring it would
escape the pool. A network with no `budget` has no accounting at all (10.7).

### 10.2 One pool per trace

Each trace of the network has its own pool, which starts at the ceilings of `budget`. Traces
share nothing: a second trace never finds the first one's spend. A producer may **lower** a
trace's pool when it injects (4.1): an injection may carry `budget`, an object of units to
amounts, each a whole number from 0 to the file's ceiling for that unit. It may never raise a
ceiling or name a unit the file lacks; the engine refuses such an injection to the producer,
as it refuses a `replaces` key over 256 bytes. A unit the injection leaves out keeps the
file's ceiling. A forward is part of its trace and draws from the same pool.

### 10.3 Reserving

When a call with a `cost` starts, the engine **reserves** the cost from its trace's pool: it
takes every unit of the cost out of the pool, and writes a `reserve` note (6.3). A call starts
when its program is started (level 0), when its `signal` line is written (level 1: a call
queued for a slot reserves when the slot frees, not before) or when the composed neuron's
input enters its inside. A held input has not started a call and reserves nothing; the
threshold call reserves once, when it acts. A call dropped before it starts (stale while held
or queued) never reserved.

If the pool cannot cover **every** unit of the cost, nothing is reserved, the call does not
start, and the engine reports an `error` for the signal the call answers (for a threshold call,
the crossing input) with code `budget` (6.1); nothing is forwarded. Other work is unaffected.
A refusal is the call failing, not a new kind of drop. A `cost` of 0 in every unit takes
nothing from the pool but is still recorded, which is how a free neuron is metered.

Several targets of one signal draw from the pool in the order of `to`, the order of delivery
(4.2, 6.2): with 10 left and `a` and `b` costing 6 each, the first listed runs and the second is
refused. A pool is never refilled in version 0.1.


### 10.4 Committing

A call that reserved **consumes** what it reserved on every outcome: an answer, quiet, a
failure, a timeout, a cut (stale), a crash. The engine writes a `commit` note when the
program is over (6.3). The amount is the lesser of what the program claims and the reservation;
in version 0.1 a program cannot claim, so the commit is the reservation, with `basis`
`estimated`. A cost is therefore both what is held at the start and the most a call can ever
consume. Nothing is returned to the pool by a failure or a cut: the work was started and
cannot be taken back. `ms` is the engine's measurement of the call's wall time.

### 10.5 Composed neurons

A composed neuron's `cost` is reserved and committed like any other. It is also a ceiling on
its inside: each call to the composed neuron starts a new inner trace, and the pool of that trace is
built from two limits, the neuron's `cost` and the `budget` of the inner file, neither of which can
raise the other:

- With a `cost` and an inner `budget`: the pool has **the units of the `cost`**, each at the
  **lower** of the cost's amount and the inner budget's ceiling for that unit.
- A unit the cost names that the inner budget lacks is limited by the cost alone (the inner file sets
  no ceiling for it; none of its neurons can cost it, `check` refuses that).
- A unit the inner budget has that the cost lacks is **not in the pool**: the cost grants nothing in
  it, so an inner neuron costing it is always refused (`check` warns, 2.7).
- With a `cost` and no inner budget: the pool is the cost.
- With no `cost` (possible only when the outer network has no budget, 10.1): the pool is the inner
  `budget`, as it stands. With neither, the inside is not metered, as in version 0.

The inner neurons reserve from that pool as in 10.3. The inside's reservations, refusals and notes stay
inside (2.6): an inner refusal is only an inner failure (code `inner`, 6.1), and one total, the composed
neuron's `cost` (or nothing, without one), crosses the boundary. A trace's `finished` status therefore
speaks for its own level: a refusal sealed inside a composed neuron shows as `done` outside.

### 10.6 Loops

A budget does not replace `maxHops`: a cycle with no `maxHops` is still an error in `check`.
A loop whose neurons cost something ends when the pool cannot cover the next call; a free
loop ends at `maxHops`, with the pool untouched.

### 10.7 Without a budget

A network with no `budget` writes no `reserve` or `commit` note, no `remaining` and no
`budget` or `cost` in the trace header (`check` rejects a `cost` there: it names no unit). Its trace
still ends with `finished`. A composed neuron in it may still be bounded by its inner file's own `budget`
(10.5); that stays inside.

### 10.8 Not in version 0.1

Usage claims from a neuron (so that a commit can be below the reservation), amounts owed to
another party, spend per source or per time window, settlement, and anything that moves value
outside the trace. A trace explains what was spent; it does not prove it to anyone else.

## 11. Effects (normative)

Some neurons do something in the world: send a message, write a record, start a job. Their calls are not
free to cut, and a failure of one does not mean nothing happened. `effect` says so in the file, so the
engine can apply two rules that a program cannot apply for itself. The reasons are in
[decision 0007](decisions/0007-effects-and-the-live-stream.md).

### 11.1 Declaring

`effect` is the neuron option `true` (2.4), on `run`, `serve`, `http` and `human` neurons; never on a
`source` or a `network` neuron (the schema refuses both). It changes nothing else about the neuron: its
contract (3), `threshold`, `within`, `timeout` and `cost` are as they were. A neuron without it behaves exactly as
before.

### 11.2 Staleness never cuts a started call

When work for an effect neuron goes stale (5.4):

- **Waiting work** (a held input, a call queued for a level 1 slot) has not started anything: it is dropped as
  usual, reason `replaced`, phase `waiting`.
- **A started call is not cut.** No `drop` is reported, no `cancel` is sent (level 1) and no signal is sent to
  the process group (level 0), and no `cancel`, `term` or `kill` note is written. The call is marked stale and
  runs to its own end. For a threshold call, any input it was built from going stale marks it, as in 5.4.
  A call that ends at the instant its input goes stale counts as stale (6.2 rule 5).
- **Its answer is always recorded** as an `answer` event (4.3), and **its cost is committed** (10.4) as for any
  call. If the call is stale when it answers, the answer is not forwarded (4.4): the `answer` event carries
  `"forwarded": false` (7.2) and no forward follows. Quiet and failures carry no mark: nothing would have been forwarded.
  A call that was not stale answers and forwards as usual.
- **Results.** What the engine returns to its caller (the answers of neurons with no outgoing links) is not a
  forward: a stale effect answer from such a neuron still reaches it, as well as the trace.

A stale input therefore suppresses only the forward. The effect happens, the trace says it happened, and the
chain downstream is spared an answer to a question that has been replaced.

### 11.3 The `unknown` outcome

If an effect call has started and fails, the engine cannot tell whether the effect happened. For a failure of
6.1 with any code except `budget`, `start` and `neuron`, the engine reports an `unknown` event in place of the
`error`: the same fields, the same `code`, the same `message` rule, at the same moment. The codes that stay
errors are those where the call never started (`budget`, `start`) and the one where the neuron itself says it
failed (`neuron`, a level 1 `error` reply). A level 0 program that could not be started at all (no such
executable, no such working directory) never ran: that is an `error` (`exit`, 127) as for any neuron, not `unknown`.
A neuron that wants to say honestly "it did not happen" should be level 1 and reply `error`: level 0 has only its exit status,
which a crash looks the same as.

- Nothing is forwarded, as for an error, and nothing else is affected. The engine does not retry, hold or
  block anything: an `unknown` is a record, and what to do about it is decided outside the engine (11.4).
  The trace shows exactly one outcome for the call.
- **A timeout is `unknown`, and the call is killed at it.** An effect call that hangs or runs past its
  `timeout` is cancelled as any call is (3.1, 3.2: SIGTERM then SIGKILL; `cancel` at level 1), at the timeout,
  and recorded `unknown` at that instant with code `timeout`. The run must be able to end, and the record
  is honest: the call was started and its effect is not known.
- The cost of the call is committed as for any outcome (10.4), `estimated`.
- `unknown` is not a drop and not an error: a reader that counts failures must count it separately, and
  a reader that does not know the event type must not read its absence as "nothing happened" (TRACE.md).

### 11.4 Reconciling (informative)

The engine has no machinery for reconciliation. The pattern uses what the specification already has:

1. The producer puts a short id at the top level of the payload it injects (`{"id": "n-17", ...}`), so it survives the
   2000-character cut of the trace (7.2), and the effect program uses it to make the effect idempotent, so doing it twice
   is the same as doing it once. The engine never retries.
2. The file declares a `source` for reconciliation (`reconcile`) linked to a neuron that can find out
   (`lookup`: reads the record the effect writes, or asks the other system).
3. Whoever sees the `unknown` (the producer reading the stream, or an observer, 9) injects the id into `reconcile` (4.1, 9). The
   injection is a root signal of its own trace, and the answer of `lookup` is its record.

The `unknown` event carries the payload of the input, so the id can be read from the event; for a threshold call it
carries only the crossing input's payload (the others are not in the event), so a threshold effect should have its id on every
input. Three cautions: `lookup` answering "not sent" is only as good as the other system's consistency, since a timed-out
or killed call may still land later; any retry the producer makes must itself be idempotent by the id; and the engine
guarantees none of this. The demo
(`npm run demo:notify`) and the fixture `observer/an-observer-reconciles-an-unknown-effect` do this.

### 11.5 Composed neurons

A `network` neuron is never an effect, and a network file used as a composed neuron MUST NOT contain a neuron
with `effect` (`check` reports an error, 2.7). A composed call is cut as a whole, recursively and without
reports (2.6 steps 2 and 4), and sealing (2.6) keeps the outside from seeing what the inside did, so neither the
guarantee of 11.2 nor the record of 11.3 could hold across the boundary. An effect belongs in the outer file,
where the trace shows it; a composed neuron may feed it.

### 11.6 Budgets, threshold, levels

- **Budgets.** An effect call reserves at its start and commits on every outcome (10.3, 10.4): its answer,
  a stale answer, an `unknown`. A refused call is a `budget` error (never started). Its `commit` comes just before the
  `answer` or `unknown` it belongs to (6.3).
- **Threshold.** Held inputs are not calls: they are replaced and expire as in 5.2. Once the threshold is
  met the call is a started call (11.2), and any of its inputs going stale marks it.
- **Levels.** At level 0 and level 1 alike a stale input starts nothing: no signal to the process group, no
  `cancel` line. The slot of a level 1 effect call is its own until it replies or `timeout` ends it.
  `http` neurons are not run by the reference engine; the rules apply to them when an engine does. A `human` effect
  call is as any other: an answer is committed, and a `timeout` is `unknown`.
- **`finished`.** A trace is finished when nothing is running, queued or held (7.2); a running effect call
  that went stale is still running, so the trace waits for it.

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

## Appendix B. Out of version 0.1 (informative)

- Strength: a state file keyed by instance path, adjusted by the caller. Until
  then every sender counts 1. (The in-run half is built: `ground`, decision
  0012 — deposits per place, evaporation, one-link routing for replies that
  name no `to`. Still out: carry-over between runs, per-link strength, and the
  caller-side file; they wait on durable run state, issue #30 item 1.)
- Passive and active channels (open; no field reserved); link attributes.
- `http` authentication and headers; a `human` wait that survives a restart, several people and who may answer
  ([decision 0008](decisions/0008-human-neurons-on-a-page.md)); Wasm neurons;
  Windows; concurrency limits at level 0; inspecting inside
  a composed neuron; byte-level fixtures and real-process fixtures; hot reload.
- Distribution: the engine will ship as a binary inside the npm and pip
  packages (the esbuild and ruff pattern).
- `may`, later ([decision 0016](decisions/0016-the-self-describing-neuron.md)): wildcards and bracketed addresses; a
  `may` on a source that carries a program.
- The neuron file, later ([decision 0019](decisions/0019-the-neuron-file.md)): a neuron's file on another
  machine; an entry that adds `effect` to a neuron that does not claim it; versioning a neuron file apart from
  the network format; parameters (a neuron file is literals, so two uses that differ in an argument are two files).
- Confinement, later ([decision 0018](decisions/0018-confining-a-program-to-its-may.md)): confinement as the
  default; a program placed on another machine; a named host for a program that does not speak HTTP; a path
  that reaches a program through a variable named in `env`.
- Effects, later: an engine-held `unknown` that blocks a key until reconciled, a retry policy, effects
  inside a composed file (11.5).
- Budgets, later: usage claims in level 1 replies, an owed ledger, spend per source or per time
  window, settlement (10.8).

## Appendix C. How the TypeScript library differs (informative)

The library in `src/` is the first engine to be brought into line. Deliberate
differences:

- one-way links (`connect` links both ways today);
- automatic forwarding along links (callers forward today); `emit` `to` limited
  to outgoing links;
- a composed neuron is one shared inner network per instance (the demo builds a
  fresh one per call);
- a default `timeout` of 30 s (none today);
- a minimal environment (the shell adapter inherits the engine's);
- process-group kill with SIGKILL after 2 s (the adapter kills the child only);
- level 0 is JSON both ways (the shell adapter writes text raw and reads
  non-JSON as text);
- a cut (running) drop is reported when the adapter settles, not when the work goes
  stale (5.4, 6.2 rule 4): an adapter that ignores abort delays it, and one that never
  settles never reports it (and `idle()` waits on it);
- `emit` reports the drops a signal causes before that signal's own event (the abort
  runs before the notification), the reverse of 6.2 rule 2;
- `phase` on drops (`onDrop` reports one reason today; the recorder and the view add `phase` from the
  held inputs they replay);
- the threshold stdin shape applies above 1 only.

### The reference engine (`src/engine/`, `npm run engine`)

A second engine, written to this specification, reads network files. It is
separate from the library (`src/index.ts` is unchanged) and is the reference
any further engine is compared with (a Rust one was tried and stopped: decision 0005). It follows the specification, including the
6.2 event order, where the library differs (the list above). Conformance gaps:

- **Neuron kinds**: `source`, `run`, `serve`, `network` and `human`. `human` needs a desk: the command line's `run`
  serves the page of Appendix D, and the library's `runNetwork` takes a `desk` option and refuses a `human` neuron
  without one. `http` is reported by `check` as a warning and refused by `run`. The conformance runner scripts a
  person (CONFORMANCE 5.5), and `spec/fixtures/human/` holds its fixtures.
- **Fixtures** (section 8): `npm run engine -- conform spec/fixtures` runs them on a
  virtual clock with scripted programs (`src/engine/virtual.ts`, a `Host` the engine takes in
  place of real time and processes). Its other tests run real processes (`sh`, `cat`) for what
  the fixtures cannot reach: bytes, process groups, the environment.
- **Observers** (section 9): the `observer` option of `runNetwork` (a function called with each outer event, the
  `finished` event included, that returns injections; the fixtures' scripted observer is one, and the notify demo
  another), with the `inject` note. `run` returns the finished trace.
- **The live stream** (9.1): the `onEvent` option of `runNetwork` and `run --stream <path>` (a file, or a pipe). Events are queued
  and offered from a microtask, as copies; a consumer's exception or rejected promise is ignored. The buffer is
  unbounded, so a consumer that never keeps up holds the events in memory.
- **Same-instant order** (6.2 rules 1 and 5) holds because the engine is one
  synchronous event loop, not because a clock orders it; a call whose process
  ends in the same millisecond its input goes stale can go either way.
- **Notes**: `cancel`, `term`, `kill`, `restart` and `junk` are written; `inject` only for a scripted observer;
  `reserve` and `commit` with a budget.
- **A run-level result** (4.4): not written by this engine, nor by the Python one. The Rust engine writes one
  when asked (`run --result <path | ->`, [decision 0022](decisions/0022-what-a-swarm-needs.md)): each trace's
  answers, failures and drops and the totals, read off the events; with it the run exits 3 when a call ended
  in an error or an unknown.
- **A neuron's own settings** (2.4): read by this engine and the Rust one, the two that start programs. The
  Python engine starts none and ignores the file. In the Rust engine a single program placed on a peer
  (`--peers`) is started by the peer with the peer's environment, and the settings file is not sent.
- **Effects** (section 11): all of it. `unknown` is a separate event type; the `forwarded` mark is on the `answer` event.
- **A live producer** (4.1): not taken by this engine, nor by the Python one. The Rust engine takes one on
  standard input (`run --inputs -`, `rust-engine/README.md`), writing its events to the stream as they happen
  and keeping none. A file it hosts for other engines (`peer --network`) takes the same injection as an
  `inject` line on a connection, and writes the file's own events to the worker's `--stream`.
- **A limit on level 0 calls at once** (Appendix B): none in this engine, nor in the Python one. The Rust
  engine takes one for the whole engine from the command line (`run --at-once <n>`, `peer --at-once <n>`,
  [decision 0021](decisions/0021-a-limit-on-calls-at-once.md)): a level 0 call over it is queued in arrival
  order, and queued means what it means at level 1 (3.2, 5.4, 10.3, 11). A network file cannot state it.
- **A neuron named by its own file** (2.10): read when the network file is (`readNetwork`), so the engine itself
  never sees the `neuron` kind. The Python and Rust engines do the same in their loaders.
- **`may`** (2.4): checked, the fold included. This engine does not confine (2.9): a program runs with the rights
  of whoever started it, whatever its `may` says. The Python engine starts no process. The Rust engine confines
  on macOS when a run asks (`run --confine`, `rust-engine/README.md`): files by path, loopback ports directly, a
  named host through a proxy of its own, `localhost` and `127.0.0.1` taken as one, and a listen address other
  than those or `0.0.0.0` refused; on any other platform it refuses a run that asks.
- **Budgets** (section 10): all of it. A call's `ms` is read from `performance.now()` in real runs, rounded to whole ms.
- **Process start failure** (no such executable) is an `exit` error, `exit 127:`
  then the reason; the table in 6.1 has no row for it.
- **The inside of a composed neuron** is available only through the `inner`
  option of `runNetwork` (CLI `--inner <out.json>`), an operator's tool outside
  the trace (2.6); it is not specified.
- **Payloads over 2000 characters** are cut to a preview in the trace (TRACE.md),
  as the recorder does; the program still gets the whole value.
- **POSIX only**: process groups, `kill(-pid)` and `sh`-style programs; no Windows.
- **Engine defaults** (decision 0001): when a call ends the engine kills its
  program's process group; a call settles 100 ms after the process exits even if
  a child still holds stdout open; ids are numbered per scope, so outer ids do not
  reveal inside work; the CLI kills every running group on SIGINT or SIGTERM.
- **The staleness map is not pruned**: a list of finished work stays under its
  (sender, key) until that key is next replaced, the same known limit as the
  library, so a very long run with many distinct keys grows in memory.
- **`check` positions** point at the key of the offending member, or the item of
  an array, not at its value.
- **Level 1 (`serve`)** follows 3.2 and 6.1. Where the text is silent the engine chose:
  - a queued call (waiting for a slot) that goes stale is dropped as `replaced`, phase
    `waiting`, with no `cancel` note; if it belongs to a threshold call, the whole call
    is removed and one drop is written, for the crossing input;
  - a hung process (3.2) is killed with `term`, then `kill` after 2 s if anything of its
    group remains, whether or not its leader has exited, and restarted; the calls still
    running fail with `crash`. The run is not over until a cancelled call's slot is free,
    so the trace records the kill. A serve process is noticed as down when it exits (after a
    100 ms grace, so its last lines are read) or closes stdout, so a helper that inherited
    stdout does not hide a crash;
  - the failed-start rule, the over-long line, junk and cancelled calls, and the `restart` note are
    specified in 3.2 and 6.3 (decision 0001 and 0004 give the reasons);
  - before `ready`, any first line other than a valid `ready` (junk, a wrong `v`, a bad
    `maxInflight`) is a failed start; the engine does not restart a neuron whose first start
    fails, it refuses to start. After a later start fails, backoff and the limit of 5 apply;
  - `ping` is sent every 10 s while nothing is running (the text gives no interval;
    `runNetwork`'s `pingEvery` option changes it); the time to answer is in 3.2;
  - the `error` message of a `crash` is `process failed`, then the first 500 bytes of stderr;
  - the engine's clock starts when every serve neuron is ready, so `at` in an inputs file
    does not include start-up;
  - at the end of the run stdin is closed and a process that has not exited is sent SIGTERM
    after 2 s and SIGKILL 2 s later, silently; the run returns after the processes are gone;
  - a `term` or `kill` note for a process that dies on its own is written only if part of its
    process group is still alive.
- **Measured** on one machine (Apple M3, macOS, Python 3.14, an idle machine; not a benchmark, not asserted
  by any test): per signal, a level 0 Python neuron costs a process start, median 19.7 ms (p95 21.6 ms, 40
  runs of `hypothesis.py`); a level 1 neuron costs a round trip over the pipe, median 0.04 ms (p95 0.07 ms,
  300 calls of `hypothesis_serve.py` with a 0 ms model call, one thread per call included). Against the alert
  storm's detector budget of about 150 ms, a level 0 Python detector spends about 13% of it on start-up, a
  level 1 one under 0.1%. A level 1 neuron pays its start once, at engine start (about the same 20 ms). The
  engine adds routing on top of both.

## Appendix D. A page for `human` neurons (informative)

What the reference engine does for `human` neurons (3.4), written down so another engine's page can show the same
thing ([decision 0008](decisions/0008-human-neurons-on-a-page.md)). None of it is normative: 3.4 leaves the
interface to the engine.

- **One page per run**, opened at the first question, on 127.0.0.1 only, behind a random token; its address goes to
  stderr (stdout carries the answers). It closes when the run ends.
- **Each call is an item** showing the neuron, its `description` and the input.
- **Three answers.** *Approve* replies with the proposed text: the input's `proposal` field if it is a string, else the
  input itself if it is a string, else the input as JSON text. *Edit* replies with the person's text (never empty).
  *No* replies empty, which is quiet.
- **The `draft`/`proposal` convention.** An input that is an object with string fields `draft` and `proposal` is shown
  as the draft beside a word diff of the proposal against it. Any other input is shown as text or as JSON. A neuron
  that sends something else is not wrong; it is just shown plainly.
- **`title` and `notes`.** With the pair, a string field `title` heads the item (what the question is about, such as
  a passage's id) and `notes`, an array of strings, is listed under the diff (what the person should know before
  answering, such as the checks the proposal passed). They are shown, never sent: Approve still sends `proposal`.
  Other fields are not shown.
- **Withdrawn.** A call cancelled by staleness (5.4), `timeout` or the end of the run takes its item down: the page
  says it is no longer waiting, and an answer to it is refused.
- **A table of people (the reference engine, [decision 0010](decisions/0010-the-desk-at-a-table.md)).** A run may be
  given grants: the people who may answer, each with their own address, the roles they hold and the human neurons
  whose items they may answer; an ask's `answerable_by` field, a role or a list of roles, narrows an item further.
  The page shows each person what waits on them and on the others, and the answers taken so far, with who gave them.
  An input with an `options` array of `{id, label, text}` is shown as its title, notes and one button per option (not
  as JSON); choosing one replies with its `text`, else its `id`. A one-line *why* may ride on any answer, for the record only: it is never sent.
- **Nothing is fetched from elsewhere.** The page loads its fonts from its own origin and runs one inline script under
  a nonce; every value from an input is shown as text, never parsed as HTML.


## Appendix E. Where each rule lives (informative)

A map, not a rule: nothing here changes what an engine does, and no fixture pins it. It marks each
runtime rule of sections 2.6 and 3 to 11 by what must be known to apply it, so that the work of
[decision 0016](decisions/0016-the-self-describing-neuron.md) (the engine as the wrapper of one
neuron) can be read off the text. A **wrapper** is whatever applies the rules for one neuron; today
one engine is the wrapper of every neuron at once.

| Mark | Meaning |
|---|---|
| **local** | One neuron's wrapper can apply it from its own declaration, the inputs delivered to it, its program and its outgoing links. |
| **link** | The two ends of a link must exchange something besides the signal. |
| **global** | It needs state shared by neurons that no link joins, or a view of the whole network. |
| **static** | It reads files, not a run (`check`, 2.7). Not a runtime rule; listed where a section mixes the two. |

A composed neuron's wrapper contains its inside (2.6), so "global to the inside" is local to that
wrapper: the same four global things below recur at every boundary, one level down.

### E.1 The map

| Section | Rule | Mark | Why |
|---|---|---|---|
| 2.6 | One shared inner network per instance; steps 1, 2, 4, 5, 7; sealing | local | The composed neuron's wrapper holds its inside. |
| 2.6 | Step 3: the inside has nothing running, queued or held for the call | global (inside) | The end of a call's inner work, G2 below, one level down. |
| 2.6 | Step 6: `cost` and the inner `budget` bound the inside (10.5) | local | The boundary owns the inner trace's pool. |
| 2.4 | `may`: the shape, and a composed neuron covering its inside | static | |
| 2.10 | A neuron named by its own file; narrowing | static | Read once, before anything runs; afterwards it is an ordinary neuron. |
| 2.9 | Confinement: the baseline, paths, connections, a proxy per neuron | local | The wrapper starts the program, and holds it. |
| 2.9 | A confined run is refused whole when one program cannot be held | global | One neuron's `may` decides for the network, as G6. |
| 3.1 | Level 0: input, output, failures, cancel, stderr | local | Wrapper and program only. |
| 3.2 | Level 1: framing, messages, `re`, slots, cancel, `timeout`, ping, restart, junk | local | Wrapper and program only. |
| 3.2 | Readiness folds outward: a network is ready when every neuron it contains is | local | Each container decides from what it contains; the outermost one not being ready is the run refused (G6, answered by decision 0017). |
| 3.2 | Reply `to`: each id an outgoing link | local | The neuron's own links. |
| 3.3, 3.4 | `http`, `human` | local | Wrapper and endpoint, wrapper and person. Appendix D's one page per run is shared by every `human` neuron (informative, G7). |
| 4.1 | Injection: a root signal at a source, its `to`, its key | local | The source's wrapper. |
| 4.1 | A new id, and a `trace` that is that id | global | Ids must not collide across the network (G5). |
| 4.1 | An injection may lower the trace's budget | global | The pool (G1). |
| 4.2 | Delivery to each neuron in `to`; each is independent work | local | The sender's links. The order of `to` matters to others only through G1 and G3. |
| 4.3 | Reply to the sender | local | Carried by the input: its `from`, `id`, `trace`, `hops`. |
| 4.4 | Forward along outgoing links; key set, cleared or inherited; two hops per neuron | local | The neuron's links and the input's fields. |
| 4.5 | The ground: a value per place, deposits, evaporation | global | Any reply may write any place, and a draw at one neuron reads its targets' places (G4). |
| 4.5 | One generator per engine, one draw per omitted-`to` reply | global | The sequence of draws follows the order of every neuron's replies (G4, G3). |
| 5.1 | Order at a delivery: hops, hold, run | local | |
| 5.2 | Threshold: groups by (neuron, trace), distinct senders, `within` | local | The receiver's own inputs; the sender is on the signal. |
| 5.3 | `maxHops` | local | `hops` is on the signal. |
| 5.4 | Staleness, when the newer signal reaches the same target | local | The target sees the newer (sender, key) input and cuts its own older work. |
| 5.4 | Staleness, when the newer signal does not reach a target that holds older work (a reply's `to` named a subset, or the ground drew another link) | link | The sender tells that target with a cut: a key reaches every outgoing link of its sender (5.4, decision 0017). |
| 5.4 | A stale answer is discarded; an effect call is not cut; a running threshold call is cut by any of its inputs | local | The receiver's own work. |
| 6.1 | Every failure row but the three below | local | Wrapper and program. |
| 6.1 | `budget` | global | The pool (G1). |
| 6.1 | `inner` | global (inside) | 2.6 step 3. |
| 6.1 | `start`: an input to a failed neuron | local | The neuron's own state. |
| 6.2 | Rule 1: work due at one instant runs in the order it was scheduled | global | One scheduler and one clock across neurons (G3). |
| 6.2 | Rule 2: a signal's event, then the drops it caused, then each target's delivery effects in `to` order | global | It orders what several wrappers report in one list (G3). The order within one target is local. |
| 6.2 | Rules 3, 4, 5, 7: reply before forward; a running drop when the work goes stale; a call ending as its input goes stale is stale; an outcome at the timeout instant goes first | local | One wrapper's own events on its own clock. |
| 6.2 | Rule 6: when a run is over | global | Nothing open anywhere (G2). |
| 6.2 | Rule 8: where `finished` goes | global | G2 and G3. |
| 6.3 | `cancel`, `term`, `kill`, `restart`, `junk` notes | local | Facts about the wrapper's own process. |
| 6.3 | `reserve`, `commit` notes | local | Written by the call's wrapper. The reservation itself is G1. |
| 6.3 | `inject` note | global | An observer's act (G8). |
| 7.1 | The header: every neuron and link, one `demo`, one `engine` | global | A description of the whole network. |
| 7.2 | What each event carries; the payload cut; `phase`, `code`, `forwarded` | local | One wrapper's fact. |
| 7.2 | One event list; `t` in ms since the run started | global | One list and one clock (G3). |
| 7.2 | `finished`, its `status` and `remaining` | global | G2, and for `remaining` G1. |
| 8 | The virtual clock, the one compared list, normalisation by first appearance | global | Conformance is written for one list (G3). It stays the test of wrappers that share a host. |
| 9 | An observer reads the events and may inject into a source | global | A view of the whole network from outside it (G8). The injection itself is 4.1, local. |
| 9.1 | The stream is the trace | global | One stream (G3). |
| 10.1 | `cost` on a neuron | local | |
| 10.1 | `budget` on the file; every neuron under it declares a `cost` | static | |
| 10.2 | One pool per trace; a forward draws from its trace's pool | global | G1. |
| 10.3 | Reserving; a refusal; targets of one signal draw in `to` order | global | G1; the draw order is also G3. |
| 10.3 | A held input reserves nothing; a queued call reserves when its slot frees | local | When the wrapper asks. |
| 10.4 | Committing: the reservation on every outcome, `ms`, `basis` | local | In version 0.1 a commit returns nothing to the pool, so it writes nothing shared. A usage claim below the reservation (10.8) would make it G1. |
| 10.5 | The pool of a composed call's inner trace | local | Held at the boundary: the composed neuron's wrapper is the owner. |
| 10.6 | A loop ends at the pool or at `maxHops` | global, local | G1; 5.3. |
| 11.1 to 11.3 | An effect call is not cut; its answer is recorded; `unknown` | local | The wrapper's own call. |
| 11.4 | Reconciling | global | A producer or an observer, outside (G8). |
| 11.5 | No `effect` inside a composed file | static | |
| 11.6 | A trace waits for a running stale effect call | global | G2. |

### E.2 The global things

Everything marked global above is one of these.

| | The thing | Where |
|---|---|---|
| G1 | The budget pool of a trace | 4.1, 6.1 `budget`, 7.2 `remaining`, 10.2, 10.3, 10.6 |
| G2 | The end: of a run, of a trace, of a composed call's inside | 2.6 step 3, 6.1 `inner`, 6.2 rules 6 and 8, 7.2 `finished`, 11.6 |
| G3 | One list on one clock | 6.2 rules 1 and 2, 7.2 `t`, 8, 9.1, the order of draws in 4.5 and 10.3 |
| G4 | The ground | 4.5 |
| G5 | Ids that do not collide | 4.1, 7 |
| G6 | The start: one `serve` neuron that is not ready refuses the whole run. Answered: readiness folds outward, so each container decides for itself | 3.2 |
| G7 | The desk: one page for every `human` neuron of a run | Appendix D (informative) |
| G8 | Observers: a reader of the whole stream | 9, 9.1, 11.4 |

And one thing a link must carry besides the signal: the cut of 5.4, to a target the newer signal
never reaches.

Decision 0016 answers G1 to G3 and leaves G4 to a record of its own. G5 to G8 and the link cut were
found by writing this map; decision 0017 answers the cut and G6 and parks G5, G7 and G8.
