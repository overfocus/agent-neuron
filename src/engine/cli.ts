// npm run engine -- check <file>
// npm run engine -- conform <dir> [--only <text>]
// npm run engine -- run <file> --inputs <jsonl> [--trace <out.json>] [--inner <out.json>] [--stream <path>]
import { closeSync, constants, existsSync, openSync, writeFileSync, writeSync } from 'node:fs';
import { conform, report } from './conform.js';
import { check, formatFinding, formatTrace, killGroups, readInjections, runNetwork, type Event } from './index.js';

const STREAM_GRACE = 1000; // ms the CLI waits for the stream's reader once the run is over
const usage = 'usage: engine check <file>\n       engine conform <dir> [--only <text>]\n       engine run <file> --inputs <jsonl> [--trace <out.json>] [--inner <out.json>] [--stream <path>]\n       (run prints the answers of neurons with no outgoing links, a stale effect call\'s included; --stream writes every event as a JSON line, a file or a pipe)';
const [command, file, ...rest] = process.argv.slice(2);
const flag = (name: string) => {
  const i = rest.indexOf(name);
  return i < 0 ? undefined : rest[i + 1];
};

/**
 * The live stream's sink (SPEC 9.1): one JSON line per event, to a file or a pipe, written without ever blocking a thread: the file is opened
 * non-blocking, so a pipe with no reader is refused at once and one whose reader stalls just keeps the lines here. `close` gives the reader
 * STREAM_GRACE ms to take what is left, then cuts the stream (says so once) so the process can exit. A failure is reported once and the run goes on.
 */
function openSink(path: string) {
  let fd: number | undefined, chunks: Buffer[] = [], timer: NodeJS.Timeout | undefined;
  const fail = (why: string) => {
    if (fd !== undefined) { try { closeSync(fd); } catch { /* gone */ } }
    if (fd !== undefined || chunks.length === 0) console.error(`engine: --stream ${path}: ${why}`);
    fd = undefined; chunks = []; clearInterval(timer);
  };
  try { fd = openSync(path, constants.O_WRONLY | constants.O_NONBLOCK | constants.O_CREAT | constants.O_TRUNC); } catch (e) { console.error(`engine: --stream ${path}: ${(e as Error).message} (a pipe needs a reader already open)`); }
  const pump = () => {
    while (fd !== undefined && chunks.length) {
      try { const n = writeSync(fd, chunks[0]!); chunks[0] = chunks[0]!.subarray(n); if (!chunks[0]!.length) chunks.shift(); } catch (e) {
        if ((e as NodeJS.ErrnoException).code === 'EAGAIN') return;
        return fail((e as Error).message);
      }
    }
  };
  return {
    write(line: string) {
      if (fd === undefined) return;
      chunks.push(Buffer.from(line));
      pump();
      if (chunks.length && !timer) timer = setInterval(pump, 10);
    },
    async close() {
      for (let waited = 0; fd !== undefined && chunks.length && waited < STREAM_GRACE; waited += 10) await new Promise((r) => setTimeout(r, 10));
      clearInterval(timer);
      if (fd === undefined) return;
      if (chunks.length) fail(`cut after ${STREAM_GRACE} ms: its reader did not take the rest`);
      else closeSync(fd);
    },
  };
}

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
  if (!file || (command !== 'check' && command !== 'run' && command !== 'conform')) return (console.error(usage), 2);
  if (command === 'conform') {
    if (!existsSync(file)) return (console.error(`engine: no such directory: ${file}`), 2);
    const only = flag('--only');
    if (rest.includes('--only') && (only === undefined || only.startsWith('--'))) return (console.error(usage), 2);
    const results = await conform(file, only === undefined ? {} : { only });
    if (!results.length) return (console.error(`engine: no fixtures under ${file}`), 2);
    for (const line of report(results)) console.log(line);
    return results.some((r) => r.status === 'fail') ? 1 : 0;
  }
  if (command === 'check') {
    const findings = check(file);
    for (const f of findings) console.error(formatFinding(f));
    const errors = findings.filter((f) => f.severity === 'error').length;
    console.log(errors ? `${file}: ${errors} error${errors > 1 ? 's' : ''}` : `${file}: ok`);
    return errors ? 1 : 0;
  }
  const inputs = flag('--inputs'), stream = flag('--stream');
  if (!inputs || (rest.includes('--stream') && (stream === undefined || stream.startsWith('--')))) return (console.error(usage), 2);
  const sink = stream === undefined ? undefined : openSink(stream);
  const inside: { path: string; event: Event }[] = [];
  const inner = flag('--inner');
  const results: string[] = []; // the answers of the neurons with no outgoing links, whole (the trace cuts long payloads)
  const trace = await runNetwork(file, readInjections(inputs), {
    ...(inner && { inner: (path: string, event: Event) => inside.push({ path, event }) }),
    result: (neuron, t, payload) => results.push(JSON.stringify({ neuron, t, payload })),
    ...(sink && { onEvent: (event: Event) => sink.write(`${JSON.stringify(event)}\n`) }),
  });
  const out = flag('--trace');
  if (out) writeFileSync(out, formatTrace(trace));
  if (inner) writeFileSync(inner, JSON.stringify(inside) + '\n');
  for (const line of results) console.log(line); // a stale effect call's answer is among them: it is recorded, not forwarded (SPEC 11.2)
  console.error(`ran ${trace.demo}: ${trace.events.length} events${out ? ` -> ${out}` : ''}`);
  // Only now wait for the stream, and not for ever: a pipe whose reader never reads must not hold back what the run produced (SPEC 9.1).
  await sink?.close();
  return 0;
}

main().then(
  (code) => process.exit(interrupted ? 130 : code),
  (e: Error) => (console.error(`engine: ${e.message}`), process.exit(1)),
);
