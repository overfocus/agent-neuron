// The reference engine for network files (docs/SPEC.md): `agent-neuron/engine`. The library in ../index.ts is separate.
export { check, formatFinding, readNetwork, validate, type Finding, type NetworkFile, type NeuronSpec } from './check.js';
export { formatTrace, killGroups, readInjections, runNetwork, type Desk, type Event, type Injection, type Question, type Trace } from './engine.js';
