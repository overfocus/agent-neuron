// Level 0 neuron with a threshold: stdin is {"payload": P, "inputs": [P, ...]}.
// argv[2]: how long the stand-in "model call" takes, in ms. Answers once, naming the detectors that agreed.
import { readFileSync } from 'node:fs';

const { inputs } = JSON.parse(readFileSync(0, 'utf8'));
await new Promise((resolve) => setTimeout(resolve, Number(process.argv[2])));
const { alert, service } = inputs[0];
console.log(JSON.stringify({ alert, service, detectors: inputs.map((i) => i.detector).sort() }));
