// Level 0 neuron: follows "depends on" to the root of the failure chain. argv[2]: the topology file.
// Passes its whole input through, adding `root`: later neurons in the composed network need the context.
import { readFileSync } from 'node:fs';

const input = JSON.parse(readFileSync(0, 'utf8'));
const topology = JSON.parse(readFileSync(process.argv[2], 'utf8'));
let root = input.service;
while (topology[root]) root = topology[root];
console.log(JSON.stringify({ ...input, root }));
