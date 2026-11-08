// The scratch repo the team decides about: rebuilt from nothing on every run
// at /tmp/agent-neuron-team-demo (deleted first). One seed commit of a tiny
// invented module - a pier with a tide clock, a weather stub, a README - then
// two real branches, agent/patch-a and agent/patch-b, each with its own small
// diff in its own voice. Nothing here touches the repository this file lives
// in; the demo's git evidence is this repo alone.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const REPO = '/tmp/agent-neuron-team-demo';

const git = (args, cwd) => execFileSync('git', args, { cwd, encoding: 'utf8' }).trim();
const write = (rel, text) => {
  const p = path.join(REPO, rel);
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, text);
};

fs.rmSync(REPO, { recursive: true, force: true });
fs.mkdirSync(REPO, { recursive: true });
git(['init', '-q', '-b', 'main'], REPO);
git(['config', 'user.name', 'the pier team (invented)'], REPO);
git(['config', 'user.email', 'demo@agent-neuron.invalid'], REPO);

write(
  'pier/README.md',
  '# the pier\n\n' +
    'A tiny invented module for the team-decisions demo: a pier that opens\n' +
    'for boats when conditions allow. Two agent patches want to change what\n' +
    '"allow" means; the demo decides which one lands.\n',
);
write(
  'pier/tide.mjs',
  '// The tide clock, as seeded. Clears a boat when the water is over the ramp\n' +
    'and falling has slowed, by an invented table.\n' +
    'const CLEAR_M = 0.4; // metres of water under the keel\n' +
    'export function tideClearance(m) {\n' +
    '  return { clearance: Number((m - CLEAR_M).toFixed(2)), ok: m > CLEAR_M };\n' +
    '}\n',
);
write(
  'pier/weather.mjs',
  '// A weather stub, as seeded: no sensors, one invented forecast per run.\n' +
    'export function forecast() {\n' +
    '  return { wind: "calm", sea: "rippled", say: "good mooring weather" };\n' +
    '}\n',
);
git(['add', '-A'], REPO);
git(['commit', '-q', '-m', 'seed: the pier module - a tide clock, a weather stub and a README (invented)'], REPO);

// agent/patch-a, in its own voice: gate the pier by the tide.
git(['checkout', '-q', '-b', 'agent/patch-a'], REPO);
write(
  'pier/tide.mjs',
  '// The tide clock. Clears a boat when the water is over the ramp\n' +
    'and falling has slowed, by an invented table.\n' +
    'const CLEAR_M = 0.4; // metres of water under the keel\n' +
    'export function tideClearance(m) {\n' +
    '  return { clearance: Number((m - CLEAR_M).toFixed(2)), ok: m > CLEAR_M };\n' +
    '}\n' +
    '\n' +
    '// The tide gate (agent/patch-a): the pier is open only while the ramp\n' +
    '// is under water and the tide is not running out fast.\n' +
    'export function isPierOpen(m, fallPerHour) {\n' +
    '  return m > CLEAR_M && fallPerHour < 0.55;\n' +
    '}\n',
);
git(['add', '-A'], REPO);
git(['commit', '-q', '-m', 'patch-a: gate the pier by the tide - close it while the sea is over the ramp edge'], REPO);

// agent/patch-b, in its own voice: light the pier after dusk.
git(['checkout', '-q', 'main'], REPO);
git(['checkout', '-q', '-b', 'agent/patch-b'], REPO);
write(
  'pier/dusk.mjs',
  '// agent/patch-b: the dusk light. The pier closes when the lanterns go\n' +
    'up, not before: an invented solar hour, no sensors needed.\n' +
    'const LANTERN_HOUR = 19.5; // hours after midnight\n' +
    'export function isPierOpen(hoursAfterMidnight) {\n' +
    '  return hoursAfterMidnight < LANTERN_HOUR;\n' +
    '}\n',
);
git(['add', '-A'], REPO);
git(['commit', '-q', '-m', 'patch-b: light the pier after dusk - boats in when the lanterns go up'], REPO);

git(['checkout', '-q', 'main'], REPO);
console.log(`prepared ${REPO} on ${os.platform()}`);
console.log('branches:');
for (const line of git(['branch', '--list', 'main', 'agent/patch-a', 'agent/patch-b'], REPO).split('\n')) {
  console.log(`  ${line.trim()}`);
}
console.log(git(['log', '--oneline', '--graph', '--all', '--decorate'], REPO));
