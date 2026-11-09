# Decision record 0014: the rust engine's lifecycle audit (2026-11-09)

**Status: decided and built (Kevin, 2026-11-09)** — findings 1–9 and the budget
regression test are on `feat/audit-lifecycle` today, one commit per item
(`75a3cd3` 1, `cf2954d` 2, `09df92a` 4, `2b571cd` the budget pin, `de505d2` 5,
`2dc86ed` 6, `79a2a17` 7, `ef94c27` 8, `b314149` 9, `2cc536c` 3), each red-test
first and gated before its commit. Measured on the branch at `2cc536c`, by the
merge gate's owner: cargo test 117/0 (unit 2, check 16, desk 17, http 11,
mcp 22, peers 20, real 29), fmt and clippy
clean, conform `spec/fixtures` 224/0/0 on five identical runs, `conform
py/probes` 10/0/0, `demo:p2p` and `npm run check` on the same tip. Findings
10–17 stay queued after the serve build. Nothing here changes the SPEC or the
fixtures: every item
below is a bug fix under text the SPEC already fixes, and the budget item pins
behavior that already holds. The peer wire changes only as far as findings 1, 3,
8 and 9 and the serve build need.

## The problem

The peer layer (0013) is merged and green on `main` (`e407c21`, counts below), and
peer-hosted `serve` is next. Before building on that lifecycle, a probe audit of
`rust-engine` ran on 2026-11-09 (ponytail-audit rules: bugs and data loss first,
no repo code changed, probes kept outside the repo). It found cancel- and
ordering-shaped gaps that the green suites do not exercise: a cancelled remote
call can still execute, a composed cut frees its bookkeeping while the inner work
keeps running, a hosted graph drops the composed semantics (2.6), a late remote
outcome answers a call after its timeout, serve startup can ignore the route's
deadline, restart loops never reach their five-failure limit, a stale slot
deadline can kill a restarted process, and a cancel against a backpressured peer
blocks the decision thread.

## The audit

Roughly fifteen subprocess probes drove the debug binary (`e407c21`) directly:
raw-socket peers, paused handshakes, non-reading peers, MCP and HTTP jobs, real
processes with orphan checks, a deep-JSON input, a signal-and-descendants probe.
The engine's own gate was verified unchanged, so every finding below is a gap the
green suites miss, not something already red: `cargo test` 106, conform
`spec/fixtures` 224/0/0 (five consecutive identical count lines), `py/probes`
10/0/0, `cargo fmt` and clippy clean, `demo:p2p` passing. The probe scripts live
in a 24-hour scratch, so the regression tests this decision orders are the
durable record of each finding. The line numbers below name `e407c21` and drift
as fixes land.

## The decision

1. **Fix findings 1–9 before peer-hosted `serve`.** Each fix lands with a
   failing-test-first regression test in the crate's real-process style (the
   peer and timing suites already serialize under `RUST_TEST_THREADS = 1` in
   `rust-engine/.cargo/config.toml`), one commit per finding, and the same gate:
   `cargo fmt --check`, clippy `-D warnings`, `cargo test`, then conform 224
   five-run identity. Tests assert what SPEC 6.1 fixes — a `code`, a message
   prefix, the presence of an event — never errno wording or wall-clock ties.
2. **Priority 2 (budgets) is pinned by a test only.** The audit's probe showed a
   remote call under a budget already carries the `reserve`/`commit` notes and
   the second call fails with the budget error, so the answer is a permanent
   regression test beside the peer tests, not an engine change.
3. **Findings 10–13 then 14–17 are queued after.** 10–13 are crash- and
   teardown-shaped (deep JSON, an unknown source name, refusal teardown, signal
   descendants); 1–9 are the ordering cluster the serve build inherits, which is
   why they go first — Kevin's go for the fix pass accepted the audit verdict:
   "fix 1–9 before building peer-hosted serve; the current green suites do not
   exercise these lifecycle and ordering failures."
4. **The handoff gains the gate.** `docs/HANDOFF.md` names finding 1's gate above
   priority 1 so a session that picks up the queue reads 0014 first.
5. **Merging stays on Kevin's go.** Branch `feat/audit-lifecycle`, `--no-ff`, the
   repo's message style, gated on merged `main` like every family merge.

## The findings

