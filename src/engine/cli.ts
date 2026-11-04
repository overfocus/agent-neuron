// npm run engine -- check <file>
// npm run engine -- run <file> --inputs <jsonl> [--trace <out.json>] [--inner <out.json>]
import { writeFileSync } from 'node:fs';
import { check, formatFinding, formatTrace, readInjections, runNetwork, type Event } from './index.js';

const usage = 'usage: engine check <file>\n       engine run <file> --inputs <jsonl> [--trace <out.json>] [--inner <out.json>]';
const [command, file, ...rest] = process.argv.slice(2);
const flag = (name: string) => {
  const i = rest.indexOf(name);
  return i < 0 ? undefined : rest[i + 1];
};

async function main(): Promise<number> {
  if (!file || (command !== 'check' && command !== 'run')) return (console.error(usage), 2);
  if (command === 'check') {
    const findings = check(file);
    for (const f of findings) console.error(formatFinding(f));
    const errors = findings.filter((f) => f.severity === 'error').length;
    console.log(errors ? `${file}: ${errors} error${errors > 1 ? 's' : ''}` : `${file}: ok`);
    return errors ? 1 : 0;
  }
  const inputs = flag('--inputs');
  if (!inputs) return (console.error(usage), 2);
  const inside: { path: string; event: Event }[] = [];
  const trace = await runNetwork(file, readInjections(inputs), { inner: (path, event) => inside.push({ path, event }) });
  const out = flag('--trace');
  if (out) writeFileSync(out, formatTrace(trace));
  if (flag('--inner')) writeFileSync(flag('--inner')!, JSON.stringify(inside) + '\n');
  // a run's result is the answers of the neurons with no outgoing links
  const leaves = new Set(trace.neurons.map((n) => n.id).filter((id) => !trace.links.some((l) => l.from === id)));
  for (const e of trace.events) if (e.type === 'answer' && leaves.has(e.from as string)) console.log(JSON.stringify({ neuron: e.from, t: e.t, payload: (e.payload as { data?: unknown; text?: string }).data ?? (e.payload as { text?: string }).text }));
  console.error(`ran ${trace.demo}: ${trace.events.length} events${out ? ` -> ${out}` : ''}`);
  return 0;
}

main().then(
  (code) => process.exit(code),
  (e: Error) => (console.error(`engine: ${e.message}`), process.exit(1)),
);
