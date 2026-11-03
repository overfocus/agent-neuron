// The composed-neuron demo: the typing demo's search box sends every keystroke
// to ONE neuron, `music`. Inside it (rust/music) is a small network of tracks,
// artist and lyrics neurons; this network cannot see them and they cannot see
// it. A signal goes in and an answer comes out. Then, to show the other side,
// the same program is run on its own with --inspect: the view from inside.
// Needs a Rust toolchain. Run with `npm run demo:composed` (it builds first).
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { Network, Neuron, ShellAdapter, serialize, text } from '../dist/index.js';
import { sleep } from './typing-demo.mjs';

const binary = fileURLToPath(new URL('../rust/target/release/music', import.meta.url));
const typed = new Map(); // what the box emitted, by signal id
const query = 'lumen harbor';

const network = new Network({
  onSignal: (s) => {
    if (s.from === 'box' || serialize(s.payload) === '') return; // music stays quiet below its threshold
    const { tracks, artist, lyrics } = s.payload.data;
    console.log(`  answer   music for "${typed.get(s.replyTo)}": tracks ${tracks.length ? tracks.join(' | ') : 'none'}; artist ${artist ?? '-'}; lyrics ${lyrics ?? '-'}`);
  },
  onDrop: (s, id) => console.log(`  dropped  ${id} for "${typed.get(s.id)}" (stale)`),
  onError: (e, s, id) => console.log(`  error    ${id}: ${e.message}`),
});
network.add(new Neuron('box', { handle: async () => text('') }));
network.add(new Neuron('music', new ShellAdapter(binary)));
network.connect('box', 'music');

for (let i = 1; i <= query.length; i++) {
  const prefix = query.slice(0, i);
  console.log(`typed "${prefix}" -> music`);
  typed.set(network.emit('box', text(prefix), { replaces: 'query' }).id, prefix);
  await sleep(100);
}
await network.idle();

// Not the outer network reaching in: a separate run of the program, looking inside.
console.log(`\nInside view: music --inspect on "${query}" (its own inner trace, not visible from the network above)`);
const inside = spawnSync(binary, ['--inspect'], { input: query, encoding: 'utf8' });
console.log(inside.stderr.trimEnd().replace(/^/gm, '  '));
console.log('Each call is its own process: nothing is learned or shared between calls.');
process.exit(inside.status ?? 1);
