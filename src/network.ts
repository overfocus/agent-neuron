import { Neuron } from './neuron.js';
import { newSignal, type Payload, type Signal } from './signal.js';

export type NetworkOptions = {
  /** Called for every signal: each send or emit, and each answer as it arrives. */
  onSignal?: (signal: Signal) => void;
  /** Called when a neuron fails while handling an `emit`. Never called for work that was replaced. */
  onError?: (error: unknown, signal: Signal, neuronId: string) => void;
  /** Called when a neuron's work for `signal` was dropped because a newer emit replaced it. */
  onDrop?: (signal: Signal, neuronId: string) => void;
};

export type EmitOptions = {
  /** Only these neurons (must be linked). Default: every linked neuron. */
  to?: string[];
  /**
   * A key. A newer emit from the same sender with the same key aborts the
   * older one's in-flight work, and the older work's answers are dropped.
   */
  replaces?: string;
};

export class Network {
  private readonly neurons = new Map<string, Neuron>();
  private readonly pending = new Set<Promise<void>>();
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
   * returns each answer as a signal from the neuron that gave it.
   */
  async send(fromId: string, payload: Payload, { to }: { to?: string[] } = {}): Promise<Signal[]> {
    const { sender, targets } = this.resolve(fromId, to);
    const signal = newSignal(fromId, payload, targets.map((t) => t.id));
    this.options.onSignal?.(signal);
    const answers: Signal[] = [];
    for (const target of targets) {
      const answer = newSignal(target.id, await target.receive(signal), [sender.id]);
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
   * signal) and nothing more is reported for it except `onDrop`.
   */
  emit(fromId: string, payload: Payload, { to, replaces }: EmitOptions = {}): Signal {
    const { targets } = this.resolve(fromId, to);
    const signal = newSignal(fromId, payload, targets.map((t) => t.id));
    const controller = new AbortController();
    if (replaces !== undefined) {
      const key = `${fromId}\0${replaces}`;
      this.replaceable.get(key)?.abort();
      this.replaceable.set(key, controller);
    }
    this.options.onSignal?.(signal);
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
    try {
      const payload = await target.receive(signal, { abort });
      if (abort.aborted) return this.options.onDrop?.(signal, target.id);
      this.options.onSignal?.(newSignal(target.id, payload, [signal.from]));
    } catch (error) {
      // an aborted run is not an error, whatever the adapter threw
      if (abort.aborted) this.options.onDrop?.(signal, target.id);
      else this.options.onError?.(error, signal, target.id);
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
