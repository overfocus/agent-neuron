# Decision record 0019: the neuron file (2026-11-11)

**One sentence reversed (2026-11-12):** [decision 0022](0022-what-a-swarm-needs.md), point 4, reverses this
record's "two uses that differ in a command argument are two files": an entry that names a `run` or
`serve` neuron's file may add `args` to its command. The rest of this record stands as written.

**Status: decided (Kevin, 2026-11-11: every recommendation), nothing built.** This is S3 of
[decision 0016](0016-the-self-describing-neuron.md), which decided that a neuron describes itself
in a file of its own and that the file which uses it may narrow it and never widen it, and left
open how a file names such a neuron. This record answers that, and says which options belong to
the neuron and which to the network. Nothing here changes the SPEC, the schema or a fixture until
its contract commit.

## The problem

Today a leaf neuron is written inline in the network file that uses it: its command, its `may`,
its `timeout` and its `threshold` sit side by side in one entry. Two things follow.

- **A neuron cannot travel.** To use the same program in a second network, its entry is copied,
  and the copies drift. What the program is allowed to touch is restated by whoever uses it,
  which is the wrong author: the person who wrote the program knows what it needs.
- **The entry mixes two kinds of fact.** Some are true of the program wherever it runs (its
  command, what it may touch, that it is an effect). Others are true only of this network (how
  many senders it waits for, what a call costs in this network's units). Nothing in the file says
  which is which, so nothing can say what a user of the neuron may change.

## The decision

### 1. A new kind, `neuron`, names a neuron's own file

`{ "id": "notify", "neuron": "tools/notify.neuron.json" }`. The path is relative to the declaring
file, as a `network` path is. `neuron` is a seventh kind field beside `source`, `run`, `serve`,
`http`, `human` and `network`.

**It is not a boundary.** A neuron named this way is the same neuron it would be written inline:
no sealing, no new trace, no instance. The engine reads the file, applies the using entry on top,
and from then on there is one neuron with one description. A run of a network that names a
neuron's file reports exactly what a run of the same network with that neuron written inline
reports, event for event. That is the gate.

Two alternatives were weighed and are refused:

- **A leaf as a network of one**, named by the `network` kind that exists. It costs no field, but
  it is not the same neuron: a composed call has its own trace and its own failures (`inner`, not
  `exit` or `timeout`), cannot carry `threshold` or `effect`, and cannot route its reply to a
  subset of the outer links. The gate above would fail.
- **Widening `network` to take either kind of file.** One spelling, but the kind would no longer
  say what the neuron is: a boundary or not.

### 2. The neuron file

One JSON object, closed, with its own schema (`spec/neuron.schema.json`):

`version` (0), `description`, exactly one of `run`, `serve`, `http` or `human`, and the options
that are the neuron's own (point 3). It has no `id` (the file that uses it names it), no `at`,
and it is never a `source`, a `network` or another `neuron`: a neuron whose inside is other
neurons already has its file, the network file.

A command's relative paths and the neuron's `cwd` are relative to **the neuron file's
directory**, not the using file's. A neuron is then a directory that can be moved: its file, its
program, and whatever its `may` names beside them.

### 3. What is the neuron's, and what is the network's

| Option | Whose | In the neuron file | On the using entry |
|---|---|---|---|
| the kind and its command or URL | the neuron's | yes | never |
| `cwd`, `effect` | the neuron's | yes | never |
| `may`, `env`, `timeout` | the neuron's | yes | to narrow only (point 4) |
| `description` | either | yes | yes; the using entry's wins |
| `id`, `at` | the network's | never | yes |
| `threshold`, `within`, `maxHops`, `cost` | the network's | never | yes |

The test for the split: does the fact hold wherever the program runs, or only in this network? A
neuron file cannot know how many senders link to it, how deep the loops around it go, or what
units this network's budget counts in. A using file cannot know what the program needs to read.

An option on the wrong side is an error in `check`.

### 4. Narrowing

The using entry may restate three of the neuron's options, each only to narrow:

- `timeout`: at most the neuron file's (its default, 30000, when the file sets none);
- `env`: a subset of the neuron file's names;
- `may`: for each member, a subset of the neuron file's entries, compared as written.

An option the using entry leaves out stands as the neuron file wrote it. One that widens is an
error in `check`, and the engine refuses to start. This is the narrowing refusal S1 deferred.

The fold of 2.7 is unchanged and reads the result: a composed neuron's `may` must cover what its
neurons may touch after their own narrowing. A composed neuron's own `may` still cannot be
narrowed by the file that uses it, because the fold forbids going below its inside.

### 5. Inline neurons stay

A neuron written inline is as valid as before, and every existing file and fixture stands. The
neuron file is another way to write the same thing, for a neuron worth carrying between networks.

## What it costs

- **One kind field** (`neuron`), the seventh. The option budget is untouched.
- **A second closed schema**, and a second file format every engine reads and checks.
- **Every engine's loader** resolves a neuron's file before anything else sees the neuron, so the
  rest of the engine is unchanged. Three engines.
- **SPEC 2.2 to 2.4 and 2.7**, a fixture area, and `check` rules: a missing or invalid neuron file,
  an option on the wrong side, a widening.
- **The trace says nothing of it.** A neuron named by file appears as its real kind (`run`, say),
  as the gate requires; a reader of a trace cannot tell it was not inline.
- **The journal on `feat/durable-state`** identifies a run by a digest of its file tree. When it
  merges, the digest must take in neuron files, or a changed neuron goes unnoticed.

## Not decided here

- **A neuron file for a neuron on another machine** (a path is local; an address is federation's,
  0016 point 3).
- **Whether `effect` may be added by the using entry.** It is refused here; marking a neuron more
  careful than it claims is harmless, and can be allowed later.
- **Versioning a neuron file** apart from the network format, and any registry or fetching of them.
- **Parameters.** A neuron file is literals; two uses that differ in a command argument are two
  files.

## The build order, with each gate

- **The contract commit and the engines together** (a fixture cannot pass before an engine reads
  the kind): SPEC text, both schemas, a fixture area, the TypeScript, Python and Rust loaders and
  checks. Gate: a fixture whose network names a neuron's file with the expected events of the
  same network written inline; a narrowed `timeout` that fires at the narrowed time; each refusal;
  all three engines pass; conformance otherwise unchanged.
- **Confinement reads the result** (Rust): a narrowed `may` is what the sandbox holds. Gate: a
  real-process test beside `tests/confine.rs`.

## The answers (Kevin, 2026-11-11)

Every recommendation is accepted: the new kind, the split of point 3, paths relative to the neuron
file's directory, narrowing of `timeout`, `env` and `may` only, and the using entry's `description`.

1. **A new kind, `neuron`** (recommended), or a leaf as a network of one, or `network` widened.
2. **The split of point 3** (recommended as written), in particular that `threshold`, `maxHops`
   and `cost` are the network's and may not appear in a neuron file.
3. **Paths relative to the neuron file's directory** (recommended), or to the using file's as an
   inline neuron's are. The second keeps one rule; the first is what lets a neuron move.
4. **Only `timeout`, `env` and `may` may be narrowed** (recommended), or nothing may be restated
   at all and a user who wants less writes a second neuron file.
5. **`description` on the using entry wins** (recommended), or the neuron file's always stands.

## Where it stands

Written and accepted 2026-11-11. **Built** on `feat/s3-neuron-file` (2026-11-11), the contract and
the three engines in one commit: SPEC 2.10 (and 2.2, 2.3, 2.7, 2.8), `spec/neuron.schema.json`, the
`neuron` branch of the network schema, and 14 fixtures in `spec/fixtures/neuronfile/`. Each engine
resolves a neuron's file in its loader, so nothing after the loader sees the kind. All three pass
the 14, and each of the eight refusals was checked to fire on its own rule in each engine.

The confinement gate is met by two real-process tests in `rust-engine/tests/confine.rs`: a neuron
named by file runs in its file's directory with its `may` read from there, and a `may` the entry
narrowed to nothing is the one the sandbox holds.

Three things the build settled, each **accepted (Kevin, 2026-11-11)**:

- **An option is allowed in a neuron file only where it is on an inline neuron of that kind**: a
  `human` neuron's file cannot carry `env`, and an entry cannot narrow the `may` of an `http` neuron.
- **A neuron's working directory is written whole once its file is read**, so an engine's message
  about a missing working directory names a full path where an inline neuron's names the path as
  written. `check` finds a missing one before a run.
- **A failed entry is reported at the entry**, naming the file: an error inside a neuron file has
  the line and column of the `neuron` key that names it, not a position inside the file.

Not done: a demo and its row in DEMOS.md; `describe_network` shows the neuron as its real kind and
does not say it came from a file; the journal's tree digest on `feat/durable-state` (named above).
