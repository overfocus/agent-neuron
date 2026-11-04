// The alert storm again, run from its network file by the engine: no neuron or routing code here.
// The engine reads spec/examples/alerts.network.json, injects the 40 timed alerts of alerts.inputs.jsonl, and returns a
// trace; the numbers below are read from that trace with the replay module (docs/TRACE.md, "Replay").
// Run with `npm run demo:network` (it builds first); `-- <network file>` runs another, e.g. the Python variant.
import { fileURLToPath } from 'node:url';
import { formatTrace, readInjections, runNetwork } from '../dist/engine/index.js';
import { replay } from './trace/replay.mjs';

const example = (name) => fileURLToPath(new URL(`../spec/examples/${name}`, import.meta.url));
const file = process.argv[2] ?? example('alerts.network.json');
const inputs = readInjections(example('alerts.inputs.jsonl'));

console.log(`Alert storm from ${file.split('/').pop()}: ${inputs.length} alerts over ${(inputs.at(-1).at / 1000).toFixed(1)} s, run by the engine.`);
const trace = JSON.parse(formatTrace(await runNetwork(file, inputs)));
const { neurons: n } = replay(trace);
const flags = n.logs.answers + n.deploys.answers + n.metrics.answers;
const calls = n.hypothesis.acted;
const cut = calls - n.hypothesis.answers;
const verdicts = trace.events.filter((e) => e.type === 'answer' && e.from === 'diagnosis');

console.log(`${n.alerts.sent} alerts -> ${flags} flags -> ${calls} hypothesis calls instead of ${n.alerts.sent}`);
console.log(`${cut} calls cut short as stale, ${n.hypothesis.dropped.replaced - cut} waiting flags replaced, ${n.hypothesis.dropped.expired ?? 0} gave up waiting`);
console.log(`${verdicts.length} diagnoses; the last: ${JSON.stringify(verdicts.at(-1)?.payload.data)}`);
console.log(`${trace.events.length} events in the trace; no inner id of diagnosis in it: ${!/"(door|topology|check)"/.test(JSON.stringify(trace))}`);
