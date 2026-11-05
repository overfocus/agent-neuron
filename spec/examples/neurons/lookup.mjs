// Level 0 neuron: reads {"id": ...} on stdin and answers whether that id is in the file named by SENT_LOG.
import { existsSync, readFileSync } from 'node:fs';

const { id } = JSON.parse(readFileSync(0, 'utf8'));
const sent = existsSync(process.env.SENT_LOG) && readFileSync(process.env.SENT_LOG, 'utf8').split('\n').includes(id);
console.log(JSON.stringify({ id, sent }));
