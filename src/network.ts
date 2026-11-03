import { Neuron } from './neuron.js';
import { newSignal, type Payload, type Signal } from './signal.js';

export type NetworkOptions = {
  /** Called for every signal: each send or emit, and each answer as it arrives. */
  onSignal?: (signal: Signal) => void;
  /** Called when a neuron fails while handling an `emit`. Never called for work that was replaced. */
  onError?: (error: unknown, signal: Signal, neuronId: string) => void;
  /** Called when a neuron's work for `signal` was dropped: 'replaced' by a newer emit, or refused for too many 'hops' (its `maxHops`),
   * or held by a neuron with a `threshold` that was not met within its `within`: 'expired'. */
  onDrop?: (signal: Signal, neuronId: string, reason: 'replaced' | 'hops' | 'expired') => void;
};

export type SendOptions = {
  /** Only these neurons (must be linked). Default: every linked neuron. */
  to?: string[];
  /** A signal that led to this one (usually an answer being passed on): the new signal joins its trace and records its id as `cause`. */
  cause?: Signal;
};

export type EmitOptions = SendOptions & {
  /**
   * A key. A newer emit from the same sender with the same key aborts the
   * older one's in-flight work, and the older work's answers are dropped.
   */
  replaces?: string;
};

/** A neuron's limits refuse a signal that is too deep in its chain. `Neuron.query` is a direct ask and skips them. */
const refuses = (target: Neuron, signal: Signal): boolean =>
  target.options.maxHops !== undefined && signal.hops > target.options.maxHops;

/** An input waiting for its neuron's threshold. `settle` stops its timer and listener; `drop` also reports it. */
type Held = { signal: Signal; settle: () => void; drop: (reason: 'replaced' | 'expired') => void };

/** How much one sender counts toward a threshold. shortcut: always 1; strength will return the receiver's rating of `sender`. */
const weight = (_target: Neuron, _sender: string): number => 1;

export class Network {
  private readonly neurons = new Map<string, Neuron>();
  private readonly pending = new Set<Promise<void>>();
  // Inputs held for a threshold, by neuron and trace, then by sender. Not in `pending`: `idle` does not wait for them.
  private readonly held = new Map<string, Map<string, Held>>();
  // shortcut: one controller kept per sender+key forever; prune if keys are unbounded.
  private readonly replaceable = new Map<string, AbortController>();

  constructor(private readonly options: NetworkOptions = {}) {}

  add(neuron: Neuron): Neuron {
    this.neurons.set(neuron.id, neuron);
    return neuron;
  }

  connect(a: string, b: string): void {
    this.get(a).connect(this.get(b));
  }

  /**
   * Send from one neuron to everything it is linked to, or only to `to`
   * (which must be linked). Delivers in link order, one at a time, and
   * returns each answer as a signal from the neuron that gave it. A target
   * whose `maxHops` the signal exceeds is skipped: its answer is left out,
   * with no `onDrop` (the signal itself is still reported to `onSignal`).
   * `threshold` does not apply to `send`, which is a direct exchange: every
   * target handles the one signal at once, with no `context.inputs`.
   */
  async send(fromId: string, payload: Payload, { to, cause }: SendOptions = {}): Promise<Signal[]> {
    const { sender, targets } = this.resolve(fromId, to);
    const signal = newSignal(fromId, payload, { to: targets.map((t) => t.id), ...(cause && { cause }) });
    this.options.onSignal?.(signal);
    const answers: Signal[] = [];
    for (const target of targets) {
      if (refuses(target, signal)) continue;
      const answer = newSignal(target.id, await target.receive(signal), { to: [sender.id], replyTo: signal });
      this.options.onSignal?.(answer);
      answers.push(answer);
    }
    return answers;
  }

