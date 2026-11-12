# Decision record 0022: what a swarm needs (2026-11-12)

**Status: decided 2026-11-12, every recommendation accepted. Built: S1 (`--result`), S2 (`each`), S4 (`args`), S6 (a neuron's own settings). Not built: S3, S5.** A real job was put
to the engine: give each of 21 business types a private template repository, one neuron doing one
type and another handing out the list. It ran (the branch `feat/template-swarm`, kept unmerged), and
it showed six things missing between the contract and a swarm. One of them, a limit on calls at
once, is [decision 0021](0021-a-limit-on-calls-at-once.md)'s `maxInflight` and is being built as
decided there. This record decides the other five, and a sixth that came up while they were being built. Nothing here changes the SPEC, the schema or a
fixture until each step's own contract commit.

## What the job showed

The network that did it is `examples/templates/` on that branch: `plan`, `create` (the effect, the
`gh` command line) and `verify`, each named by its own file ([0019](0019-the-neuron-file.md)), and a
`dispatch` neuron in front.

| What was wanted | What the contract gave | What was done instead |
|---|---|---|
| One answer, a list, becomes one call per item | One answer is one forward with one payload (4.4) | `dispatch` links to itself: it answers the first type and the rest, and the loop hands out one a turn, sized by `maxHops` |
| The one-type network as a neuron of the all-types network | A composed file may not contain an effect (11.5) | The three neurons were named again, one by one, in the second file |
| At most a few `create` calls at once | Only `--at-once`, which also makes the dispatcher wait | `--at-once 4` (0021 L2 is the answer) |
| To know from the run whether it worked | A run exits 0 whatever its calls did; the answers are in the trace (4.4) | A sample run in which every `create` failed exited 0 and printed nothing |
| A failed call tried again | The engine never retries (11.3, 11.4) | Run it again by hand; safe only because `create` is idempotent |
| One program used two ways (dry, real) | A neuron file is literals; two uses that differ in an argument are two files (0019) | An environment variable, passed by name |

## The proposal

### 1. Fan-out: `each`, an option

`"each": true` on a `run`, `serve`, `http` or `network` neuron. Its answer MUST then be a JSON
array, and the engine emits **one forward per item**, in the array's order, each with that item as
its payload, to the links the reply would have gone to (4.4). An empty array forwards nothing. An
answer that is not an array is a failure of the call (6.1, code `output`).

- **One answer, one reply.** The `answer` event is one, with the array as its payload; what changes
  is the forward. Each forward is a signal of its own, on the same trace, with the answer as its
  cause, so the pool (10.2) and `maxHops` (5.3) apply as they do to any forward.
- **The items of one answer are one emission for staleness** (5.4): none makes another stale. A later
  answer of the same sender on the same key makes every one of them stale, and its own items replace
  them together.
- **A threshold counts senders** (5.2), so several items from one sender regroup at a threshold
  neuron as any repeated sender does. Fan-out is for work that is done per item, not for a quorum.
- **Not on `human`** (a person answers one thing) and not on a `source` (a producer that wants
  several signals injects several).
- It says how neurons connect, which is the field budget's test (2.8).

**Not in this step: a join.** Nothing waits for "every item of that answer is done". At the end of a
run the result of point 3 gathers them; a join inside a network is its own decision, and it needs a
name for the group that this step deliberately does not invent.

### 2. Reuse: an effect inside a composed file

A network file used as a composed neuron MAY contain neurons with `effect`. The `network` entry that
names it MUST then state `"effect": true`, as its `may` must cover what its inside declares (2.4):
`check` reports an entry that leaves it out as an error, and one that states it over a file with no
effect as an error too. Decision 0007 row 11 forbade this and said it could be relaxed later.

- **A composed effect call is never cut once an effect inside it has started** (11.2 at the
  boundary). Before that it is cut as any composed call is. After it, staleness marks the call and
  suppresses its forward; the inside runs to its end.
- **An inner `unknown` is the composed neuron's `unknown`**, code `inner`, in place of the error of
  2.6 step 3. Sealing stands: the outside learns that an effect inside may have happened, and not
  which one.
- **A timeout** of a composed effect call ends it as today, and is `unknown` if an inner effect had
  started, an error if none had.
- **The first reply of `exit` still answers the call**, but remaining inner work is cancelled (2.6
  step 2) only where it is not a started effect; those run to their end and keep the trace open.

### 3. A run-level result: `--result <path | ->`, an engine flag

At the end of a run the engine writes one JSON object: for each trace, in order of injection, its
source, its `replaces` key, the answers of the neurons with no outgoing links (the neuron and the
payload), and every `error`, `unknown` and `drop` (the neuron, the code or reason, the message); and
the totals. With the flag, the run exits **3** when any call ended in an error or an unknown.
Without the flag nothing changes: no file, and the exit status of today.

- It is a reading of the trace and adds nothing to it: an engine surface like `--trace`, not a field
  of the network file.
- `-` writes it to standard output as the last line, after the answers the run already prints.
- A live run (0020) keeps nothing, so `--result` there holds the totals only.
- Exit 1 and 2 keep their meaning (refused, usage).

### 4. Parameters: `args`, on an entry that names a neuron's file

`{ "id": "create", "neuron": "create.neuron.json", "args": ["--apply"] }`: an array of literal
strings **appended** to the file's command. Only on a `neuron` entry whose file is a `run` or
`serve` neuron. They are literals as every value is (2): no interpolation, no reference to the
payload.

- This reverses one line of 0019 ("two uses that differ in a command argument are two files"). The
  job showed the cost of that line: a second file that differs in one word and must be kept in step.
- The split of 2.10 stands otherwise. The command is the neuron's; what this network adds to it is
  the network's. `args` can add and never remove or replace.
- An entry field, not an option: it changes no rule an engine applies, only the command it starts.
- A value known only when the run starts stays what it is today: an `env` name.

### 5. Retry: `retry`, an option

`"retry": { "times": 2, "wait": 1000 }` on a `run`, `serve`, `http` or `network` neuron. When a call
fails, the engine starts it again with the same input after `wait` ms, at most `times` more times.
`times` is at least 1; `wait` defaults to 0.

- **Each attempt is a call**: it reserves its cost when it starts (10.3) and commits when it ends,
  has its own `timeout`, and takes a place under `maxInflight` and `--at-once` like any call. While
  it waits it is waiting work: dropped if it goes stale (phase `waiting`), and it keeps its trace
  open.
- **Each failed attempt is reported** as the `error` it was, and a note says a retry was arranged.
  Only the last attempt's outcome is the call's: an answer forwards, a failure fails.
- **Never retried:** a `budget` refusal (the pool will not grow), a call that went stale, and a call
  the neuron's `maxHops` refused.
- **On an `effect` neuron `retry` covers `unknown` too**, and writing it there is the author's
  statement that the program is idempotent by an id in its payload (11.4). This changes decision
  0007 row 10 from "the engine never retries" to "the engine never retries unless the file says
  so". An effect neuron without `retry` is exactly as before. The alternative, never retrying an
  `unknown`, would leave a level 0 effect with no retry at all, since every failure of one after its
  start is `unknown` (11.3).
- It is the neuron's own (0019): a using entry may lower `times` and never raise it.

### 6. A neuron's own settings: `.env` beside the file that declares it (added 2026-11-12)

A neuron's `env` is names only (2.4), and until now every value came from the environment of the
engine that starts the program. So two neurons under one engine cannot hold different values for one
name, and a neuron that runs as a daemon, or under somebody else's engine, gets whatever that
process was started with: nothing travels with the neuron. The template job met this at once (a
token for the neuron that creates, another for the one that only reads), and its first answer was
programs that load a file themselves, which every program would then have to do.

The engine does it instead. **The settings file of a neuron is `.env` in the directory of the file
that declares its program**: the neuron's own file for an entry that names one (2.10), the network
file for a neuron written inline. For each name in the neuron's `env`, the value is the file's if the
file has the name, and the engine's environment's otherwise.

- **Only the names in `env` are passed**, as before. The file never widens what a program sees; a
  name in the file that the neuron does not list is not passed.
- **The file wins over the engine's environment.** The common convention is the other way round. Here
  the point is that a neuron behaves the same whoever starts the engine.
- **No file, or a name the file lacks: exactly as today.** A default that does nothing.
- **The format is lines of `NAME=value`.** Blank lines and lines that start with `#` are skipped; a
  value in single or double quotes loses them; there is no interpolation, no `export`, no escape. A
  line that is anything else is an error: `check` reports its file and line (never its text), and the
  engine refuses to start.
- **A value is never reported**: not in an event, a note, the trace, or a `check` message.
- **`run` and `serve` neurons**, the only kinds with an `env`. An entry that narrows `env` (2.10)
  narrows which names are filled, from the same file.
- **The engine reads the file, not the program**, so the program needs nothing in its `may` for it.
- **Where the program is started is where the file is read.** An engine that hosts a file
  (`peer --network`) reads the settings beside that file, on its own machine, and no value crosses
  the wire. A single program placed on a program worker is another matter: see S6 as built.

It is not a field: nothing in the network file or the neuron file names the settings file.

## The field budget

2.8 allows nine options. `maxInflight` is the tenth (0021). `each` and `retry` are the eleventh and
twelfth; `args` is an entry field and `--result` is not in the file. Kevin accepted extending the
budget for these (2026-11-12). The test of 2.8 is unchanged: a field says how neurons connect or how
one decides, and a decider is a program.

## What it costs

- **`--result`**: the Rust engine and its README. No SPEC rule; one informative paragraph where 4.4
  says a run's result is not a return value, and Appendix B loses "a run-level result".
- **`each`, effects in a composed file, `retry`**: a contract commit each. The schema, the SPEC
  sections named above with 2.4, 2.7, 2.8 and Appendix B and E, a fixture area, and all three
  engines.
- **`args`**: a contract commit. The schema, 2.7, 2.10, fixtures in `neuronfile/`, three engines.

## Not decided here

- **A join** for the items of one fan-out (point 1).
- **A bound on a queue** (0021 point 4); fan-out makes one easier to fill.
- **Backoff** that grows, or a retry that depends on the failure's code.
- **Parameters for a composed file**, and a value that reaches a program from the payload other than
  as its input.
- **An engine-held `unknown`** that blocks a key until reconciled (Appendix B).

## The build order, with each gate

Each step is a `feat/` branch and a merge of its own.

- **S1: `--result`.** Gate: a run with an error exits 3 with the flag and 0 without; the object
  names each trace's answers and failures; `-` puts it last on standard output; a live run gives
  totals.
- **S2: `each`.** Gate: fixtures for an array of three (order, one trace, three calls), an empty
  array, a non-array (`output`), a later answer replacing all three, a level 1 reply that names
  `to`, a composed neuron with `each`, the pool running out mid-list, and each refusal.
- **S3: effects in a composed file.** Gate: fixtures for the fold (both refusals), a call not cut
  after its inner effect started, an inner `unknown` surfacing, a timeout before and after the inner
  start, and `exit` replying while an inner effect still runs.
- **S4: `args`.** Gate: fixtures for the appended command, an entry with `args` on an inline neuron,
  on an `http` or `human` file, and with a non-literal.
- **S5: `retry`.** Gate: fixtures for an error retried to an answer, retries used up, the cost of
  each attempt, a stale call not retried, a `budget` refusal not retried, `unknown` retried on an
  effect with `retry` and not without, `wait` in virtual time, narrowing from a neuron's file, and
  each refusal.

- **S6: a neuron's own settings.** Gate, on real processes: a program sees the file's value for a
  listed name and not for an unlisted one; the file wins over the engine's environment; two neurons
  with their own files see their own values for one name; a `serve` program sees its own; no file is
  as before; a malformed line refuses the start and names the line without its text.

Then the template job again, as one file: `dispatch` with `each`, the one-type network as a composed
effect neuron with `maxInflight` and `retry`, `args` for the dry run, `--result` for the answer.

**One more criterion for that job, from Kevin (2026-11-12):** its neurons must not depend on the `gh`
command line. They call GitHub's API themselves, with what they need (the token, the owner) read from
`.env` settings that belong to the neuron: point 6.

## Decided (2026-11-12)

Kevin accepted every recommendation, and extending the field budget.

1. **`each`** as written, with no join in this step.
2. **Effects inside a composed file**, the entry stating `effect`.
3. **`--result`**, opt-in, exit 3 on any error or unknown.
4. **`retry` on an effect covers `unknown`**: the file says the program is idempotent.
5. **`args` on the entry**, reversing 0019 on that one line.
7. **A neuron's own settings** (point 6), the file winning over the engine's environment. Asked for
   by Kevin for a neuron that can stand alone as a daemon; accepted the same day.
6. **The order**: `maxInflight` (0021 L2), `--result`, `each`, effects in a composed file, `args`,
   `retry`.

## S1 as built (2026-11-12)

`run --result <path | ->` in the Rust engine (`rust-engine/README.md`, `src/result.rs`), one
informative sentence in SPEC 4.4, and Appendix B no longer lists a run-level result. No schema,
fixture or normative text changed.

| Check | Result |
|---|---|
| Two traces, one whose call exits 7, with `--result -` | exit 3; the object is the last line of standard output |
| The same run without the flag | exit 0, and the output of before |
| A run with no failure, `--result res.json` | exit 0; the file holds the answer under its trace |
| An answer of 3000 characters | whole in the result (the trace cuts it at 2000) |
| A live run with one failing injection | exit 3; `network`, `ok` and `totals`, no `traces` |

Three things the build settled.

1. **The object's shape**: `network`, `ok`, `totals` (`traces`, `answers`, `errors`, `unknowns`,
   `drops`) and `traces`, each with `trace`, `source`, `replaces` when it has one, `status`,
   `answers`, `failures` and `drops`.
