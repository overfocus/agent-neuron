# Decision record 0018: confining a program to its `may` (2026-11-11)

**Status: decided (Kevin, 2026-11-11: every recommendation, "for now"), nothing built.** This is S2 of
[decision 0016](0016-the-self-describing-neuron.md), which left the mechanism, the baseline and the
edges of `may` undecided. S1 fixed the declaration (SPEC 2.4); this record proposes how the Rust
engine enforces it, from probes run on 2026-11-11. Nothing here changes the SPEC, the schema or a
fixture until its contract commit.

## What the probes found

Fourteen probes of `sandbox-exec` on the development machine (macOS 26.3, arm64), kept in a
scratch directory outside the repository. Each line below is something a probe showed.

1. **A profile that denies by default can hold a real interpreter.** With the system's own base
   profile (`system.sb`), `process-exec`, `process-fork`, metadata of any path, and read access to
   the tree the interpreter is installed in, `node` and `python3` start and run. Without the last
   two they abort or exit before running a line.
2. **Files are confined by path.** A read outside the granted trees fails with `Operation not
   permitted`; the same read inside a granted tree succeeds. Writes behave the same. The home
   directory is unreadable unless granted. A child process inherits the confinement.
3. **Connections are confined by port, not by host.** A connection to a loopback port succeeds when
   that port is granted and fails when another is. But a rule that names a host or an address is
   refused when the profile is compiled: `host must be * or localhost in network address`.
4. **Listening is confined the same way**: by port, and by `*` or `localhost` only.

Finding 3 is the one that shapes this record. `may.connect` names hosts, and the sandbox cannot
tell one host from another. From memory and not probed here: Linux's Landlock has the same limit
(its network rules name ports only).

## The decision

### 1. Confinement is asked for: `run --confine <file>`

A side file, the `--grants` and `--peers` pattern. Without the flag nothing is confined and a run
is what it is today, which is what SPEC 2.4 allows in this version. With it, every `run` and
`serve` program of the tree runs confined to its `may`, and a neuron with no `may` gets the
baseline and nothing else.

The file says what belongs to the machine and not to the house rules: where this machine keeps
its interpreters. `{"version": 0, "tools": ["/opt/tools"]}`, closed. Each entry is a tree every
confined program may read. A network file never names such a path, because it differs from one
machine to the next.

### 2. An engine that cannot confine refuses to run

Under `--confine`, the run is refused before any process starts when the platform has no
mechanism the engine knows, when a `may` entry cannot be enforced (points 5 and 6), or when the
run also has `--peers` (a placed program runs on another machine, and the wire does not carry
`may` yet). It fails closed.

### 3. macOS first, through `sandbox-exec`

It is the machine the gate runs on, and the probes are from it. Linux follows as its own step.
The cost named: Apple marks `sandbox-exec` deprecated, though it ships and works on 26.3.

### 4. The baseline: what running a command needs

Every confined program gets, whatever its `may` says:

- the system's base profile, and the right to start processes;
- metadata of any path (that a file exists, its size and dates), never its contents;
- read access to the trees named by `tools` in the `--confine` file;
- its own stdin, stdout and stderr.

Nothing else. In particular **not its working directory**: a program that reads its own script
declares `"read": ["."]`. And not a temporary directory, and not `HOME`.

The metadata grant is a leak accepted: a confined program can learn that a file exists. The
probes showed an interpreter cannot start without it.

### 5. What a path means

- A relative path is resolved against the neuron's working directory (2.4's `cwd`).
- A path that names a directory grants its whole tree.
- The path is resolved through symbolic links when the program starts, because the sandbox
  matches real paths. A path whose last part does not exist yet is granted as written under its
  resolved parent, so a program may create the file it declares.
- `write` does not include `read`. A neuron that does both declares both.

The S1 fold still compares strings as written. It stays that way: the fold is about what a
reviewer reads, and this is about what a machine enforces.

### 6. `connect` and `listen`

- **A loopback entry is enforced directly.** `localhost:P` and `127.0.0.1:P` become a grant of
  that loopback port.
- **`listen` on `0.0.0.0:P` is enforced directly** as any address on that port. Any other listen
  address is refused under `--confine` in this version: the sandbox cannot tell it from another.
- **A `connect` to any other host goes through a proxy the engine runs.** The confined program may
  reach one loopback port, the engine's own; the proxy opens a tunnel only to a `host:port` the
  neuron's `may` lists; and the program is given `HTTP_PROXY` and `HTTPS_PROXY` naming it. A
  program that ignores those variables cannot connect at all, which is the safe failure.
