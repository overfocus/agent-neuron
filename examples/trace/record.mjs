// Records a demo's run as a trace file (format: docs/TRACE.md).
//   npm run record -- <alerts|typing|suggest> [--threshold <n>] [--out <file>]     (builds first; default out: traces/<demo>.json)
// --threshold (alerts only): detectors that must agree before hypothesis acts; default 2.
// Record on an idle machine: the numbers in the file are what that run produced.
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { alertsScenario, scenarios } from './scenarios.mjs';
import { tap } from './tap.mjs';

export const FORMAT = 'agent-neuron-trace';
export const VERSION = 1;
/** A payload longer than this many characters (as text, or as JSON text) is cut to a preview of this many. */
export const PAYLOAD_CAP = 2000;

const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

export function capPayload(p) {
  const raw = p.type === 'text' ? p.text : JSON.stringify(p.data);
  if (raw.length <= PAYLOAD_CAP) return p;
  const preview = raw.slice(0, PAYLOAD_CAP);
  return p.type === 'text' ? { type: 'text', text: preview, truncated: true } : { type: 'json', preview, truncated: true };
}

/** Runs `demo` on `lib` (the built library) and returns the trace object. */
export async function record(demo, lib, { threshold } = {}) {
  if (!Object.hasOwn(scenarios, demo)) throw new Error(`unknown demo "${demo}"; choose one of: ${Object.keys(scenarios).join(', ')}`);
  if (threshold !== undefined && demo !== 'alerts') throw new Error('--threshold applies to alerts only');
  const scenario = threshold === undefined ? scenarios[demo] : alertsScenario(threshold);
  const events = [];
  let failure;
  const { listeners, fail } = tap((e) => events.push(e.payload ? { ...e, payload: capPayload(e.payload) } : e));
  const result = await scenario.run(listeners, lib).catch((e) => ((failure = e), fail(e)));
  if (failure) throw failure;
  // The declaration must match the neurons the run built, or the trace would describe something else.
  const built = result.neurons.map((n) => ({ id: n.id, ...(Object.keys(n.options).length && { options: n.options }) }));
  const declared = scenario.neurons.map((n) => ({ id: n.id, ...(n.options && { options: n.options }) }));
  if (!same(built, declared)) throw new Error(`the ${demo} declaration in scenarios.mjs does not match the run: ${JSON.stringify(built)} vs ${JSON.stringify(declared)}`);
  // Links are two-way in the library; compare them as unordered pairs.
  const pair = (a, b) => [a, b].sort().join('\0');
  const builtLinks = result.neurons.flatMap((n) => [...n.links].map((o) => pair(n.id, o.id)));
  const declaredLinks = scenario.links.map((l) => pair(l.from, l.to));
  if (!same([...new Set(builtLinks)].sort(), [...new Set(declaredLinks)].sort())) throw new Error(`the ${demo} links in scenarios.mjs do not match the run`);
  return {
    format: FORMAT,
    version: VERSION,
    demo,
    description: scenario.description,
    library: lib.VERSION,
    ...(scenario.parameters && { parameters: scenario.parameters }),
    neurons: scenario.neurons,
    links: scenario.links,
    events,
    summary: scenario.summary(result),
  };
}

// Readable and diffable: one neuron or event per line.
const list = (items) => `[\n${items.map((x) => `    ${JSON.stringify(x)}`).join(',\n')}\n  ]`;
export const format = (t) =>
  `{\n${Object.entries(t)
    .map(([k, v]) => `  ${JSON.stringify(k)}: ${k === 'neurons' || k === 'links' || k === 'events' ? list(v) : JSON.stringify(v)}`)
    .join(',\n')}\n}\n`;

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const args = process.argv.slice(2);
  const at = args.indexOf('--out');
  const out = at >= 0 ? args.splice(at, 2)[1] : undefined;
  const th = args.indexOf('--threshold');
  const threshold = th >= 0 ? Number(args.splice(th, 2)[1]) : undefined;
  const [demo, ...rest] = args;
  if (!demo || rest.length || (at >= 0 && !out) || (th >= 0 && !(Number.isInteger(threshold) && threshold >= 1))) {
    console.error(`usage: npm run record -- <${Object.keys(scenarios).join('|')}> [--threshold <n>] [--out <file>]`);
    process.exit(2);
  }
  const file = path.resolve(out ?? fileURLToPath(new URL(`../../traces/${demo}.json`, import.meta.url)));
  try {
    const trace = await record(demo, await import('../../dist/index.js'), { threshold });
    await mkdir(path.dirname(file), { recursive: true });
    await writeFile(file, format(trace));
    console.log(`recorded ${demo}: ${trace.events.length} events -> ${path.relative(process.cwd(), file)}`);
  } catch (e) {
    console.error(`record: ${e.message}`);
    process.exit(1);
  }
}
