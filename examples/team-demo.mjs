// demo:team — the team-decisions orchestrator. Runs prepare-repo (a fresh
// scratch repo with two real agent branches), spawns the Rust engine over
// examples/team/team.network.json from the repo root, captures the desk
// stderr line, starts the multiplayer pageserver, prints the three person
// URLs (one per role), prints every desk answer line and the direction and
// merge lines as they land, and — when the run settles — prints the final
// report: the git evidence before and after (branch graph, merge commit,
// tag), the full decision ledger, and the trace path.
//
//   npm run demo:team                 # people answer on the pages
//   npm run demo:team -- --headless   # ada, lin (both patch-a), then sam (merge)
//   npm run demo:team -- --headless --tie   # lin votes patch-b: tie, lead refuses
//
// The --headless answers go through the pageserver's POST path (the same one
// the page uses), so the ledger records the three people in both modes.
import { execFileSync, spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const scratch = '/tmp/agent-neuron-team-demo';
const tracePath = '/tmp/agent-neuron-team-demo-trace.json';
const streamPath = '/tmp/agent-neuron-team-demo-stream.jsonl';

const args = process.argv.slice(2);
const headless = args.includes('--headless');
const tie = args.includes('--tie');
const IC_VOTES = tie ? ['agent/patch-a', 'agent/patch-b'] : ['agent/patch-a', 'agent/patch-a'];
const IC_WHYS = tie
  ? ['the tide gate reads like the ramp already behaves', 'I trust the lanterns more than the tide table']
  : ['the tide gate reads like the ramp already behaves', 'the gate is measurable in the field, dusk is not'];

const say = (line) => process.stdout.write(`${line}\n`);
const die = (why, code = 1) => {
  say(`team demo: ${why}`);
  process.exit(code);
};

// -- The engine binary, fresh against its sources ----------------------------

const ENGINE = path.join(root, 'rust-engine/target/debug/neuron-engine');
const newestUnder = (dir, acc = 0) =>
  fs
    .readdirSync(dir, { withFileTypes: true })
    .reduce((acc, d) => {
      const p = path.join(dir, d.name);
      return d.isDirectory() ? Math.max(acc, newestUnder(p)) : Math.max(acc, fs.statSync(p).mtimeMs);
    }, acc);
const engineMtime = () => fs.statSync(ENGINE).mtimeMs;
if (!fs.existsSync(ENGINE) || newestUnder(path.join(root, 'rust-engine/src')) > engineMtime()) {
  say('team demo: building the engine (cargo build, rust-engine/)...');
  const built = spawnSync('cargo', ['build', '--manifest-path', 'rust-engine/Cargo.toml'], { cwd: root, stdio: 'inherit' });
  if (built.status !== 0 || !fs.existsSync(ENGINE)) die('the engine binary is missing: run cargo build in rust-engine/', 2);
}

// -- 1. The scratch repo, fresh; the branch graph before the decision -------------

const prep = spawnSync('node', [path.join('examples', 'team', 'prepare-repo.mjs')], { cwd: root, stdio: 'inherit' });
if (prep.status !== 0) die('prepare-repo failed', 2);
say('\nbranch graph, before the decision:');
say(execFileSync('git', ['-C', scratch, 'log', '--oneline', '--graph', '--all', '--decorate'], { encoding: 'utf8' }));

// -- 2. The engine, its desk, its stream -------------------------------------------

fs.rmSync(tracePath, { force: true });
fs.rmSync(streamPath, { force: true });
const engine = spawn(ENGINE, [
  'run',
  'examples/team/team.network.json',
  '--inputs',
  'examples/team/team.inputs.jsonl',
  '--trace',
  tracePath,
  '--stream',
  streamPath,
], { cwd: root });

const deskAnswerLines = [];
let engineDone;
const engineEnded = new Promise((resolve) => (engineDone = resolve));
engine.on('exit', (code, signal) => engineDone({ code, signal }));

let deskPort = 0;
let deskToken = '';
const stderrDone = new Promise((resolve) => {
  readline.createInterface({ input: engine.stderr }).on('line', (line) => {
    if (line.startsWith('desk: ')) {
      const m = line.match(/^desk: http:\/\/127\.0\.0\.1:(\d+)\/\?token=([0-9a-f]+)$/);
      if (!m) die(`the desk line is not the shape the demo reads: ${line}`);
      deskPort = Number(m[1]);
      deskToken = m[2];
    }
    if (line.startsWith('desk answer: ')) deskAnswerLines.push(line);
    say(line);
  }).on('close', resolve);
});
const engineOut = [];
readline.createInterface({ input: engine.stdout }).on('line', (line) => engineOut.push(line));

const deskLine = await Promise.race([
  new Promise((resolve) => {
    const t = setInterval(() => {
      if (deskPort) {
        clearInterval(t);
        resolve(true);
      }
    }, 20);
    setTimeout(() => {
      if (!deskPort) {
        clearInterval(t);
        resolve(false);
      }
    }, 10000);
  }),
  engineEnded.then(() => false),
]);
if (!deskPort) {
  engine.kill('SIGTERM');
  await engineEnded;
  await stderrDone;
  die('the engine never announced its desk');
}

// -- 3. The multiplayer pageserver ------------------------------------------------

const pageserver = spawn('node', [path.join('examples', 'team', 'pageserver.mjs')], {
  cwd: root,
  env: { ...process.env, TEAM_DEMO_DESK_PORT: String(deskPort), TEAM_DEMO_DESK_TOKEN: deskToken },
  stdio: ['ignore', 'pipe', 'inherit'],
});
let pagePort = 0;
readline.createInterface({ input: pageserver.stdout }).on('line', (line) => {
  const m = line.match(/^team: pageserver on http:\/\/127\.0\.0\.1:(\d+)/);
  if (m) pagePort = Number(m[1]);
  say(line);
});
// (pageserver stderr rides inherited with its own line, straight to the terminal)
await new Promise((resolve) => {
  const t = setInterval(() => {
    if (pagePort) {
      clearInterval(t);
      resolve(true);
    }
  }, 20);
  setTimeout(() => {
    if (!pagePort) die('the pageserver never announced its port');
  }, 10000);
});

say('\nopen the desks (two of these in separate browser windows; one person per role):');
for (const [key, who] of [
  ['ada', 'Ada (role ic)'],
  ['lin', 'Lin (role ic)'],
  ['sam', 'Sam (the lead)'],
]) {
  say(`  ${who}: http://127.0.0.1:${pagePort}/p/${key}`);
}
say(`  shared ledger: http://127.0.0.1:${pagePort}/log`);

// -- 4. The live stream, tailed for the prints the demo promises --------------------

const streamLines = [];
let tailPos = 0;
let tailBuf = '';
let tailStop = false;
const tailer = setInterval(() => {
  if (tailStop) return;
  try {
    if (!fs.existsSync(streamPath)) return;
    const size = fs.statSync(streamPath).size;
    if (size <= tailPos) return;
    const fd = fs.openSync(streamPath, 'r');
    const chunk = Buffer.alloc(size - tailPos);
    fs.readSync(fd, chunk, 0, chunk.length, tailPos);
    fs.closeSync(fd);
    tailPos = size;
    tailBuf += chunk.toString('utf8');
    const lines = tailBuf.split('\n');
    tailBuf = lines.pop();
    for (const line of lines) {
      if (!line.trim()) continue;
      let ev;
      try {
        ev = JSON.parse(line);
      } catch {
        continue;
      }
      streamLines.push(ev);
      if (ev.type === 'answer' && ev.from === 'direction') say(`direction: ${ev.payload?.text ?? JSON.stringify(ev.payload)}`);
      else if (ev.type === 'answer' && ev.from === 'apply') say(`merge outcome: ${ev.payload?.text ?? JSON.stringify(ev.payload)}`);
      else if (ev.type === 'unknown' || ev.type === 'error') say(`engine: ${ev.type} ${ev.neuron}: ${ev.message}`);
      else if (ev.type === 'drop' && ev.reason === 'expired') say(`engine: a held input expired at ${ev.neuron} (the join never fired)`);
      else if (ev.type === 'drop') say(`engine: drop at ${ev.neuron}: ${ev.reason}${ev.phase ? ` (${ev.phase})` : ''}`);
    }
  } catch {
    // a torn read across a write: the next tick catches up
  }
}, 80);

// -- 5. --headless: the three answers, through the pageserver's POST path -----------

const pageCall = (route, body) =>
  new Promise((resolve, reject) => {
    const req = http.request(
      {
        host: '127.0.0.1',
        port: pagePort,
        path: route,
        method: body === undefined ? 'GET' : 'POST',
        headers: {
          host: `127.0.0.1:${pagePort}`,
          ...(body !== undefined && { 'content-type': 'application/json', 'content-length': Buffer.byteLength(body) }),
          connection: 'close',
        },
      },
      (res) => {
        let raw = '';
        res.setEncoding('utf8');
        res.on('data', (c) => (raw += c));
        res.on('end', () => resolve({ status: res.statusCode, text: raw }));
      },
    );
    req.on('error', reject);
    if (body !== undefined) req.write(body);
    req.end();
  });

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const waitPersonItem = async (who, budgetMs) => {
  const deadline = Date.now() + budgetMs;
  while (Date.now() < deadline) {
    const r = await pageCall(`/p/${who}/state`).catch(() => null);
    if (r) {
      const state = JSON.parse(r.text);
      const waiting = (state.items || []).find((i) => i.status === 'waiting');
      if (waiting) return waiting;
      if (state.closed && !state.items?.length) throw new Error(`${who}'s desk is already closed`);
    }
    await sleep(120);
  }
  throw new Error(`${who} was never asked`);
};

const answerAs = async (who, body) => {
  const r = await pageCall(`/p/${who}/answer`, JSON.stringify(body));
  const j = JSON.parse(r.text || '{}');
  if (r.status !== 200) die(`${who}'s answer was refused (${r.status}): ${JSON.stringify(j)}`);
  return j;
};

async function driveHeadless() {
  say('\nheadless: answering as all three people, through the pageserver (no browser needed)');
  const adaItem = await waitPersonItem('ada', 60000);
  say(`headless: Ada votes ${IC_VOTES[0]}`);
  await answerAs('ada', { item: adaItem.id, choice: 'vote', branch: IC_VOTES[0], why: IC_WHYS[0] });
  const linItem = await waitPersonItem('lin', 60000);
  say(`headless: Lin votes ${IC_VOTES[1]}`);
  await answerAs('lin', { item: linItem.id, choice: 'vote', branch: IC_VOTES[1], why: IC_WHYS[1] });
  const gate = await waitPersonItem('sam', 60000);
  if (tie) {
    say('headless: the vote tied, Sam refuses (no merge)');
    await answerAs('sam', { item: gate.id, choice: 'refuse' });
  } else {
    const branch = gate.payload?.options?.[0]?.branch;
    say(`headless: quorum for ${branch}, Sam gates the merge`);
    await answerAs('sam', { item: gate.id, choice: 'merge', branch });
  }
}

if (headless) {
  const watchdog = setTimeout(() => {
    die('headless: the run never settled in 90 s', 3);
  }, 90000);
  watchdog.unref();
  await driveHeadless();
} else {
  say('waiting for the three people to answer; the demo ends when the run settles (its last answer kills the desk)');
}

// -- 6. The run settles ------------------------------------------------------------

const outcome = await engineEnded;
tailStop = true;
clearInterval(tailer);
await new Promise((resolve) => setTimeout(resolve, 250));
await Promise.race([stderrDone, sleep(2000)]);

say(`\nengine exited (code ${outcome?.code}${outcome?.signal ? `, ${outcome.signal}` : ''}). Final report:`);
say('\ndecision record, the engine stderr desk answer lines:');
for (const line of deskAnswerLines) say(`  ${line}`);
say('\nengine stdout (the printed answers of the neurons with no outgoing links):');
if (engineOut.length) for (const line of engineOut) say(`  ${line}`);
else say('  (none: every answer was forwarded, or the gate refused - a quiet prints nothing)');

// The ledger, read back from the pageserver while it still runs.
let ledger = null;
for (let i = 0; i < 5 && !ledger; i++) {
  const r = await pageCall('/log').catch(() => null);
  if (r?.status === 200) ledger = JSON.parse(r.text);
  else await sleep(300);
}
say('\ndecision ledger (the names are the pageserver\'s, never the protocol\'s):');
if (ledger) {
  if (!ledger.lines.length) say('  (empty)');
  for (const line of ledger.lines) say(`  ${line.who} - ${line.kind} - ${line.text || '(refused)'} - item ${line.item}`);
  say(`  quorum: ${JSON.stringify(ledger.quorum)}`);
} else {
  say('  (the pageserver was already gone; its ledger lines printed above as they landed)');
}

say('\ngit evidence, after (the scratch repo):');
say(execFileSync('git', ['-C', scratch, 'log', '--oneline', '--graph', '--all', '--decorate'], { encoding: 'utf8' }));
const tags = execFileSync('git', ['tag', '-n1'], { cwd: scratch, encoding: 'utf8' }).trim();
if (tags) {
  say('the tag:');
  say(`  ${tags}`);
  say('the merge commit stat:');
  say(execFileSync('git', ['-C', scratch, 'show', '--stat', '--format=medium', 'team-decision'], { encoding: 'utf8' }));
} else {
  say('no tag, no merge commit: the gate was refused (the ledger records it)');
}
say(`\ntrace: ${tracePath}`);
say(`stream: ${streamPath}`);
pageserver.kill('SIGTERM');
process.exit(0);