2. **A drop does not fail a run.** Replaced work and a refused hop are the network doing what its
   file says; only an `error` or an `unknown` makes `ok` false.
3. **The result is folded from the events as they are reported**, beside the stream, so a run asked
   for one hears every event whether or not `--stream` was given. The engine itself is unchanged
   but for handing out the whole answers with their neurons.

## S4 as built (2026-11-12)

`args` on an entry that names a neuron's file, in the three loaders (`readNetwork` and `check` in
`src/engine/check.ts`, `_resolve` in `py/agent_neuron/check.py`, `neuron_of_file` in
`rust-engine/src/net.rs`): the entry is folded into the inline neuron with the longer command, and no
engine proper changed. The contract commit: `spec/network.schema.json` (the `neuron` entry gains
`args`, nothing else does), SPEC 2.2, 2.7, 2.8, 2.10 and Appendix B, and five fixtures in
`spec/fixtures/neuronfile/`.

| Check | Result |
|---|---|
| `args-are-appended-to-the-neurons-command`: a `run` file and a `serve` file, each entry with `args` | the events of the worked fixture (8.4), in all three engines |
| `refuses-args-on-an-inline-neuron` | refused: an unknown field |
| `refuses-args-on-an-http-neuron-file`, `refuses-args-on-a-human-neuron-file` | refused: the file has no command |
| `refuses-args-that-are-not-literals` (`${NAME}`) | refused |
| Real processes, `rust-engine/tests/args.rs`: a `run` and a `serve` program that answer with their arguments | `["own","--apply","two words","--apply"]`, and the run's output is that of the inline neuron |

