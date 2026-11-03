import { Neuron } from './neuron.js';
import { newSignal, type Payload, type Signal } from './signal.js';

export type NetworkOptions = { onSignal?: (signal: Signal) => void };

export class Network {
  private readonly neurons = new Map<string, Neuron>();

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
    const sender = this.get(fromId);
    const targets = to
      ? to.map((id) => {
          const target = this.get(id);
          if (!sender.links.has(target)) throw new Error(`${fromId} is not linked to ${id}`);
          return target;
        })
      : [...sender.links];
    const signal = newSignal(fromId, payload, targets.map((t) => t.id));
    this.options.onSignal?.(signal);
    const answers: Signal[] = [];
    for (const target of targets) {
      const answer = newSignal(target.id, await target.receive(signal), [fromId]);
      this.options.onSignal?.(answer);
      answers.push(answer);
    }
    return answers;
  }

  private get(id: string): Neuron {
    const neuron = this.neurons.get(id);
    if (!neuron) throw new Error(`unknown neuron: ${id}`);
    return neuron;
  }
}
