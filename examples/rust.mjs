// The typing demo with its tracks neuron written in Rust: the `catalogue`
// binary (rust/catalogue) wrapped by the shell adapter, in place of the
// JavaScript one. Same network, same rule, so the answers agree.
// Run with `npm run demo:rust`, which builds the library and the binary first.
import { fileURLToPath } from 'node:url';
import { ShellAdapter, serialize, text } from '../dist/index.js';
import { runTyping } from './typing-demo.mjs';

const binary = fileURLToPath(new URL('../rust/target/release/catalogue', import.meta.url));
const shell = new ShellAdapter(binary);

// The binary prints [] for a query it does not act on; the neuron stays quiet then.
const rustTracks = {
  async handle(payload, signal, context) {
    const answer = await shell.handle(payload, signal, context);
    return serialize(answer) === '[]' ? text('') : answer;
  },
};

await runTyping({ tracksId: 'tracks (rust)', tracksAdapter: rustTracks, pad: 14 });