Four things the build settled.

1. **A fixture cannot see an argument.** The scripted programs are keyed by instance path and nothing
   runs a command (CONFORMANCE 2, 5), and the trace header carries no command (7.1). So the fixture
   pins what it can, that the network is accepted and the run reports what the inline neuron's does,
   and the proof that the arguments arrive is outside the fixtures: the real-process tests of the Rust
   engine, and a unit test of each of the other two loaders (`test/spec.test.ts`,
   `py/tests/test_args.py`) that the neuron handed to the engine is the inline one.
2. **A string of `args` is what a string of a command is**: a literal, with no further limit. An empty
   string and a repeated one stand, and the array is not a set.
3. **On an inline neuron `args` is an unknown field**, of every kind, a source and a composed neuron
   included: the schema's own refusal, with no rule added. A neuron file that holds `args` is refused
   the same way, by its own closed schema.
4. **The appended arguments come after everything the file wrote**, with nothing put between them:
   a file whose command ends in an option that takes a value gives that value to the first of `args`.
   That is the file author's to know; no engine looks inside a command.

## S6 as built (2026-11-12)

SPEC 2.4 (normative), section 0, 2.7 and Appendix C; the Rust engine (`src/settings.rs`, the loader's
check, the spawn) and the TypeScript engine (`settingsOf` in `src/engine/check.ts`, `launch`). The
Python engine starts no programs and is unchanged. No schema or fixture changed: the conformance
host is scripted and starts nothing, so the rule is pinned by real-process tests in both engines.

