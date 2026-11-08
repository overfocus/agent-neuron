# agent-neuron engine, second implementation (Rust, M3)

This crate is the Rust engine of [`agent-neuron`](../), milestones M0-M3: the
M0 sans-IO conformance core, the process layer (real `run`, the `check`
command, the trace file and the live stream), real `http` neurons and the desk
of M2, and the MCP interface of M3 — a stdio JSON-RPC server speaking Model
Context Protocol's stable core subset. It is written from
[SPEC.md](../docs/SPEC.md), [CONFORMANCE.md](../docs/CONFORMANCE.md),
[spec/network.schema.json](../spec/network.schema.json), the fixtures
themselves and [TRACE.md](../docs/TRACE.md)
([conformance-python.md](../docs/conformance-python.md) is an engine's
documentation, not engine source). The TypeScript and Python engines were used
only as black boxes: `node dist/engine/cli.js conform <dir>` and probe fixtures
of my own, written into a scratch directory outside the repository.

## M0 scope (the conformance core, closed)

- `neuron-engine conform <dir> [--only <text>]`, the command-line contract of
  CONFORMANCE 8: one line per fixture, `pass`/`fail` (first difference, two
  more indented lines)/`pending`, a last summary line, exit 0/1/2.
- The engine core of SPEC 4-11 on a virtual clock with scripted programs:
  routing, threshold and staleness, `maxHops`, level 0 and level 1 (with all of
  SPEC 3.2 and 6.1), composed neurons (sealing, scopes per call, 10.5 pools),
  human neurons (the scripted person of CONFORMANCE 5.5), budgets (10), effects
  (11), observers (9), the live stream (9.1) and `finished`.
- `check`-strength validation of network files (the errors of 2.7, needed for
  the refusal fixtures); a file failing them, or a network with an `http`
  neuron, refuses to run.

## M1 — the process layer

- `neuron-engine run <file> --inputs <jsonl> [--trace <out.json>]
  [--stream <path>] [--ping <ms>]` (SPEC 9.1's command form): the producer's
  injections at their `at` ms from time zero (the instant the last serve
  program said ready, CONFORMANCE 4.2's real analogue), the answers of the
  neurons with no outgoing links on stdout, one per line. Exit 1 on a run the
  engine refused; the streams and traces are written even in that case only
  when the run had events to report.
- The trace file (`--trace <out.json>`), format v1 per
  [TRACE.md](../docs/TRACE.md): the header (`format`, `version`, `library`,
  `engine` = `agent-neuron-rust`, the `demo` name, `ids`, `neurons`, `links`)
  and the events, one value per line inside. The `run` self-checks after
  writing: the header's `ids` and the events read back from the file are the
  writer's own, and (with a stream) the stream's lines are the same events in
  the same order — P2's gate.
- The header's `ids` field, written on every trace this engine writes (the
  MCP `keep_trace` route and `run` share the one trace builder): an object
  `{ "signal": <scheme>, "trace": <scheme> }` after `engine`, the ids scheme
  of the file's events under the format's two id kinds (`signal` ids,
  `trace` ids), so the header declares the file's ids as it declares its
  neurons and links up front, while TRACE.md's own rule stands: the ids
  themselves are opaque strings, compared, never parsed. This engine writes
  `"run-numbered"` for both kinds — its ids are the run's own sequential
  decimal numbers, 1, 2, 3, ... from the one counter the whole run draws on,
  in the order signals exist, a chain's `trace` id its first signal's id;
  ids never reach a program (SPEC 11.1), and inner signals are sealed
  (SPEC 2.6), so the file's ids name nothing inside. A v1 reader ignores the
  field, so it stays optional for them (`read_trace` reads a trace without
  one); the engine's two trace routes cannot write a trace without it, and
  both self-checks say so. Interpretation, recorded while the item was open:
  docs/TRACE.md names ids only in its compatibility rules, not as a header
  field, and SPEC 7.1 lists the v1 header without one — the shape above is
  this engine's minimal reading of an optional v1 field, added (not renamed),
  set down here because the trace format's doc lives outside this crate's
  tree; the values name the family's schemes (the TypeScript recorder's ids
  are random UUIDs, the reference engine's are numbered per scope: decision
  0001).
