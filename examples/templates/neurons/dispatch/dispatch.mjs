// Hands out a list one item per call. It answers the first type and the rest of the list; the network
// links that answer to the worker (which reads `type`) and back to this neuron (which reads `types`).
// In:  { types: [slug, ...] }
// Out: { type, types: [the rest] }, or quiet when the list is empty: the loop ends there.
import { readFileSync } from 'node:fs';

const p = JSON.parse(readFileSync(0, 'utf8'));
if (!Array.isArray(p?.types)) { console.error('dispatch: types is not a list'); process.exit(1); }
if (p.types.length > 0) {
  const [type, ...types] = p.types;
  console.log(JSON.stringify({ type, types }));
}