| Check | Result |
|---|---|
| A listed name, an unlisted name, a quoted value, a comment | the listed ones arrive, the unlisted one does not |
| The file and the engine's environment both have a name | the file's value; a name the file lacks falls back |
| No file | as before |
| Two neurons named by files in two folders, a third `.env` beside the network | each sees its own value |
| A `serve` program | sees its own |
| A line that is not `NAME=value` | `check` and `run` refuse, naming file and line; neither the line nor a value is printed |
| The TypeScript engine: an inline neuron and one named by its file | the same values as the Rust engine |

Three things the build settled.

1. **A neuron that lists no `env` names never has the file read**, so a stray `.env` beside a
   network (a web project's, say) cannot refuse a run that asks nothing of it.
2. **A repeated name takes its last value.**
3. **A program placed on a program worker does not get the settings.** Point 6 said the driver
   sends a placed program its environment; it does not. The wire carries the names, and the worker
   fills them from its own environment. Sending values would put secrets on the wire; having the
   worker read a file means telling it which directory, and the wire has no word for that. So a
   placed program is as before, and a file hosted by `peer --network` reads the settings beside it.
   **Open, for the wire's own SPEC text.** Not tested here: the hosted file's case goes through the
   same planting code as a run, and has no test of its own.

## S2 as built (2026-11-12)

The contract and the Rust engine, on `feat/s2-each` (on top of `feat/l2-max-inflight`): `each` in both
schemas (`run`, `serve`, `http` and `network` neurons; `run`, `serve` and `http` neuron files), SPEC
2.4, 2.7, 2.8, 2.10, 3.1, 3.2, 4.4, 4.5, 5.4, 6.1, 6.2 rule 3, 11.2 and Appendix B, C and E,
CONFORMANCE 4.1, and 26 fixtures in `spec/fixtures/each/`, their expected events written by hand from
the text. The Rust engine passes all 298 fixtures, the 272 that were there untouched. **The TypeScript
and Python engines were not changed** (they were removed the same day, [decision 0023](0023-one-engine.md)): they accept the option in `check` and
do not act on it.

The gate, by fixture:

| Check | Fixture (`each/`) |
|---|---|
| an array of three: order, one trace, three calls downstream | `an-array-of-three-is-three-forwards`, `items-take-a-place-in-the-arrays-order` |
| an empty array; quiet | `an-empty-array-forwards-nothing`, `a-quiet-reply-is-quiet` |
| a non-array is `output` | `an-answer-that-is-not-an-array-is-an-output-error`, `a-non-array-from-an-effect-neuron-is-unknown` |
| a later answer on the key replaces every item | `a-later-answer-on-the-key-replaces-every-item`, `a-later-answer-drops-the-items-still-waiting` |
| a level 1 reply that names `to`; its `replaces`; `to: []` | `a-level-1-reply-that-names-to-sends-every-item-there`, `a-level-1-reply-sets-the-key-of-every-item`, `a-level-1-reply-with-an-empty-to-forwards-no-item` |
| a composed neuron with `each` | `a-composed-neuron-with-each`, `a-composed-neuron-with-each-whose-answer-is-not-an-array`, `each-inside-a-composed-neuron-and-the-first-exit-reply` |
| the pool running out mid-list | `the-pool-runs-out-in-the-middle-of-the-list` |
| a threshold counts senders | `items-from-one-sender-regroup-at-a-threshold` |
| `maxHops` on the items | `max-hops-applies-to-every-item` |
| the ground's draw | `with-select-every-item-draws-its-own-link` |
| an effect neuron with `each` | `a-stale-effect-answer-forwards-none-of-its-items` |
| no outgoing links | `with-no-outgoing-links-the-answer-is-the-array` |
| the neuron's own file | `each-in-the-neurons-own-file` |
| each refusal | the five `refuses-*`: on `human`, on a source, a value that is not `true`, on an entry that names a neuron's file, in a `human` neuron's file |

Real processes: two tests in `rust-engine/tests/real.rs`, one in `tests/http.rs` (an `http` neuron,
which no fixture reaches) and one in `tests/check.rs` (each refusal fires on its own rule).

What the build settled that point 1 left loose. Each follows what a single forward does today.
**All are for the maintainer to confirm.**

1. **`each` is the neuron's own** (0019): written in the neuron's file, never on the entry that names
   it, in the row of `effect`. It must match what the program prints, and that holds wherever the
   program runs. The other reading has a case too (one list-printing program, used whole in one
   network and fanned out in another); it would make `each` the network's, on the entry.
2. **The ground: one draw per item**, in the array's order, and none for an empty array. Each item is a
   forward of a reply that names no `to`. An `each` answer never deposits: its payload is an array.
3. **An empty array makes nothing stale.** Staleness comes from emitting a signal, and none is
   emitted, as for a level 1 reply with `to: []`. So `[]` on a key does not withdraw the items of an
   earlier answer on that key.
4. **The failure's message starts `not an array`**, and the check is on every kind's answer at the
   point where it would be reported: no `answer` event is written. On an effect neuron it is
   `unknown` (the call started), also when the call was stale. On the `exit` of a composed neuron it is
   an inner failure, so the composed call ends as 2.6 step 3 says.
5. **A level 1 reply's `to` and `replaces` hold for every item.**
6. **Event order**: the answer, then each item's signal with its drops and delivery effects before the
   next item's (6.2 rules 2 and 3). The older work of the key is dropped after the first item's signal.
   A trace is not `finished` between items.
7. **A threshold neuron may itself have `each`**; its one call's answer fans out. Not pinned by a
   fixture.
8. **Inside a composed neuron** an `each` neuron fans out inside, and the first reply of `exit` is
   still the reply: the calls of the other items are cancelled. That is 2.6 unchanged, and it is the
   place a join will be missed first.

Not done: the TypeScript and Python engines and the mutants in `test/conformance.test.ts`; a demo and
its row in DEMOS.md; the trace header does not list `each` among a neuron's `options`; the MCP
server's description of a network does not show it; `each` on a neuron placed on a peer or in a
hosted file is untested (the answer comes back through the same `call_replied`).
