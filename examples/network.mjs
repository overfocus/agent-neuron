// The alert storm, run from its network file by the engine: no neuron or routing code here.
// The engine (rust-engine, the `neuron-engine` binary) reads spec/examples/alerts.network.json, injects the 40 timed
// alerts of alerts.inputs.jsonl and writes a trace; the numbers below are read from that trace with the replay module
// (docs/TRACE.md, "Replay").
// Run with `npm run demo:network` (it builds the engine first); `-- <network file>` runs another, e.g. the Python variant.
// With a budget (`-- spec/examples/alerts-budget.network.json`) the producer's part grows by one line: it lowers the
// budget of every alert that is not critical when it injects it (docs/SPEC.md 10.2), and the summary reads the spend.
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { replay } from './trace/replay.mjs';

const engine = fileURLToPath(new URL('../rust-engine/target/release/neuron-engine', import.meta.url));
if (!existsSync(engine)) {
  console.error(`no engine at ${engine}\nbuild it first: cargo build --release --manifest-path rust-engine/Cargo.toml`);
  process.exit(1);
}

const example = (name) => fileURLToPath(new URL(`../spec/examples/${name}`, import.meta.url));
const file = process.argv[2] ?? example('alerts.network.json');
const { budget } = JSON.parse(readFileSync(file, 'utf8'));
const half = budget && Object.fromEntries(Object.entries(budget).map(([unit, n]) => [unit, Math.floor(n / 2)]));
const inputs = readFileSync(example('alerts.inputs.jsonl'), 'utf8').split('\n').filter((line) => line.trim()).map((line) => JSON.parse(line))
  .map((i) => (half && i.payload.severity !== 'crit' ? { ...i, budget: half } : i));

console.log(`Alert storm from ${file.split('/').pop()}: ${inputs.length} alerts over ${(inputs.at(-1).at / 1000).toFixed(1)} s, run by the engine.`);
const work = mkdtempSync(join(tmpdir(), 'network-demo-'));
let trace;
try {
  writeFileSync(join(work, 'inputs.jsonl'), inputs.map((i) => JSON.stringify(i)).join('\n') + '\n');
  const run = spawnSync(engine, ['run', file, '--inputs', join(work, 'inputs.jsonl'), '--trace', join(work, 'trace.json')], { encoding: 'utf8' });
  if (run.status !== 0) {
    console.error(run.stderr || run.error?.message || `the engine exited with ${run.status}`);
    process.exit(1);
  }
  trace = JSON.parse(readFileSync(join(work, 'trace.json'), 'utf8'));
} finally {
  rmSync(work, { recursive: true, force: true });
}
const { neurons: n } = replay(trace);
const flags = n.logs.answers + n.deploys.answers + n.metrics.answers;
const calls = n.hypothesis.acted;
const cut = calls - n.hypothesis.answers;
const verdicts = trace.events.filter((e) => e.type === 'answer' && e.from === 'diagnosis');

console.log(`${n.alerts.sent} alerts -> ${flags} flags -> ${calls} hypothesis calls instead of ${n.alerts.sent}`);
if (budget) {
  const sum = (list) => list.reduce((t, units) => Object.entries(units).reduce((u, [k, v]) => ({ ...u, [k]: (u[k] ?? 0) + v }), t), {});
  const show = (units) => Object.entries(units).map(([k, v]) => `${v} ${k}`).join(', ') || 'nothing';
  const cost = Object.fromEntries(trace.neurons.filter((x) => x.cost).map((x) => [x.id, x.cost]));
  const refused = trace.events.filter((e) => e.type === 'error' && e.code === 'budget');
  const finished = trace.events.filter((e) => e.type === 'finished');
  console.log(`budget ${show(budget)} per alert; the ${inputs.filter((i) => i.budget).length} that are not critical were injected with ${show(half)}`);
  console.log(`spent ${show(sum(trace.events.filter((e) => e.what === 'commit').map((e) => e.units)))}; ${refused.length} calls refused for lack of budget (${[...new Set(refused.map((e) => e.neuron))].map((id) => `${id} x${refused.filter((e) => e.neuron === id).length}`).join(', ') || 'none'}), avoiding ${show(sum(refused.map((e) => cost[e.neuron])))}`);
  console.log(`${finished.filter((e) => e.status === 'exhausted').length} of ${finished.length} traces finished exhausted; ${trace.events.filter((e) => e.what === 'reserve' && e.neuron === 'hypothesis').length} of ${calls} hypothesis calls ran`);
} else {
  console.log(`${cut} calls cut short as stale, ${n.hypothesis.dropped.replaced - cut} waiting flags replaced, ${n.hypothesis.dropped.expired ?? 0} gave up waiting`);
}
console.log(`${verdicts.length} diagnoses; the last: ${JSON.stringify(verdicts.at(-1)?.payload.data)}`);
console.log(`${trace.events.length} events in the trace; no inner id of diagnosis in it: ${!/"(door|topology|check)"/.test(JSON.stringify(trace))}`);