- The live stream (`--stream <path>`): a file or a pipe fed one event per line
  as they happen, never holding the run up. A pipe with no reader is off,
  with one message on stderr (9.1), and the run is untouched by it.
- `neuron-engine check <file>` (SPEC 2.7): the closed schema's errors
  (duplicate ids, a link naming an unknown id with the nearest-id suggestion,
  a duplicated link, a link into a source, the composed-file rules —
  missing/invalid file, entry/exit pairing and existence, a source inside, an
  exit with outgoing links, effect inside a composed file), a cycle in which
  no neuron has `maxHops`, the budget rules (whole numbers 0..2^53-1, a cost
  unit the file's budget lacks, an empty `budget` or `cost`, a budget
  demanding a cost on every non-source, composed included), and the warnings
  (a cost above the file's ceiling, a composed file whose neurons cost a unit
  the declaring cost does not name — 10.5 —, an executable not findable on
  `PATH`, an `http` neuron: a warning here, a refusal to run). Exit 1 on any
  error, 0 with none, 2 for usage and for a file that cannot be read. Every
  composed file is checked, relative to the file that declares the neuron
  (2.4); a tree that loops back on itself is visited once.
- The real host (`host.rs`), std library plus `kill`, `pipe`, `read`,
  `signal` and `write` (extern "C", POSIX): one process per call in its own
  process group (`process_group(0)`), one JSON value to stdin then closed
  (never a blocking step: the write happens in its own thread), the 1 MiB cap
  on a level 0 output and on a level 1 line, one long-lived process per serve
  neuron with slots, the `re` counter, queues, pings and the restart backoff
  of SPEC 3.2 (`f` counts: not ready, a life under 1000 ms after ready, or
  killed as hung; a pong or a life of at least 1000 ms that does not end in a
  hang clears it; 5 is failure). SIGTERM to a group, SIGKILL 2 s later, with
  the `term`/`kill` notes of 6.3; the notes of a cancelled level 0 call land
  at its timeout; the engine tears itself (SIGINT/SIGTERM to the CLI) down
  through a self-pipe that kills every registered group.

### How to run everything

```sh
cargo fmt --check                                   # max_width 140 (rustfmt.toml)
cargo clippy --all-targets -- -D warnings           # zero
cargo test                                          # 86: mcp 18, real 27, check 11, desk 17, http 11, unit 2
cargo run --manifest-path rust-engine/Cargo.toml -- conform spec/fixtures   # 201, 0, 0
cargo run --manifest-path rust-engine/Cargo.toml -- conform py/probes       # 10, 0, 0
target/debug/neuron-engine run spec/examples/alerts.network.json --inputs spec/examples/alerts.inputs.jsonl --trace alert.json
target/debug/neuron-engine run spec/examples/alerts-python-serve.network.json --inputs spec/examples/alerts.inputs.jsonl
target/debug/neuron-engine run spec/examples/alerts-budget.network.json --inputs spec/examples/alerts.inputs.jsonl
target/debug/neuron-engine check <file>
neuron-engine mcp                                   # the M3 server on stdin/stdout
```

Results when M1 closed, re-run the commands above to check: `spec/fixtures`
201 passed, 0 failed, 0 pending (the M0 numbers, identical over 5 consecutive
runs, still good); `py/probes` 10 passed, 0 failed; `cargo test` 27 real + 11
check + 1 probe, 0 failed; `cargo fmt --check` clean at `max_width = 140`;
`cargo clippy --all-targets -- -D warnings` clean. The three alert smoke runs
work end to end with written traces and self-checks, and their event counts on
this machine are machine-dependent: the storm's programs are real processes,
and a scheduler that coalesces small timer sleeps (this machine: a 10 ms sleep
at ~45 ms) stretches process starts, so the count of `note term` findings
moves by a handful of events per run (313-319 for the plain alerts network
with the same 40 traces, no errors and no timeouts in any run). The
conformance suites are the deterministic gate; the smoke runs are the
end-to-end one.

## What is deliberately NOT in M1

- The `http://` and human *page* surfaces: these are M2 (the next section);
  at M1 an `http` neuron refused to run and `run` refused a file with a
  `human` neuron in it ("no desk for a person").
- Windows: the process layer is POSIX. The engine's platform list in
  `signal_name` covers darwin and linux.

## M2 — `http` neurons and the desk

### The `http` kind, real

