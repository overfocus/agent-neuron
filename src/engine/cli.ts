// npm run engine -- check <file>
// npm run engine -- run <file> --inputs <jsonl> [--trace <out.json>] [--inner <out.json>]
import { writeFileSync } from 'node:fs';
import { check, formatFinding, formatTrace, killGroups, readInjections, runNetwork, type Event } from './index.js';

const usage = 'usage: engine check <file>\n       engine run <file> --inputs <jsonl> [--trace <out.json>] [--inner <out.json>]';
const [command, file, ...rest] = process.argv.slice(2);
const flag = (name: string) => {
  const i = rest.indexOf(name);
  return i < 0 ? undefined : rest[i + 1];
};

// Stopped from outside: take every running program's process group down with us (TERM, then KILL after 2 s).
let interrupted = false;
for (const signal of ['SIGINT', 'SIGTERM'] as const) {
  process.on(signal, () => {
    interrupted = true;
    killGroups('SIGTERM');
    setTimeout(() => (killGroups('SIGKILL'), process.exit(130)), 2000);
  });
}

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
  const inner = flag('--inner');
  const results: string[] = []; // the answers of the neurons with no outgoing links, whole (the trace cuts long payloads)
  const trace = await runNetwork(file, readInjections(inputs), {
    ...(inner && { inner: (path: string, event: Event) => inside.push({ path, event }) }),
    result: (neuron, t, payload) => results.push(JSON.stringify({ neuron, t, payload })),
  });
  const out = flag('--trace');
  if (out) writeFileSync(out, formatTrace(trace));
  if (inner) writeFileSync(inner, JSON.stringify(inside) + '\n');
  for (const line of results) console.log(line);
  console.error(`ran ${trace.demo}: ${trace.events.length} events${out ? ` -> ${out}` : ''}`);
  return 0;
}

main().then(
  (code) => process.exit(interrupted ? 130 : code),
  (e: Error) => (console.error(`engine: ${e.message}`), process.exit(1)),
);
