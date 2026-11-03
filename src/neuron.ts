import type { Adapter } from './adapter.js';
import { newSignal, type Payload, type Signal } from './signal.js';

/** A wrapper around anything, reachable through one interface. */
export class Neuron {
  readonly links = new Set<Neuron>();

  constructor(
    readonly id: string,
    readonly adapter: Adapter,
  ) {}

  /** Link both ways. */
  connect(other: Neuron): void {
    this.links.add(other);
    other.links.add(this);
  }

  /** Ask this neuron directly and get its answer. */
  query(payload: Payload): Promise<Payload> {
    return this.receive(newSignal(this.id, payload));
  }

  receive(signal: Signal): Promise<Payload> {
    return this.adapter.handle(signal.payload, signal);
  }
}
