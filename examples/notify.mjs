// A side-effecting neuron: `notify` sends a message, and is an effect (docs/SPEC.md 11). Three build updates arrive for one
// key; the second replaces the first while its message is being sent, and the third loses its connection after sending.
// Prints the live stream as it happens (SPEC 9.1), then runs the same network with `effect` removed to show the difference.
// Run with `npm run demo:notify` (it builds first).
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readInjections, runNetwork } from '../dist/engine/index.js';

const dir = fileURLToPath(new URL('../spec/examples/', import.meta.url));
const inputs = readInjections(join(dir, 'notify.inputs.jsonl'));
const work = mkdtempSync(join(tmpdir(), 'notify-demo-'));

/** One run; `effect` false runs a copy of the file with the option removed. Prints each event of interest as it happens. */
async function go(effect) {
  const net = JSON.parse(readFileSync(join(dir, 'notify.network.json'), 'utf8'));
  for (const n of net.neurons) { if (!effect) delete n.effect; if (n.run) n.cwd = dir; }
  const file = join(work, `${effect ? 'effect' : 'plain'}.network.json`);
  writeFileSync(file, JSON.stringify(net));
  process.env.SENT_LOG = join(work, `${effect ? 'effect' : 'plain'}.sent`);
  const say = (e, text) => console.log(`${String(e.t).padStart(6)} ms  ${text}`);
  // The id is short and at the top of the payload; a payload over 2000 characters is cut to a preview, so fall back to reading the id out of it.
  const id = (e) => e.payload?.data?.id ?? e.payload?.preview?.match(/"id":"([^"]+)"/)?.[1] ?? e.payload?.data;
  const why = (e) => e.message.trim().replace(/:$/, '');
  const stream = (e) => {
    if (e.type === 'signal' && e.from === 'builds') say(e, `build ${id(e)} arrives (key ${e.replaces})`);
    else if (e.type === 'drop') say(e, `notify is cut: the message for ${id(e)} was being sent (${e.reason})`);
    else if (e.type === 'answer' && e.from === 'notify') say(e, `notify answers ${e.payload.data.sent}: ${e.forwarded === false ? 'recorded, not forwarded (its input is stale)' : 'forwarded to archive'}`);
    else if (e.type === 'unknown') say(e, `notify is UNKNOWN for ${id(e)} (${why(e)}): the message may have been sent`);
    else if (e.type === 'error') say(e, `notify FAILED for ${id(e)} (${why(e)}): a producer that retries would send it twice`);
    else if (e.type === 'answer' && e.from === 'lookup') say(e, `reconciled: ${e.payload.data.id} was ${e.payload.data.sent ? '' : 'not '}sent`);
  };
  // The reconciliation pattern: an observer sees the unknown and injects the build id into the declared source `reconcile`.
  const observer = (e) => (e.type === 'unknown' ? [{ at: 50, source: 'reconcile', payload: { id: id(e) } }] : []);
  console.log(effect ? 'With effect (the network file as written):' : '\nThe same network with `effect` removed:');
  const trace = await runNetwork(file, inputs, { onEvent: stream, observer });
  const sent = readFileSync(process.env.SENT_LOG, 'utf8').trim().split('\n');
  console.log(`messages sent: ${sent.join(', ')}; notify events: ${trace.events.filter((e) => e.neuron === 'notify' || e.from === 'notify').map((e) => e.type).filter((t) => t !== 'signal' && t !== 'note').join(', ')}`);
  return trace;
}

console.log(`Notify demo: ${inputs.length} build updates for one key; notify takes 300 ms to send a message.\n`);
await go(true);
await go(false);