- `src/http.rs`: a std-only HTTP/1.1 client. It POSTs the 3.1 input value
  (the bare payload, or `{"payload", "inputs"}` above 1) as
  `application/json`, reads `Content-Length`, `chunked` and EOF bodies with
  the 1 MiB cap, follows no redirect (3.3), and authenticates with nothing.
- 3.3's outcomes: 2xx with a JSON body is an answer (the reply keeps its
  whole shape in the engine's accounting; the trace cuts over 2000
  characters, TRACE.md); 204 and an empty 2xx body are quiet; a non-2xx
  status, a connection failure, or a non-JSON body is an error, code `http`;
  the failures read as one `http:` prefix whatever the errno.
- Cancel ends the socket at once (`shutdown` both ways) — staleness and
  timeout both; a reply that races its deadline loses to the deadline (
  `pump`'s tie rule: an event stamped at the wake's instant is a tie, and
  the wake armed at the call's start owns it, per 6.2 rule 7; measured on
  this machine with 1-in-4 runs losing that tie before it was fixed).
- **`https://` is refused at plant time**: no TLS in a std-only crate. This
  is an honest limitation against SPEC 3.3, not a reading of it ("the http
  neuron names 'https://…', and this engine has no TLS").
- `check` keeps its `http` warning (2.7's table; an engine cannot reach the
  network without asking for it), and `run` now RUNS it — the M2 choice,
  where Appendix C's reference engine still refuses.
- Covered by `tests/http.rs` (11): answers, quiet, errors, refusal,
  timeout/error and timeout/`unknown` for an effect, staleness cancel with
  the late reply discarded, the threshold body shape, a composed instance,
  and a no-link neuron's answer on stdout.

### The desk: `human` neurons run for real

- `src/desk.rs`: one page per run (Appendix D), bound to 127.0.0.1 only, a
  random 128-bit token (checked in constant time everywhere), exact `Host`
  and `Origin`, a nonce CSP with no outside request, every payload value
  rendered as text. The address line `desk: <url>` goes to stderr.
- The API behind the page: `GET /?token=…` (the page, its script inline and
  nonced), `GET /state` (the items in ask order with their instance paths,
  titles, notes and statuses — `waiting`, `withdrawn`, `timed-out`,
  `answered` — and the run's `over`), `POST /answer` (`approve` sends
  `proposal`, else a string input, else the input as JSON text; `edit`
  sends the person's text, an empty one refused 400; `no` sends quiet).
- What a *host* recording a run needs, which the page itself never reads:
  the ask, whole — the state's item carries the `payload` (the input value
  as the engine received it) and its `t`, the run's millisecond at the ask —
  and the answers: the desk closes with the run, so a last answer can only
  be read live, and every accepted answer lands as one `desk answer: {item,
  kind, text}` line on the run's stderr at the moment of its POST, as the
  run will carry it (an approve's text is the proposal the engine computed,
  an edit's the person's, a `no`'s empty). The page fields stay as Appendix
  D has them; these ride beside them, and the page is unchanged.
- A human neuron inside a composed instance asks on the same page, its item
  named by instance path (`box/who`); `effect` human calls behave as the
  fixtures of M0 prove (a timeout is `unknown`); withdrawn and timed-out
  items refuse answers (410) as Appendix D says.
- The M0 fixtures are unchanged: the virtual host still refuses `human`
  without its scripted person, and `conform` never serves a page.
- Covered by `tests/desk.rs` (12): the three answers with the page's rules,
  a withdrawn item, a timed-out effect, the credential and path refusals,
  the page's CSP and values-as-text, the composed instance — all driven
  through the API the browser uses, no browser needed — and the host
  surface's ask (the state's `payload` and `t`) and answer (the stderr's
  `desk answer:` line).

### The desk at a table: grants, choices and the record (decision 0010)

- `run … --grants <grants.json>`: `{"version": 0, "people": [{id, name?,
  roles?, answers}]}`, closed; every `answers` entry must be a human neuron
  of the network, or the run is refused before the desk opens. Each person
  gets a token of their own, printed as `desk person: {id, name, roles,
  url}` after the `desk:` line; the run's token stays the owner's.
- The POST checks the grants: the item's neuron must be granted and the
  ask's `answerable_by` (a role or a list), when present, must name a role
  of the person's; otherwise 403 and a `desk refused: {item, neuron, by,
  why}` line. `desk answer:` carries `by` when the run has grants, and
  `option` and `why` when given.
- `kind: "choose"` answers an ask with `options: [{id, label?, text?}]`: the
  reply is the option's `text`, else its `id`. `why` (one line, at most 500)
  rides on any answer, on the record only — never into the network.
- `/state` adds `record` (every accepted answer: item, neuron, kind, text,
  by?, option?, why?, ms), each item's `yours` and, with grants,
  `answerable_by` (the people who may answer it), plus `me` and `people`
  with each one's waiting count. The page shows the table, the option
  buttons, a why line, other people's items read-only ("waiting on …"), the
  record, and the viewer's waiting count in its title.
- Covered by `tests/desk.rs` (4 more, 16 in all).

### Gates measured when M2 closed

- `conform spec/fixtures` 201/0/0 and `conform py/probes` 10/0/0 — unchanged.
- `cargo test`: unit 1, check 11, desk 11, http 11, real 27, all green over
  four consecutive full runs; the http tests are serialized per binary
  (`.cargo/config.toml`: real ports and real deadlines are timing-sensitive,
  and one test at a time keeps them deterministic).
- `cargo fmt --check` and `clippy --all-targets -- -D warnings` clean.
- The three alert smokes still write their traces (324/316/580 events —
  counts vary run to run with the machine's load, as the M1 section notes).

## M3 — the MCP interface (`neuron-engine mcp`)

An MCP server on stdio, `src/mcp.rs`, hand-rolled on `json.rs` and the
standard library: newline-delimited JSON-RPC 2.0, one request per line, one
response per line; logs and diagnostics go to stderr only (stdout is the
protocol, nothing else writes there). It speaks the stable core subset of
Model Context Protocol and nothing beyond it: no sampling, no elicitation, no
prompts, and `conform` and the fixtures are not exposed anywhere on this
surface.

- The handshake: `initialize` answers `{"protocolVersion", "capabilities",
  "serverInfo"}` — the client's version echoed when the server speaks it
  (`2024-11-05` and `2025-06-18`), else the first stable protocol's;
  `capabilities` advertise `{tools: {listChanged: false}, resources:
  {subscribe: true, listChanged: false}}`; `serverInfo` is `{"name":
  "agent-neuron", "version": "0.0.0"}`. `notifications/initialized` closes it
  silently; a second `initialize` on the same session is refused. `ping`
  answers `{}` at any time. Protocol errors keep JSON-RPC's semantics:
  malformed JSON is `-32700` (a fresh server survives it), an unknown method
  `-32601`, bad params on the wire `-32600`/`-32602`. Tool failures that are
  the subject's fault — a check that found errors, a refused run, a timed-out
  one — are RESULTS with `isError: true`, not wire errors.
- `tools/list` carries six tools (five at M3; `describe_network` came after), each with its description and a closed
  JSON-schema `inputSchema` (`required` marked, `additionalProperties`:
  false); the schemas are built, not guessed — an unknown field of the
  arguments is refused as the schema promised. `tools/call` answers
  `content` (one text block, the compact JSON of the body, byte-identical to
  it) plus `structuredContent` (the same object); an unknown tool name is a
  protocol error, as the spec's error table has it.
- `check_network {file}` — the `check` command's whole surface (same error
  set, same wordings, positions), the file resolved as a path of the
  server's working directory: `{ok, errors, warnings}`, each diagnostic with
  its file, line, column and message. A check with errors is `isError` too:
  the subject is the thing at fault.
- `describe_network {file}` — the shape of a network file tree, for a
  client that draws the network before any traffic: the file is checked
  first (the `check_network` surface: a tree with errors is `isError` with
  `{ok: false, errors, warnings}`), then `{ok: true, warnings, network}`
  where `network` is `{file, name, description, entry?, exit?, budget?,
  neurons, links}` — `file` the name as the caller or the declaring file
  wrote it (never a resolved path), every neuron in the v1 trace's own shape
  (TRACE.md "neurons", built by the one function the trace writer uses),
  plus `effect: true` on a neuron the file declares `effect` (SPEC 11: the
  trace's neuron shape does not carry it, and a drawing needs to mark the
  calls that are never cut), and every link. A composed neuron carries its inner file the same way under
  `network`, resolved against its declaring file's directory as the runner
  resolves it; a file already open further up the tree is marked
  `{file, cycle: true}` instead of described again (`check` passes a
  composed file that composes itself, so the description has to end the loop
  itself). Static only: a run's events never name inner neurons (TRACE.md:
  a composed neuron is one neuron).
- `run_network {file, inputs, timeout_ms? = 60000, cap 600000,
  keep_trace? = false}` — a synchronous run through the engine's real route:
  `{status: "done"|"refused"|"failed", answers, summary, events}` where the
  summary says the whole counts and `events` carries a run's events whole to
  300, else the first 299 plus a `{"truncated": <total>}` marker. A refused
  run (checking included) is `refused` with the engine's refusal wording; the
  deadline reached is `failed` with the reason `timed out at N ms`, and the
  run is torn down by the engine's own end-of-run discipline (stdin closed,
  then the SIGTERM/SIGKILL of 3.2 — the deadline inside the engine loop, so
  the teardown is the one every real run ends with, not a kill from outside).
  `keep_trace` writes a v1 trace (with the P2 self-check, as `run` writes
  its traces) into a temp directory the server manages (removed at the
  server's own exit; a server killed by a signal leaves it to the OS's temp
  cleaning), returned as `{trace_path}` with its trace:// resource. The
  events of an events-stamped-at-the-deadline run are the run's facts up to
  the deadline: the tie of such an event resolves the pump way already (the
  host's stamps, 6.2 rule 7), so nothing new was needed at the M3 line.
- `start_run` — the same arguments, non-blocking: the run goes in a job's
  own thread; the answer is `{job_id}` at once. `poll_run {job_id,
  wait_ms? = 0, cap 30000}` waits for the end and returns the run result (or
  `{status: "running"}`); an unknown job is `isError`. The Engine runs two
  jobs' engines at once honestly: the instances are per-job, and the only
  process-wide bits are the live-group registry and the start instant that
  the signal/teardown machinery owns.
- `read_trace {path}` — the records of a v1 trace file: `{header, events}
  from the trace's own readers' wordings; an unknown version or an
  unreadable file is `isError`.