- **Until the proxy is built, a confined run refuses a non-loopback `connect` entry** (point 2).

The proxy is the second half of this work (S2b). It is plain TCP tunnelling and needs no TLS in
the engine, so the no-crypto reversal of 0016 is not what unblocks it.

### 7. Nothing new is reported

A denied operation is the program's own failure: it exits or errors as it would on a real
permission error, and SPEC 6.1 already has the row. No event, note or trace field is added,
which is what 2.4 says of `may`.

## What it costs

- **Every existing network file fails under `--confine` until it declares a `may`.** None of the
  examples, the demos, the office or timbre has one. That is the reason it is opt-in.
- **A `--confine` file per machine**, a third side file after grants and peers.
- **Real-process tests only**, macOS only. Fixtures start no process, so conformance cannot pin
  any of this; the TypeScript and Python engines stay check-only.
- **SPEC 2.4 gains the meaning of confinement** (the baseline, what a path means, the proxy) in
  the contract commit, and Appendix C says which engine confines on which platform.
- **The proxy** is a new listener inside the engine, std-only.

## Not decided here

- **Linux.** The mechanism, and whether its limits match macOS's.
- **A path that arrives through `env`** (the notify example's `SENT_LOG`). Such a neuron declares
  the directory, or runs unconfined; nothing better is proposed.
- **Carrying `may` to a peer**, so a placed program is confined where it runs.
- **Making confinement the default.** A question for the version 1 review.
- **Programs that do not honour proxy variables.** They cannot reach a named host. Whether that is
  acceptable for the harnesses the office wraps is found by trying them, in S2b.

## The build order, with each gate

- **S2a: files, loopback and listen.** `--confine`, the baseline, the path rules, loopback
  `connect`, `listen`; the refusals of point 2. Gate: real-process tests that a neuron with no
  `may` cannot read its own directory, write anywhere, connect or listen; that each member grants
  exactly what it names and no more; that a child process is held too; that each refusal refuses
  before a process starts. Conformance unchanged, since the flag is off by default.
- **S2b: the proxy.** Gate: a confined program reaches a listed host through the proxy and is
  refused an unlisted one; a program that ignores the proxy variables reaches nothing; the
  interpreters the examples use are tried and the result written down.
- **The contract commit**: SPEC 2.4 and Appendix C, after S2b.

## The answers (Kevin, 2026-11-11)

Every recommendation is accepted, "for now": each stands until it is revisited, and none is a
promise about version 1.

1. **Opt-in through `--confine <file>`** (recommended), or always on. Always on breaks every
   existing network file today.
2. **The baseline excludes the working directory** (recommended), or includes it read-only. The
   second is far more convenient and means every neuron can read every other neuron's script.
3. **Named hosts through an engine proxy** (recommended), or enforce the port only and treat the
   host as declared and unenforced. The second is a day's less work and makes `connect` promise
   more than it delivers.
4. **Refuse a non-loopback `connect` until the proxy exists** (recommended), or allow it
   unenforced in the meantime with a line on stderr.
5. **macOS first** (recommended), or Linux first because that is where a deployment would run.

## Where it stands

Written and accepted 2026-11-11. **S2a is built** on `feat/s2a-confine` (2026-11-11):
`run --confine <file>`, `src/confine.rs`, and `tests/confine.rs` (17 real-process tests, macOS only).
Without the flag a run is unchanged, and conformance is untouched.

Building it changed the baseline of point 4 in two places. Both are my reading, found by running
real programs, and each can be struck:

- **A confined program may signal the processes it started.** Without that it cannot stop its own
  child: a script that started a listener could not end it, and it ran on until the engine killed
  the group. Only processes in the same sandbox can be signalled.
- **A confined program may read its working directory as a directory, and no deeper.** `node` and
  `python3` ask where they are when they start, and both refused to start without this. The cost:
  the program can see the names of the files beside it. It cannot read any of them, or list a
  directory below. This narrows the answer to decision 2 below, which was to leave the working
  directory out entirely; with it out entirely, every interpreter neuron would have to declare
  `"read": ["."]`, which grants the whole tree and defeats the point.

A hand run, not a test (it needs this machine's interpreter tree): a `node` neuron with
`"read": ["neurons"]` and `HOME` named in its `env` ran, answered, and could not list the home
directory; its twin with no `may` could not open its own script and failed with an `exit` error.

Not in S2a: the proxy (S2b), a demo and its row in DEMOS.md, the MCP route (`run_network` confines
nothing and has no option to), and the SPEC text (the contract commit, after S2b).
