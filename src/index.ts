// agent-neuron: a universal wrapper that adapts any software to one interface,
// linked many-to-many with other neurons.

export const VERSION = '0.0.0';

export { json, newSignal, parsePayload, serialize, text } from './signal.js';
export type { Link, Payload, Signal } from './signal.js';
export type { Adapter, AdapterContext } from './adapter.js';
export { Neuron } from './neuron.js';
export { Network } from './network.js';
export type { EmitOptions, NetworkOptions } from './network.js';
export { ShellAdapter, ShellError } from './adapters/shell.js';
export { HttpAdapter, HttpError } from './adapters/http.js';
export { HumanAdapter } from './adapters/human.js';
