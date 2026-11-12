// Answers a list as an array, one item per type. The neuron has `each`, so the engine forwards each
// item as a signal of its own (SPEC 2.4): this program hands nothing out and keeps no loop.
// In:  { types: [slug, ...] }
// Out: [{ type }, ...]        an empty list is an empty array, and nothing is forwarded
import { readFileSync } from 'node:fs';

const p = JSON.parse(readFileSync(0, 'utf8'));
if (!Array.isArray(p?.types)) { console.error('dispatch: types is not a list'); process.exit(1); }
console.log(JSON.stringify(p.types.map((type) => ({ type }))));
