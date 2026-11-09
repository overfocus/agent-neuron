#!/usr/bin/env node
// The peer layer's demo (decision 0013, docs/P2P.md): four engines, one run.
//
//   west    a whole network (classify.network.json) is one neuron
//   east    a whole network (review.network.json) is one neuron, and that
//           graph is itself spread: its `wrap` node runs on `helper`, so from
//           outside `review` is still a single neuron
//   helper  a plain worker for that one node of the east graph
//   driver  `run --peers`, the only engine that holds the outer network
//
// The launcher writes the files into a fresh directory, starts the engines on
// free ports (each peer prints the port it bound), runs the outer network and
// prints the answer, the trace it wrote, and where the run lives. Nothing is
// written into the repository.
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../..');
const TOKEN = 'a1b2c3d4e5f60718293a4b5c6d7e8f90';

function fail(why) {
  console.error(`p2p demo: ${why}`);
  process.exit(1);
}

function engine() {
  for (const c of [
    process.env.NEURON_ENGINE,
    join(repo, 'rust-engine/target/release/neuron-engine'),
    join(repo, 'rust-engine/target/debug/neuron-engine'),
  ]) {
    if (c && existsSync(c)) return c;
  }
  fail('no engine found: `cargo build --release --manifest-path rust-engine/Cargo.toml`, or set NEURON_ENGINE');
}

// -- the files the run is made of -------------------------------------------

const CLASSIFY = {
  version: 0,
  name: 'classify',
  neurons: [
    {
      id: 'label',
      description: 'tags the request',
      run: ['/bin/sh', '-c', 'read line; printf \'{"topic":"billing","request":%s}\' "$line"'],
    },
    {
      id: 'score',
      description: 'adds a score',
      run: ['/bin/sh', '-c', 'read line; printf \'{"scored":%s,"score":7}\' "$line"'],
    },
  ],
  links: [{ from: 'label', to: 'score' }],
  entry: 'label',
  exit: 'score',
};

const AUDIT = {
  version: 0,
  name: 'audit',
  neurons: [
    {
      id: 'stamp',
      description: 'in',
      run: ['/bin/sh', '-c', 'read line; printf \'{"audited":%s}\' "$line"'],
    },
    {
      id: 'sign',
      description: 'the final call',
      run: ['/bin/sh', '-c', 'read line; printf \'{"signed":%s}\' "$line"'],
    },
  ],
  links: [{ from: 'stamp', to: 'sign' }],
  entry: 'stamp',
  exit: 'sign',
};

const REVIEW = {
  version: 0,
  name: 'review',
  neurons: [
    {
      id: 'wrap',
      description: 'the reviewer, on its own engine',
      run: ['/bin/sh', '-c', 'read line; printf \'{"wrapped":%s}\' "$line"'],
    },
    {
      id: 'audit',
      description: 'a whole network, on a third engine',
      network: 'audit.network.json',
      timeout: 20000,
    },
  ],
  links: [{ from: 'wrap', to: 'audit' }],
  entry: 'wrap',
  exit: 'audit',
};

const OUTER = {
  version: 0,
  name: 'p2p-demo',
  neurons: [
    { id: 'intake', description: 'the request comes in', source: true },
    {
      id: 'classify',
      description: 'a whole graph, on the west engine',
      network: 'classify.network.json',
      timeout: 20000,
    },
    {
      id: 'review',
      description: 'a whole graph on the east engine, which is itself spread',
      network: 'review.network.json',
      timeout: 20000,
    },
  ],
  links: [
    { from: 'intake', to: 'classify' },
    { from: 'classify', to: 'review' },
  ],
};

function peersFile(self, name, port, placement) {
  return {
    version: 0,
    self,
    peers: [{ name, address: `127.0.0.1:${port}`, token: TOKEN }],
    placement,
  };
}

// -- the engines -------------------------------------------------------------

