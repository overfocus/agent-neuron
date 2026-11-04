// Level 0 neuron: read-only look at the deploy list for the root. argv[2]: the deploy list.
// Each line is "<service> <deploy id> <minutes ago>". Passes its input through, adding `deploy` and `minutes`.
import { readFileSync } from 'node:fs';

const input = JSON.parse(readFileSync(0, 'utf8'));
const line = readFileSync(process.argv[2], 'utf8').split('\n').find((l) => l.startsWith(`${input.root} `));
const [, deploy, minutes] = line?.split(' ') ?? [];
console.log(JSON.stringify({ ...input, deploy, minutes: Number(minutes) }));
