// The typing demo: a search box emits the query so far, one letter at a time,
// to three neurons with different latencies. Each decides in its own adapter
// whether to act. A newer prefix replaces the older one, so slow work made
// stale is aborted and dropped. Run with `npm run demo:typing`.
// The catalogue is made up.
import { Network, Neuron, json, serialize, text } from '../dist/index.js';

const tracks = [
  { title: 'Harbor Lights', artist: 'Lumen Harbor' },
  { title: 'Lumen at Dawn', artist: 'Lumen Harbor' },
  { title: 'Paper Lanterns', artist: 'Tide and Timber' },
  { title: 'Timber Hymn', artist: 'Tide and Timber' },
];
const artists = ['Lumen Harbor', 'Tide and Timber'];
const lyrics = ['carry me home to the lumen harbor', 'paper lanterns on the tide'];

const sleep = (ms, abort) =>
  new Promise((resolve, reject) => {
    const timer = setTimeout(resolve, ms);
    abort?.addEventListener('abort', () => {
      clearTimeout(timer);
      reject(abort.reason);
    });
  });

// An adapter that waits `ms` (stopping early if aborted), then asks `look`
// for hits. `look` returns null to stay quiet.
const lookup = (ms, look) => ({
  async handle(payload, _signal, context) {
    const prefix = payload.text.toLowerCase();
    await sleep(ms, context?.abort);
    const hits = look(prefix);
    return hits ? json({ prefix: payload.text, hits }) : text('');
  },
});

const network = new Network({
  onSignal: (s) => {
    if (s.from === 'box' || serialize(s.payload) === '') return; // quiet neurons say nothing
    const { prefix, hits } = s.payload.data;
    console.log(`  answer   ${s.from.padEnd(7)} for "${prefix}": ${hits.length ? hits.join(' | ') : 'no hits'}`);
  },
  onDrop: (s, id) => console.log(`  dropped  ${id.padEnd(7)} for "${s.payload.text}" (stale)`),
  onError: (e, s, id) => console.log(`  error    ${id}: ${e.message}`),
});

network.add(new Neuron('box', lookup(0, () => null)));
// tracks: fast, acts from 3 characters
network.add(
  new Neuron(
    'tracks',
    lookup(40, (p) =>
      p.length < 3 ? null : tracks.filter((t) => `${t.title} ${t.artist}`.toLowerCase().includes(p)).map((t) => t.title),
    ),
  ),
);
// artist: quickest, acts only on an exact name
network.add(new Neuron('artist', lookup(20, (p) => (artists.some((a) => a.toLowerCase() === p) ? [p] : null))));
// lyrics: slow, acts from 5 characters
network.add(
  new Neuron(
    'lyrics',
    lookup(250, (p) => (p.length < 5 ? null : lyrics.filter((l) => l.includes(p)))),
  ),
);
for (const id of ['tracks', 'artist', 'lyrics']) network.connect('box', id);

const query = 'lumen harbor';
for (let i = 1; i <= query.length; i++) {
  const prefix = query.slice(0, i);
  console.log(`typed "${prefix}"`);
  network.emit('box', text(prefix), { replaces: 'query' });
  await sleep(100);
}
await network.idle();