/// Start a peer on port 0 and wait for the port it printed: the readiness line
/// is `peer <name>: listening on 127.0.0.1:<port>`.
function startPeer(name, args) {
  const child = spawn(engine(), ['peer', ...args, '--token', TOKEN, '--name', name], {
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let seen = '';
  let pending = '';
  return new Promise((ok, bad) => {
    const timer = setTimeout(() => bad(new Error(`${name} never listened`)), 5000);
    child.stderr.on('data', (d) => {
      // Print whole lines only: a chunk can split a line in two.
      pending += String(d);
      const lines = pending.split('\n');
      pending = lines.pop() ?? '';
      for (const line of lines.filter(Boolean)) console.log(`  [${name}] ${line}`);
      seen += String(d);
      const m = seen.match(/listening on [^:\s]+:(\d+)/);
      if (m && timer) {
        clearTimeout(timer);
        ok({ name, port: Number(m[1]), child });
      }
    });
    child.stdout.on('data', (d) => process.stdout.write(`  [${name}] ${d}`));
    child.on('exit', (code) => bad(new Error(`${name} exited (${code}) before it listened`)));
  });
}

function runToExit(args, cwd) {
  return new Promise((ok) => {
    const child = spawn(engine(), args, { cwd, stdio: ['ignore', 'pipe', 'pipe'] });
    let out = '';
    let err = '';
    child.stdout.on('data', (d) => (out += d));
    child.stderr.on('data', (d) => (err += d));
    child.on('exit', (code) => ok({ code, out, err }));
  });
}

// -- the demo ----------------------------------------------------------------

const runDir = join(tmpdir(), `agent-neuron-p2p-${process.pid}-${Date.now()}`);
mkdirSync(runDir, { recursive: true });
const write = (name, value) => {
  const path = join(runDir, name);
  writeFileSync(path, typeof value === 'string' ? value : JSON.stringify(value, null, 2) + '\n');
  return path;
};

write('classify.network.json', CLASSIFY);
write('audit.network.json', AUDIT);
write('review.network.json', REVIEW);
write('outer.network.json', OUTER);
write('inputs.jsonl', JSON.stringify({ at: 0, source: 'intake', payload: { id: 7, text: 'the invoice is wrong' } }) + '\n');

console.log(`p2p demo: ${runDir}`);
console.log('starting engines:');

const started = [];
const stopAll = () => {
  for (const p of started) {
    try {
      p.child.kill('SIGTERM');
    } catch {
      // already gone
    }
  }
};
process.on('exit', stopAll);

try {
  // helper first: the east graph's `wrap` node lives there. north hosts the
  // graph that `review` places inside itself.
  const helper = await startPeer('helper', ['--listen', '127.0.0.1:0']);
  started.push(helper);
  const north = await startPeer('north', ['--listen', '127.0.0.1:0', '--network', join(runDir, 'audit.network.json')]);
  started.push(north);
  const eastPeers = write('peers.east.json', {
    version: 0,
    self: 'east',
    peers: [
      { name: 'helper', address: `127.0.0.1:${helper.port}`, token: TOKEN },
      { name: 'north', address: `127.0.0.1:${north.port}`, token: TOKEN },
    ],
    placement: { wrap: 'helper', audit: 'north' },
  });

  const east = await startPeer('east', [
    '--listen',
    '127.0.0.1:0',
    '--network',
    join(runDir, 'review.network.json'),
    '--peers',
    eastPeers,
  ]);
  started.push(east);

  const west = await startPeer('west', ['--listen', '127.0.0.1:0', '--network', join(runDir, 'classify.network.json')]);
  started.push(west);

  // The outer network places one node on each graph engine: three peers hold
  // three different things, and the driver holds only the file.
  const outerPeers = write('peers.outer.json', {
    version: 0,
    self: 'driver',
    peers: [
      { name: 'west', address: `127.0.0.1:${west.port}`, token: TOKEN },
      { name: 'east', address: `127.0.0.1:${east.port}`, token: TOKEN },
    ],
    placement: { classify: 'west', review: 'east' },
  });

  console.log('running the outer network:');
  const trace = join(runDir, 'trace.json');
  const run = await runToExit(
    ['run', 'outer.network.json', '--inputs', 'inputs.jsonl', '--peers', outerPeers, '--trace', trace],
    runDir,
  );
  if (run.err.trim()) console.log(`  [driver] ${run.err.trim().split('\n').join('\n  [driver] ')}`);
  if (run.code !== 0) fail(`the run exited ${run.code}`);

  const answer = run.out.trim();
  if (!answer.includes('"signed"') || !answer.includes('"audited"') || !answer.includes('"wrapped"')) {
    fail(`the answer is not the composed one: ${answer}`);
  }

  const doc = JSON.parse(readFileSync(trace, 'utf8'));
  const kinds = {};
  for (const ev of doc.events) kinds[ev.type] = (kinds[ev.type] ?? 0) + 1;

  console.log('');
  console.log('engines');
  console.log('  west    graph classify  (label -> score), one neuron to the outer run');
  console.log('  east    graph review    (wrap -> audit), one neuron to the outer run');
  console.log("  helper  review's wrap node: the east run is itself peer-to-peer");
  console.log('  north   graph audit     (stamp -> sign), placed by the graph on east');
  console.log('  driver  run --peers, holding only outer.network.json');
  console.log('');
  console.log(`answer    ${answer}`);
  console.log(`trace     ${trace}`);
  console.log(`events    ${doc.events.length} (${Object.entries(kinds).map(([k, n]) => `${k} ${n}`).join(', ')})`);
  console.log(`files     ${runDir}`);
} catch (e) {
  fail(e.message);
} finally {
  stopAll();
}