- The resources: `resources/list` shows the four spec resources —
  `spec://SPEC.md`, `spec://CONFORMANCE.md`, `spec://TRACE.md`,
  `spec://network.schema.json` (mime text/markdown or application/json, a
  title each) — plus every job's live `run://{job_id}` (mime
  application/json) and every kept trace's resource
  (`trace://{job_id}`, or `trace://sync/{n}` for a synchronous run's). The
  spec resources are build-embedded (`include_str!`): the text a
  client reads is this public repo's normative text at the build the binary
  came from, byte for byte (`resources/read` equality is a test), and it is
  version-stamped by the binary itself rather than read from a filesystem
  that may have moved on. A job's state is `{job_id, status: "running",
  events_so_far, answers_so_far}` while the run goes and the run's result
  plus its trace path once it ended; jobs live for the server's lifetime —
  nothing is dropped when a result is collected, so a client may read back
  what it saw, and a job the client never collects is dropped only when the
  whole server exits. `resources/subscribe {uri}` stands (unknown uris are
  refused, for a subscription that would never fire is a lie); the updates
  come as `notifications/resources/updated` (params `{uri}`) when a
  subscribed job's state changes — coarse, at most one per second per job
  while it runs, with the run's end announced immediately against the limit
  (it is the one change a subscriber must not miss; a client who misses it
  still has `poll_run`) — and `resources/unsubscribe` takes a subscription
  back off. No list-changed notifications: the capability promises none, and
  none are sent; a new job's resource simply appears from the next list on.
- The two honest exclusions, enforced rather than documented: a network with
  `human` neurons is refused by the run tools — the engine's no-desk route
  refuses at plant with the M1 wording carried by an extra clause (the desk
  is the person's surface, not an MCP surface), because a person answers at
  a desk and an MCP result never holds that page; and a network with an
  `https://` neuron is refused by the engine as at M2 (no TLS in a std-only
  crate). `conform` and the fixtures are not exposed.
