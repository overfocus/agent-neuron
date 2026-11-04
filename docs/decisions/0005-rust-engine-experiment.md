# Decision record 0005: the Rust engine experiment (2026-11-04)

A time-boxed experiment with go/no-go criteria agreed with Kevin: build the engine of
[SPEC](../SPEC.md) a second time, in Rust, from the specification text alone, and see whether it
meets six criteria. The rule agreed beforehand: missing any criterion is a no-go. The builder wrote a
first version of this record that said go; two independent reviewers re-ran the evidence and
differential probes, and this is the corrected record.

## Result: no-go

The TypeScript engine (`src/engine/`) stays the only engine. The Rust code is on the branch
`feat/rust-engine` (commit `e5a44d3`, pushed for reference, not merged); none of it is on `main`.
Criterion 5 missed a rule of SPEC 3.1 and 6.3, and criterion 6 was met only on a technicality, so the
rule gives no-go. The earlier record's "no disagreements with the TypeScript engine" and "no criterion
bent" were wrong; see below.

## Verdict per criterion

| # | Criterion | Verdict | Independent evidence |
|---|---|---|---|
| 1 | `conform spec/fixtures` passes every active fixture, identically over 5 runs | met | 138 passed, 0 failed, 10 pending, identical on 5 runs by both reviewers. |
| 2 | Alert storm with real processes (JavaScript, Python level 0, Python level 1) passes the trace validity and replay checks | met | 14 of 14 checks across the three networks. |
| 3 | `hypothesis_serve.py` (30 lines) works unchanged | met | Runs as is: 17 calls, 10 cut, 7 finished, as with the TS engine. |
| 4 | Level 1 round trip p99 under 5 ms | met, with limits | 1 ms p99 on every reviewer run at load 5 to 6; the TS engine reached 15 ms once under the same load. The builder's own record had 2 of 21 runs above 5 ms (157 ms and 9 ms) on a busy machine. The bench sends 3 ms apart, so there is no queueing, and the trace has whole-ms resolution. |
| 5 | Process hygiene | met except one rule | When a group leader dies on SIGTERM and a child that ignores SIGTERM survives, Rust kills the group at once and writes no `kill` note. SPEC 3.1 says SIGKILL comes "2 seconds later if anything remains" and 6.3 asks for the note. The TS engine follows the spec. |
| 6 | Core plus engine at most about 1,500 lines, excluding tests | missed (in spirit); met only on a technicality by `wc -l` | 1,292 by `wc -l`, but the code is golfed: 125 lines over 140 characters (the longest 287), one- and two-letter names, statements stacked on a line. See "Size". |

## Size

| Measure | Rust (core + engine) | TypeScript (`src/engine/`) |
|---|---|---|
| `wc -l` as written | 1,292 | 1,353 |
| rustfmt default / prettier default | about 3,200 to 3,250 | 2,326 |
| rustfmt / prettier at width 140 | 2,491 | 1,741 |

The fair, readable size is about 2,200 to 2,500 lines for Rust, roughly 1.4 times the TS engine
formatted the same way. The repo's other Rust is close to rustfmt shape; this code was not.

## Disagreements with the TS engine (the first record said none)

Found with differential probes: the same network and inputs through both engines on the virtual clock.

1. **A timeout against another call's wake-up at the same instant.** `s` feeds `a` (timeout 100, never
   replies) and `b` (replies after 100). TS gives the timeout error of `a`, then the answer of `b`; Rust
   gives the answer first. SPEC 6.2 rule 1 and CONFORMANCE 4.1 (the order set) decide it: **TS follows the
   text.** Rust made every timeout lose every tie, as if every other wake-up were its own call's reply.
2. **A timeout set before a `within` expiry at the same instant.** TS: error, then the expired drop. Rust:
   the reverse. **TS follows the text.**
3. **`signal S:` for a signal other than SIGKILL.** Rust wrote `signal SIG30:` for SIGUSR1 and `SIG3` for
   SIGQUIT (it names only 1, 2, 6, 9, 11 and 15); TS writes `SIGUSR1`. The text was silent; it now says the
   name. **TS follows the (now explicit) text.**
