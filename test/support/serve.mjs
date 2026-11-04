// A scriptable level 1 neuron for the engine tests. argv: <log file> [flags]. It appends every message it receives to the log.
// Flags: many (maxInflight 2), stubborn (ignores SIGTERM), noping (never pongs), failrestart (once it has exited, every start exits).
// Payloads: {hold} waits for a cancel, then answers "late"; {hang} never replies; {helper} starts a background `sleep` that inherits stdout, then exits; {exit} exits (and notes it in the log); {say: [...]} writes each item (a string raw, an object with `re` filled in), else it answers the payload.
import { spawn } from 'node:child_process';
import { appendFileSync, existsSync, readFileSync } from 'node:fs';
import { createInterface } from 'node:readline';

const [log, ...flags] = process.argv.slice(2);
const record = (m) => appendFileSync(log, `${JSON.stringify(m)}\n`);
const send = (m) => process.stdout.write(`${typeof m === 'string' ? m : JSON.stringify(m)}\n`);
const exited = existsSync(log) && readFileSync(log, 'utf8').includes('"exit"');
if (flags.includes('failrestart') && exited) process.exit(1);
if (flags.includes('stubborn')) process.on('SIGTERM', () => {});
const held = new Set(); // calls waiting for a cancel
const rl = createInterface({ input: process.stdin });
send({ v: 1, type: 'ready', maxInflight: flags.includes('many') ? 2 : 1 });
rl.on('close', () => process.exit(0));
rl.on('line', (line) => {
  const m = JSON.parse(line);
  record(m.type === 'hello' ? m : { type: m.type, re: m.re, n: m.n });
  if (m.type === 'ping' && !flags.includes('noping')) send({ type: 'pong', n: m.n });
  if (m.type === 'cancel' && held.has(m.re)) send({ type: 'answer', re: m.re, payload: 'late' });
  if (m.type === 'signal' && m.payload?.hold) held.add(m.re);
  if (m.type !== 'signal' || m.payload?.hold || m.payload?.hang) return;
  if (m.payload?.helper) {
    spawn('sleep', ['5'], { stdio: ['ignore', 'inherit', 'ignore'] });
    process.exit(3);
  }
  if (m.payload?.exit) {
    record({ type: 'exit' });
    process.exit(3);
  }
  for (const x of m.payload?.say ?? []) send(typeof x === 'string' ? x : { re: m.re, ...x });
  if (!m.payload?.say) send({ type: 'answer', re: m.re, payload: m.inputs ?? m.payload }); // a threshold call gets `inputs`
});