- Pointing a client at it: run `neuron-engine mcp` as a child process and
  speak the protocol to its stdin/stdout — the tests do exactly that
  (`tests/mcp.rs`, a subprocess with a buffered reader, no MCP client
  dependency).
- Covered by `tests/mcp.rs` (14): the handshake with both protocol-version
  answers and the second-initialization refusal, ping, unknown method,
  malformed line, tools/list with the inputSchemas, resources/list with the
  four spec resources then a live run's entry, resources/read for every
  embedded text (byte for byte) and the unknown-uri refusals, check_network
  on an example (ok) and on a bad network (errors with file/line/col) and
  the unreadable and no-file refusals, run_network sync (answers, events,
  summary), the three refusals (a failing file, `https://`, the human
  clause), timeout_ms honored, poll_run of an unknown job, start_run +
  poll_run to completion with the live resource's two states and its trace,
  the subscription's announcements, read_trace round trip and its two
  refusals, and the server leaving cleanly when the client closes stdin
  (killing any live group first, and empty of stray stdout lines).

### Gates measured when M3 closed

- `conform spec/fixtures` 201/0/0 and `conform py/probes` 10/0/0 — unchanged.
- `cargo test`: unit 2, check 11, desk 11, http 11, mcp 14, real 27, all
  green (the mcp suite runs its many short servers one at a time with the
  rest, as the crate's config already holds).
- `cargo fmt --check` and `clippy --all-targets -- -D warnings` clean.
- The three alert smokes still write their traces through the unchanged CLI
  (this machine's closing run: 316/316/587 events — counts vary run to run
  with the machine's load, as the M1 section notes).

## Design in one page

`main.rs` (the CLI: discovery, argument, output) and `runner.rs` (the runner of
CONFORMANCE 3-6: the live-stream consumer, the two-run observer rule, message
prefixes, harness and refusal handling) drive `engine.rs`. `engine.rs` is the
whole simulation: one `BinaryHeap` of wake-ups ordered by `(t, set-order)`, so
SPEC 6.2 rule 1 and the fixed points of CONFORMANCE 4.1 fall out of the arm
order; a rule-7 reply wake-up is simply armed before its call's timeout. Every
difference between the virtual host and the real one sits in one `is_real()`
branch. Signals, works (held inputs, queued and running calls), level-0
groups, level-1 processes with their slots and queues, the staleness scopes
(the outer instance is one scope; a composed neuron's inside is a scope per
call) and the per-trace budget pools live in plain vectors and maps. Events
are built as `json::Value`s whose ids are opaque strings; the runner
normalises both event lists by first appearance (CONFORMANCE 3.2) before
comparing. The inside of a composed neuron is simulated honestly but its
events go to a discard buffer (sealing, SPEC 2.6). The real host's loop pumps
a queue of host events stamped when the reader read them, so an event's time
is never later than its own timeout. No external crates: `json.rs` is a
std-only JSON parser/writer with `deep_eq` (`covers` for observer rules),
which is what a `serde`/`serde_json` pair would have done, at about 500 lines
over what `serde_json` costs — this is the honest price of a dependency-free
engine, not golf. `net.rs`'s validators produce positioned findings (a small
scanner walks the raw text's array spans and finds each rule's anchor key), so
`check` reports `file:line:column` where it can, and `load` flattens the same
rules to the strings the runner carries. A rule about a whole file whose key
is absent sits at `1:1`; the columns count characters.

Places where the fixtures pin behaviour beyond the literal text, and how this
engine reads it (all checked black-box against the TypeScript engine or taken
from fixtures):

- The restart delay counts from the failure's detection, and a due restart is
  skipped silently when a kill grace is the only thing keeping the run open
  (`failures/l1-process-exits-and-leaves-a-survivor`; a probe against the
  TypeScript engine agreed that the delay does not wait for the grace).
- A pong deadline keeps the run open only while other work does: the run-over
  rule does not count it (probe: the TypeScript engine ends the run with a
  pending pong deadline; `CONFORMANCE` 4.1's list of things that keep a run
  open does not include it).
- A cut level 1 call's `commit` lands when its slot frees (probe against the
  TypeScript engine: commit at the reply that frees the slot, not at the cut).
