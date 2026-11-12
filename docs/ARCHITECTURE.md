# Architecture map (informative)

A dense index of how agent-neuron is put together: the model, the engine, the inside of the
Rust engine, the wire between engines, and the ways to run. It is written to be read fast by
someone (or some agent) who needs to find the right file and function, not as an introduction; for
that, read the [README](../README.md).

**This is a snapshot, taken 2026-11-12 with decision 0021's `maxInflight` option built (L2) and decision 0023 (one engine) carried out.** Nothing here is
normative. The contract is [SPEC.md](SPEC.md), [`spec/fixtures/`](../spec/fixtures/) and the
[decision records](decisions/). A name below that no longer greps is this file's error: fix it here.

```
MODEL
  network file  = closed JSON, version 0: {version,name,neurons[],links[],entry?,exit?,budget?}
  neuron        = wrapper around ONE thing; exactly one kind:
    source   root signals enter here (no program)
    run      level 0: argv, one process per call, stdin=input JSON, stdout=answer, exit code
    serve    level 1: long-lived process, newline JSON; says `ready` {v, maxInflight}; slots + queue
    http     one request per call
    human    a person answers at the desk (page)
    network  composed: a whole network file as one neuron (entry in, first exit reply out; inside sealed)
    neuron   named by its own neuron file (0019); using entry may NARROW timeout/env/may/maxInflight only
             and may ADD args[] to a run/serve file's command (0022 S4; an entry field, gone after the loader)
  options (10)  threshold, within, maxHops, timeout, env, cwd, cost, effect, may{connect,listen,read,write}, maxInflight (run|http|network: calls at once, rest wait in arrival order; 0021 L2)   (SPEC 2.4)
  settings      .env beside the declaring file (neuron file, or network file for inline) fills `env` names; wins over the engine's environment; never reported (0022 S6; settings.rs)
  input         {at, source, payload, replaces?, budget?}; `replaces` is the staleness key and rides the signal
  signal        payload + trace id + cause; link from->to carries answers; `to`-less reply routes by links
  trace         one root signal and everything it causes; ends with `finished`
  events        signal, answer, quiet, error, unknown, drop{reason,phase}, note{what: reserve|commit|cancel|term|kill|restart|junk|inject}, finished
  outcome/call  exactly one: answer | quiet | error(code: exit|timeout|junk|output|http|inner|budget|start|neuron) | unknown (effect call that started, outcome not knowable)
  staleness     newer signal on same `replaces` key cuts older work: Held/Queued -> drop phase `waiting`; Running -> cancel (effect: marked, never cut)
  budget        pool per trace; check -> reserve at call START -> commit when program over
  queued        (level 1 slot, maxInflight place, --at-once): not started => no reserve, timeout not counting, never unknown, keeps trace open
  maxInflight   place held from start until the call's record is done; given back at END OF STEP, then oldest waiting call whose neuron has room starts, one at a time (SPEC 6.2 rule 9)
  cut sequence  cancel note -> SIGTERM group (`term`) -> 2000 ms grace -> SIGKILL (`kill`) -> commit -> done

ONE ENGINE (conformance = scripted host, virtual time)
  Rust        rust-engine/   the engine and the reference: real processes, peers, desk, confine, mcp
  history     a TypeScript library, a TypeScript reference engine and a Python engine passed the same fixtures until decision 0023 deleted them
  gate        `npm run check` = cargo fmt --check, clippy -D warnings, cargo test --release, then conform spec/fixtures and spec/probes; fixtures by area in spec/fixtures/<area>/

RUST ENGINE  rust-engine/src
  engine.rs 5.5k  the Engine: nodes, works, sigs, heap of wake-ups, all rules
  host.rs         real host: spawn groups, reader threads -> ONE mpsc queue of Ev (stamped when read); Post handle (Clone)
  peers.rs        wire, driver side (Peers) + worker side (`peer` command), Hosted, Gate
  net.rs          parse/validate network + neuron files      check.rs  `check` command
  run.rs          `run` command glue, RunOpts, live stdin producer, live_input_of
  desk.rs         human page (HTTP), grants                  http.rs   http neurons
  confine.rs + proxy.rs   sandbox-exec profile from `may`, loopback connect proxy (macOS only)
  mcp.rs          stdio JSON-RPC, five tools                 trace.rs  trace file + Stream
  runner.rs + fixture.rs  conformance runner (scripted host; leak check for forgetting)

  STATE
    nodes[]   per planted neuron: kind, inst (instance; 0 = outer), display path, timeout, cost, effect, peer?
    works[]   one per call/held input: {node, sig, trace, kind WKind(Run|Serve|Human|Http|Composed), st WSt(Held|Queued|Running), done, cut, l0?, inner_root?, reserved, committed}
    sigs{}    signals by id      l0s[] L0Proc{work, gid, leader_alive, remote, holds}
    serves{}  per serve node: cur Proc{gen, ready, slots, max_inflight}, queue, restart/backoff/ping state
    pools{}   budget per trace   groups{} threshold/within holds   traces[] roots to report `finished`
    low       index below which works are forgotten (tidy/all_done/forget/forget_later)
    at_once, cap_live, cap_queue   engine cap (0021 L1)
    places{}, waiting              a neuron's own limit (0021 L2): node -> works holding a place; one arrival-ordered queue for every neuron
    hosting, hosted_out, inner_why, caller   hosted-file mode
  LOOP (real host)
    Engine::drive(&mut dyn Door) -> timed_out      Door{take, deadline, ends_when_over}
      RunDoor     a run (file inputs or live stdin)
      HostedDoor  hosted file: takes HostedReq::{Call(HostedCall), Inject(Box<Input>)} from mpsc, backstop
    real_step: tidy -> wait min(next wake, deadline) on host queue
               -> host_step(Ev) | fire(Wake) -> place_drain -> cap_drain -> wake_composed_calls
    Ev (host.rs):   L0Done L1Line L1Down ServeStartFailed HttpDone Desk Inject InputsClosed Nudge
    Wake (engine):  Inject ObserverInject CallEnd CallTimeout Within L1Reply CancelReply EndAt StartDeadline ReadyAt L0Grace ServeGrace RestartDue PingTick PongArrive PongDeadline SlotDeadline
    rule: nothing in the engine blocks on a process; everything reports into the one queue
    scripted (conformance) host has its own small loop in run(): fire -> wake_composed_calls -> place_drain; no real time, no at_once
  CALL PATH
    signal arrives -> make_call -> start_new_call [maxInflight gate: Queued in `waiting`] -> start_placed -> by kind:
      Run       start_l0: [cap gate] check_cost -> reserve -> host.l0_begin | peers.start -> arm CallTimeout
      Serve     free slot ? signal_line : Queued in serves[node].queue (drain_serve on slot free)
      Human     desk      Http  thread -> HttpDone      Composed  start_composed (inner instance or remote graph)
    both limits on one call: the neuron's place first (kept while it waits), then the cap gate in start_l0; a placed neuron's maxInflight is held by the driver
    end: host_l0_done -> l0_finished -> call_replied(Outcome) | fail_call ; cut: cut_work -> terminate_l0 -> L0Grace -> fire_l0_grace
  COMPOSED / HOSTED
    composed call: inner trace root = works[cw].inner_root; first reply at exit answers the call (composed_exit_replied) then composed_cancel_inner; quiet inside -> quiet; inner failure -> error `inner` (cause sealed)
    plant_hosted: hosted file IS instance 0; callers enter through hidden node `__call` (Kind::Network, own never-reported instance); a wire call = a composed call

WIRE (peers; newline JSON over TCP; token handshake)
  hello{v:1,token,peer} -> hello{ok,mode: program|graph|serve}
  call{id,argv|graph,cwd,env,input,timeout} -> done{id,out,started}      cancel{id}
  inject{source,payload,replaces?,budget?}   no reply; refusal on worker stderr "refused an injection: ..."
  one call at a time per connection; calls overlap across connections
  worker modes
    peer                         runs programs (execute_program); --at-once => process-wide Gate (FIFO)
    peer --serve                 hosts driver's serve programs, one per connection
    peer --network <f> [--peers] [--stream <p>] [--at-once n]
                                 hosts a file: one Engine on thread "hosted-graph"; file must be callable (entry+exit) or fed (a source) or both; ground refused; neither => exit 1
  driver: peers file {version,self,peers[{name,address,token}],placement{node path: peer}}; a placed call is an L0Proc{remote:true}; driver timeout runs from send
  BACKSTOP_MS past a call's timeout the worker kills what nobody will report

RUN MODES
  batch   run <f> --inputs <jsonl> [--trace] [--stream]   inputs at `at` ms; ends when nothing left; trace at end
  live    run <f> --inputs - --stream <p>                  stdin lines injected when read; ends at EOF + nothing left; keeps nothing; refuses --trace
  hosted  peer --network                                   never ends; serve programs persist; events only to --stream
  flags   --ping --grants --peers --confine (not with --peers) --at-once <n> --result <path | -> (one object at the end; exit 3 on any error or unknown)
  exit    1 = run refused/failed to start, 2 = usage

GLOBALS the driver holds for everyone (SPEC App. E.2 G1-G8 + 0020 S4d table)
  one table of work/signals, staleness + thresholds, placement, confinement, teardown (kill_live_groups on signal), time limits, the caller's door
```
