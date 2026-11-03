// The typing demo plus a `suggest` neuron that acts only when two different
// neurons agree on the same keystroke (threshold 2). The answers of tracks,
// artist and lyrics are forwarded to it with `{ cause }`. Most keystrokes get
// one answer, so suggest holds it and later drops it; it runs when a second
// neuron answers the same keystroke. Run with `npm run demo:suggest`.
import { jsTracks, runTyping } from './typing-demo.mjs';

await runTyping({ tracksAdapter: jsTracks(), suggest: true });
