// The typing demo with its tracks neuron written in Rust: the `catalogue`
// binary (rust/catalogue) wrapped by the shell adapter, in place of the
// JavaScript one. Same network and rule (the catalogue is repeated in the Rust source), so the
// answers agree; the binary prints nothing under 3 characters, which is a quiet neuron.
// Run with `npm run demo:rust`, which builds the library and the binary first.
import { fileURLToPath } from 'node:url';
import { ShellAdapter } from '../dist/index.js';
import { runTyping } from './typing-demo.mjs';

const binary = fileURLToPath(new URL('../rust/target/release/catalogue', import.meta.url));
const rustTracks = new ShellAdapter(binary);

await runTyping({ tracksId: 'tracks (rust)', tracksAdapter: rustTracks, pad: 14 });