4. **The kill grace** (criterion 5): TS gives the group 2 seconds and writes `kill`; Rust kills at once.
   **TS follows the text.**

A fifth probe (a kill grace and another call's timeout at one instant) agreed.

## Bugs in the Rust engine (not fixed: the code is not merged)

- A failed spawn stores process group 0, so SIGINT in that window means `kill(-0)`, which signals the
  engine's own group and whatever launched it.
- stderr reading stops at 64 KiB and the pipe is dropped, so a program that logs more gets SIGPIPE (status 141).
- `check` panics (index out of bounds) on a file whose whole content is `0`.
- Unbounded channels and arenas never shrink: memory grows for a long-running network.
- Blocking `println!` and `fs::write` on the current-thread runtime.
- `threshold as u32` truncates; `epoch + at` is unchecked.
- Lossy UTF-8 on protocol lines, where the spec wants them treated as junk.
- `cargo fmt --check` gives 105 hunks; `cargo clippy -D warnings` fails (31 warnings, among them `if_same_then_else` and dead fields).
- The observer fallback in its conform runner differs from CONFORMANCE 6.
- The core has no unit tests and there are no Rust mutants.
- `npm run check` would need the network on a fresh clone (cargo fetches about 20 crates).

## What was genuinely strong

- The sans-IO core: free of I/O, clock and RNG, with the fixtures running the same code paths as real
  runs. It is a good basis for a later WebAssembly sealed neuron (the JSON layer would move to a shared crate).
- Speed: `conform` about 2.5 times faster (0.048 s against 0.12 s); a burst of 3,000 signals in 83 ms
  against 219 ms; round trips equal or better under load.
- CONFORMANCE.md was implementable from the text. Three places needed a second reading (a scheduled
  `hang` must survive until the cancel; `starts[n-1]` counts restarts; a signalled scripted process exits in
  the same step).

## What the experiment fixed on main

It exposed holes in the specification and fixtures, closed in the commit that carries this record:

- **Ordering of timeouts** made explicit (SPEC 6.2 rule 1, CONFORMANCE 4.1) and pinned by four fixtures
  (`ordering/timeout-and-...`, each tie in both orders).
- **`signal S:` is the name** (SPEC 6.1); fixture `failures/l0-killed-by-a-named-signal`
  (the virtual host's `crash` reply may now name a signal).
- **Kill grace with a survivor** (SPEC 6.3): fixtures `staleness/cut-leader-exits-on-term-a-survivor-is-killed`
  and `failures/l1-process-exits-and-leaves-a-survivor`, with a `leaves` field on scripted calls and starts.
- **`replaces` is on `signal` events only** (SPEC 7.2); the example and one pending fixture had it on a `drop`.
- **A pending restart delay does not keep the run open** (SPEC 6.2 rule 6): fixture
  `runend/a-pending-restart-delay-does-not-keep-the-run-open`. This is what the TS engine did; the text
  said "no timer is pending", which could be read the other way.
- **A composed neuron's reply carries `exit`'s `replaces`** (SPEC 2.6): two fixtures (set and `null`).
- **Lead decision, for Kevin to confirm:** a kill grace already under way for any process group the engine has
  terminated, cancelled or crashed, keeps the run open and its `kill` note is recorded (SPEC 6.2 rule 6, CONFORMANCE 4.1).
  This is what TS does; the text covered only cancelled work.
- CONFORMANCE 6 (which list each observer run is compared with) is now spelled out; `docs/agents/feature-loop.md`
  lists the `conform` gate step; six mutants were added.

## Revisit Rust when

- a sealed composed neuron should run as WebAssembly, where the sans-IO core is the natural unit;
- the engine must be embedded in another runtime that cannot host Node; or
- scale makes the engine's own cost matter (throughput or memory of a long-running network).

Any new engine starts from the fixtures, which are the contract, and from a written formatting rule
(rustfmt, width 140 at most) so a size criterion means something.