  /**
   * Like `send`, but returns the outgoing signal at once and does not wait.
   * Targets work in parallel; answers arrive through `onSignal`, failures
   * through `onError`. With `replaces`, a newer emit under the same key from
   * the same sender aborts the older one's work (adapters get an abort
   * signal) and nothing more is reported for it except `onDrop`. A target
   * whose `maxHops` the signal exceeds never runs: `onDrop` with 'hops'.
   *
   * A target with a `threshold` holds each input until enough distinct
   * senders have sent one on the same trace: a sender's newer input replaces
   * its older one ('replaced'), the sum of senders reaching the threshold runs
   * the adapter once with those inputs in `context.inputs` and answers the
   * last of them (the others get no answer, no drop), and an input still held
   * after the neuron's `within` is dropped as 'expired'. A held input whose
   * emit is replaced is dropped as 'replaced'. Held inputs are waiting, not
   * working: `idle` does not wait for them, and their timers never keep the
   * process alive, so a program that ends first never sees the 'expired'.
   * A later input on a trace that already ran starts a new hold.
   */
  emit(fromId: string, payload: Payload, { to, replaces, cause }: EmitOptions = {}): Signal {
    const { targets } = this.resolve(fromId, to);
    const signal = newSignal(fromId, payload, { to: targets.map((t) => t.id), ...(cause && { cause }) });
    const controller = new AbortController();
    if (replaces !== undefined) {
      const key = `${fromId}\0${replaces}`;
      this.replaceable.get(key)?.abort();
      this.replaceable.set(key, controller);
    }
    this.notify(() => this.options.onSignal?.(signal));
    for (const target of targets) {
      const work = this.run(target, signal, controller.signal).finally(() => this.pending.delete(work));
      this.pending.add(work);
    }
    return signal;
  }

  /** Resolves once all emitted work has finished or been dropped, including work started meanwhile. */
  async idle(): Promise<void> {
    while (this.pending.size) await Promise.allSettled([...this.pending]);
  }

  private async run(target: Neuron, signal: Signal, abort: AbortSignal): Promise<void> {
    if (refuses(target, signal)) return this.notify(() => this.options.onDrop?.(signal, target.id, 'hops'));
    const inputs = target.options.threshold === undefined ? undefined : this.hold(target, signal, abort);
    if (target.options.threshold !== undefined && !inputs) return; // held, waiting for the others
    let payload: Payload | undefined;
    let failure: { error: unknown } | undefined;
    try {
      payload = await target.receive(signal, { abort, ...(inputs && { inputs }) });
    } catch (error) {
      failure = { error };
    }
    // Listeners run outside the try, so a listener's bug is never blamed on the neuron.
    // An aborted run is never an error, whatever the adapter threw or returned.
    if (abort.aborted) this.notify(() => this.options.onDrop?.(signal, target.id, 'replaced'));
    else if (failure) this.notify(() => this.options.onError?.(failure.error, signal, target.id));
    else this.notify(() => this.options.onSignal?.(newSignal(target.id, payload as Payload, { to: [signal.from], replyTo: signal })));
  }

  /** Hold `signal` for `target`'s threshold. Returns the inputs to act on if it is met, otherwise undefined. */
  private hold(target: Neuron, signal: Signal, abort: AbortSignal): Signal[] | undefined {
    const key = `${target.id}\0${signal.trace}`;
    const group = this.held.get(key) ?? new Map<string, Held>();
    group.get(signal.from)?.drop('replaced');
    this.held.set(key, group);
    const settle = () => {
      clearTimeout(timer);
      abort.removeEventListener('abort', onAbort);
    };
    const entry: Held = {
      signal,
      settle,
      drop: (reason) => {
        settle();
        group.delete(signal.from);
        if (!group.size) this.held.delete(key);
        this.notify(() => this.options.onDrop?.(signal, target.id, reason));
      },
    };
    const onAbort = () => entry.drop('replaced');
    const timer = setTimeout(() => entry.drop('expired'), target.options.within ?? 10_000);
    timer.unref();
    abort.addEventListener('abort', onAbort, { once: true });
    group.set(signal.from, entry);
    let total = 0;
    for (const sender of group.keys()) total += weight(target, sender);
    if (total < (target.options.threshold as number)) return undefined;
    const inputs = [...group.values()];
    for (const held of inputs) held.settle();
    this.held.delete(key);
    return inputs.map((held) => held.signal);
  }

  /**
   * Listeners called for `emit` work (onSignal, onError, onDrop) cannot take
   * the network down: whatever they throw is swallowed, since nothing is
   * waiting to catch it. Catch errors inside your own listener if you care.
   */
  private notify(call: () => void): void {
    try {
      call();
    } catch {
      // swallowed on purpose, see above
    }
  }

  private resolve(fromId: string, to?: string[]): { sender: Neuron; targets: Neuron[] } {
    const sender = this.get(fromId);
    const targets = to
      ? to.map((id) => {
          const target = this.get(id);
          if (!sender.links.has(target)) throw new Error(`${fromId} is not linked to ${id}`);
          return target;
        })
      : [...sender.links];
    return { sender, targets };
  }

  private get(id: string): Neuron {
    const neuron = this.neurons.get(id);
    if (!neuron) throw new Error(`unknown neuron: ${id}`);
    return neuron;
  }
}