- A cancelled level 1 call's slot and a kill grace keep the run open; restart
  delays and idle pings do not (`runend/` fixtures).
- The `budget:` message is `budget: needs <cost> <unit>, <pool> left`
  (CONFORMANCE 3.4's fixtures pin it); with several short units the first in
  the neuron's cost order is named (no fixture pins the case).
- The trace's `finished` checks run after the stale drops a new signal caused
  and before its own delivery effects, and again after a reply step's queue
  drain (`budget/a-cut-call-commits-its-reservation`,
  `budget/a-queued-level-1-call-checks-when-its-slot-frees`, a probe of mine).
- A level 0 cancelled call's group (and the kill grace) keep the run open and
  delay the call's `commit` and the trace's `finished` to the kill
  (`budget/a-cut-call-that-ignores-term-commits-when-killed`).

And the real host's own choices (M1; the SPEC text each stands on is named):

- A call settles 100 ms after its process exits, whatever the exit was
  (decision 0001, Appendix C): the outcome is read whole — an over-long
  output's kill can be seen at all, a survivor's last bytes land in the head.
  The level 1 down-notice rides the same settle, so an exit's last lines are
  read before the crash is reported.
- A program whose group ignores SIGTERM keeps the run open past its term
  (6.2 rule 6): the engine probes the group with `kill(-pgid, 0)` — the
  POSIX way to ask whether a member is still there — and the kill comes
  2000 ms after the term (6.3). The probe is honest and imperfect the same
  way: a member the engine has not reaped yet, or a reparented zombie for a
  moment, reads as a live member, because for an engine whose monitors reap
  what they see that is what "a member" is.
- A command's first argument containing `/` is resolved by the engine against
  the neuron's working directory before the exec, because on darwin the
  spawn's own chdir happens around it (races the exec); a bare name goes to
  `PATH` (2.3). The run command resolves the declaring file's directory
  against the caller's cwd when the file is named bare (2.4).
- The end-of-run teardown polls: stdin is closed everywhere, then the
  engine waits up to 2 s (a serve program's shutdown window, 3.2), SIGTERMs
  what still lives, waits 2 s more, SIGKILLs, and returns when the groups
  are gone or after a bounded last wait. A group that is gone is not waited
  for; a leftover the engine never reported on is stopped silently
  (decision 0001), without the notes 6.3 gives a cancelled call.
- The events of the real host are stamped when the reader read them, so the
  engine's virtual clock and the wall clock cannot disagree about an event's
  order at its own timeout instant (6.2 rule 7).
- `check` positions a rule on the quoted key it names, found in the raw
  text by span; a rule about the whole file whose key is absent (a required
  `neurons`, the cycle, the un-costed budget demand) sits at `1:1`, and the
  position of a warning about a composed file's costs is the declaring
  neuron's `cost` (10.5 names the declaring cost as the thing lacking).
- A missing composed file is an error of the declaring file, at the
  `network` key that names it; the composed file's own errors, when it can
  be read, are its own (each at its own file's positions).

## Size

Lines as committed (`wc -l`), formatted at `max_width = 140`:

| File | Lines |
|---|---|
| `src/json.rs` | 415 |
| `src/net.rs` | 1083 |
| `src/check.rs` | 257 |
| `src/fixture.rs` | 400 |
| `src/engine.rs` | 4182 |
| `src/host.rs` | 953 |
| `src/run.rs` | 168 |
| `src/trace.rs` | 204 |
| `src/runner.rs` | 539 |
| `src/mcp.rs` | 1335 |
| `src/main.rs` | 198 |
| `src/lib.rs` | 15 |
| source total | 11152 |
| `tests/real.rs` | 802 |
| `tests/check.rs` | 493 |
| `tests/desk.rs` | 664 |
| `tests/http.rs` | 394 |
| `tests/mcp.rs` | 927 |

Updated after M3 (`wc -l` of the same files, plus the new ones): `mcp.rs`
(new) 1335, `engine.rs` 4182, `host.rs` 953, `run.rs` 168; tests:
`mcp.rs` (new) 927. Sources 11152, tests 3280 (with the two desk/http test
files of M2 and the probe unit test), 14432 all told.

For scale: the TypeScript reference engine (`src/engine/`) is 1353 by
`wc -l` (decision 0005's table). This crate is larger for the same honest
reasons as at M0 — the std-only JSON layer and the conformance runner — plus
the M1 process layer (host, run, trace, check) and the M3 std-only MCP server
(the whole client side of the protocol: a JSON-RPC frame on the JSON layer,
five tools, the resources and their subscriptions) that a Python or Node
engine would get "for free" from an SDK. None of this is golf.