| # | Finding (what is wrong) | Where at `e407c21` | The fix |
|---|---|---|---|
| 1 | A cancelled peer call can still execute: the worker clears a pending cancel when it takes a call, and the driver can send a call after its cancel | `peers.rs:347-433,568-605,625-672` | Key cancellation to the call id; never clear a received cancel; check before spawn and before send, with the final check ordered against cancel |
| 2 | Composed cancellation frees bookkeeping without cancelling real work: `composed_cancel_inner` marks work done and frees serve slots through none of the L0/serve/HTTP/human/peer cancel paths | `engine.rs:1898-1985,4434-4498` | Drive each open inner work item through the existing `cut_work` path, inner reports sealed, work stays open until its process or slot actually finishes |
| 3 | Hosted graphs lose composed semantics (2.6): every hosted call builds a fresh standalone engine, so stateful inner neurons restate and a fast exit does not finish or cancel sibling work | `peers.rs:596-605,703-782`, `docs/SPEC.md:237-259` | One engine per hosted instance; reuse the composed-completion machinery: first exit reply wins, remaining work cancelled, inner serve state persists |
| 4 | A remote composed call can answer after its timeout: the composed branch runs before the eligibility check, and `composed_remote_done` checks `done`/`cut` but not `reported` | `engine.rs:2517-2541,4402-4416` | Route a late remote outcome through cleanup only; require still-running and not-done before `composed_remote_done` |
| 5 | Serve startup ignores the route's deadline: `start_serves` waits each neuron's `timeout`, uncapped by the run deadline | `engine.rs:632-638,964-1031` | Cap startup waits at the run deadline and return the route's normal timeout result after cleanup |
| 6 | Restart failures never reach the five-failure limit: spawn/ready failures increment the count and schedule another restart, bypassing the limit that fails queued work | `engine.rs:3720-3848,4142-4155` | One failed-start finalizer: at five, mark the serve failed, fail queued calls with `start`, stop scheduling restarts |
| 7 | An old slot deadline can kill a restarted process: `SlotDeadline` carries only `(server, re)` and `re` restarts at 1 each generation | `engine.rs:426-444,1540-1555,3692-3700,4262-4275` | Carry the generation (or the work id) in `SlotDeadline` and require an exact match before `process_down` |
| 8 | An effect that never started is reported `unknown`: connection, handshake and spawn failures arrive as ordinary `L0Done` outcomes | `peers.rs:276-287,347-360,625-650`, `engine.rs:1645-1662,2517-2541` | Carry a started flag on the remote outcome; report those failures as `error` (SPEC 11's unstarted case), reserving `unknown` for genuinely ambiguous started work |
| 9 | A backpressured peer stalls the engine: `Peers::cancel` does a blocking socket write on the decision thread | `peers.rs:296-314,913-918`, `engine.rs:2387-2400` | Move the cancel write off the decision thread; abort the socket when cancellation cannot be delivered promptly |
| 10 | Deep JSON aborts the process (stack overflow): the parser recurses without a depth limit, and the same parser reads peer messages | `json.rs:142-191`, `peers.rs:420-424,571-590` | A fixed nesting-depth cap that returns a normal parse error |
| 11 | An unknown input source panics (harness failure) and orphans started processes | `run.rs:38-53`, `engine.rs:1289-1294` | Validate source names in preflight and refuse normally; the same refusal teardown for every route |
| 12 | Refused injections skip real-process teardown: budget and key refusals return before `end_run_quietly` | `engine.rs:1044-1063,1130-1132`, `run.rs:118-125` | Run refusal teardown before the early return; refusal tests assert the process group is gone |
| 13 | Signal teardown forgets surviving descendants: the group unregisters when its leader exits | `host.rs:190-219,607-634,795-813` | Unregister only when the group is empty; prune dead retained ids; unregister after normal cleanup |
| 14 | Dead serve generations retain writer threads | `host.rs:654-704,795-813`, `engine.rs:4066-4160` | Remove the old generation and drop its sender after reading the crash stderr |
| 15 | The driver accepts malformed peer envelopes (handshake type/version, outcome type/id unchecked) | `peers.rs:385-405,420-433` | Require `hello` with `v: 1` and the expected name; require `done` with the active call id |
| 16 | Fail-open streaming fails the run when `--trace` asks for the self-check with no stream sink installed | `main.rs:149-182`, `run.rs:185-199` | Enable the self-check only when a sink was installed; extend the FIFO test with `--trace` |
| 17 | An unused, speculatively-kept helper in the peer tests | `tests/peers.rs:845-850` | Delete it; reintroduce when a test needs it |

## Not decided here

- The P1 serve build's design (the wire's `serve_start`/`ready`/`line`/`down`
  messages, the start-failure trap named in the handoff) stays a separate
  decision built on this one.
- Findings 10–17's exact build order after the serve build.
- Anything about the TS/Python engines: they were not in this audit's scope.

## Where it stands

Started 2026-11-09 on `feat/audit-lifecycle` with findings 1–9 and the budget
regression test; counts quoted above are the audit's at `e407c21` and the branch
gate restates them from the tree before the merge.
