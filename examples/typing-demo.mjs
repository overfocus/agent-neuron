// Shared setup for the typing demos (typing.mjs, rust.mjs): a made-up
// catalogue, the neurons around the tracks neuron, and the printing.
import { Network, Neuron, json, serialize, text } from '../dist/index.js';

export const tracks = [
  { title: 'Glass Orchard', artist: 'Lumen Harbor' },
  { title: 'Ninefold Morning', artist: 'Lumen Harbor' },
  { title: 'Quiet Kettles', artist: 'Tide and Timber' },
  { title: 'Marrow Road', artist: 'Tide and Timber' },
];
const artists = ['Lumen Harbor', 'Tide and Timber'];
const lyrics = ['carry me home to the lumen harbor', 'quiet kettles on the tide'];

export const sleep = (ms, abort) =>
  new Promise((resolve, reject) => {
    const stop = () => {
      clearTimeout(timer);
      reject(abort.reason);
    };
    const timer = setTimeout(() => {
      abort?.removeEventListener('abort', stop);
      resolve();
    }, ms);
    abort?.addEventListener('abort', stop, { once: true });
  });

// An adapter that waits `ms` (stopping early if aborted), then asks `look`
// for hits. `look` returns null to stay quiet.
export const lookup = (ms, look) => ({
  async handle(payload, _signal, context) {
    const prefix = payload.text.toLowerCase();
    await sleep(ms, context?.abort);
    const hits = look(prefix);
    return hits ? json(hits) : text('');
  },
});

// The JavaScript tracks adapter: fast, acts from 3 characters.
export const jsTracks = () =>
  lookup(40, (p) =>
    p.length < 3 ? null : tracks.filter((t) => `${t.title} ${t.artist}`.toLowerCase().includes(p)).map((t) => t.title),
  );

// Types 'lumen harbor' into a search box wired to the tracks neuron (an
// `Adapter` under `tracksId`) plus the artist and lyrics neurons. `pad` is the
// width of the neuron id column in the output.
export async function runTyping({ tracksId = 'tracks', tracksAdapter, pad = 7 }) {
  // What the box emitted, by signal id: an answer's `replyTo` finds its prefix here.
  const typed = new Map();

  const network = new Network({
    onSignal: (s) => {
      if (s.from === 'box' || serialize(s.payload) === '') return; // quiet neurons say nothing
      const hits = s.payload.data;
      console.log(`  answer   ${s.from.padEnd(pad)} for "${typed.get(s.replyTo)}": ${hits.length ? hits.join(' | ') : 'no hits'}`);
    },
    onDrop: (s, id) => console.log(`  dropped  ${id.padEnd(pad)} for "${typed.get(s.id)}" (stale)`),
    onError: (e, s, id) => console.log(`  error    ${id}: ${e.message}`),
  });

  network.add(new Neuron('box', lookup(0, () => null)));
  network.add(new Neuron(tracksId, tracksAdapter));
  // artist: quickest, acts only on an exact name
  network.add(new Neuron('artist', lookup(20, (p) => (artists.some((a) => a.toLowerCase() === p) ? [p] : null))));
  // lyrics: slow, acts from 5 characters
  network.add(
    new Neuron(
      'lyrics',
      lookup(250, (p) => (p.length < 5 ? null : lyrics.filter((l) => l.includes(p)))),
    ),
  );
  for (const id of [tracksId, 'artist', 'lyrics']) network.connect('box', id);

  const query = 'lumen harbor';
  for (let i = 1; i <= query.length; i++) {
    const prefix = query.slice(0, i);
    console.log(`typed "${prefix}"`);
    typed.set(network.emit('box', text(prefix), { replaces: 'query' }).id, prefix);
    await sleep(100);
  }
  await network.idle();
}
