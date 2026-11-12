# Decision record 0022: what a swarm needs (2026-11-12)

**Status: decided 2026-11-12, every recommendation accepted. Built: S1, `--result`; S4, `args`. Not built: S2, S3, S5.** A real job was put
to the engine: give each of 21 business types a private template repository, one neuron doing one
type and another handing out the list. It ran (the branch `feat/template-swarm`, kept unmerged), and
it showed six things missing between the contract and a swarm. One of them, a limit on calls at
once, is [decision 0021](0021-a-limit-on-calls-at-once.md)'s `maxInflight` and is being built as
decided there. This record decides the other five. Nothing here changes the SPEC, the schema or a
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

Then the template job again, as one file: `dispatch` with `each`, the one-type network as a composed
effect neuron with `maxInflight` and `retry`, `args` for the dry run, `--result` for the answer.

**One more criterion for that job, from Kevin (2026-11-12):** its neurons must not depend on the `gh`
command line. They call GitHub's API themselves, with what they need (the token, the owner) read from
`.env` settings that belong to the neuron. That is a change to the example's programs, not to the
contract: a program may read a file its `may` lets it read. Whether an engine should load such a file
for a neuron's `env` names is not decided here.

## Decided (2026-11-12)

Kevin accepted every recommendation, and extending the field budget.

1. **`each`** as written, with no join in this step.
2. **Effects inside a composed file**, the entry stating `effect`.
3. **`--result`**, opt-in, exit 3 on any error or unknown.
4. **`retry` on an effect covers `unknown`**: the file says the program is idempotent.
5. **`args` on the entry**, reversing 0019 on that one line.
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
