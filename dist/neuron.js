import { newSignal } from './signal.js';
/** A wrapper around anything, reachable through one interface. */
export class Neuron {
    id;
    adapter;
    options;
    links = new Set();
    constructor(id, adapter, options = {}) {
        this.id = id;
        this.adapter = adapter;
        this.options = options;
    }
    /** Link both ways. */
    connect(other) {
        this.links.add(other);
        other.links.add(this);
    }
    /** Ask this neuron directly and get its answer. */
    query(payload) {
        return this.receive(newSignal(this.id, payload));
    }
    receive(signal, context) {
        return this.adapter.handle(signal.payload, signal, context);
    }
}
//# sourceMappingURL=neuron.js.map