// The typing demo: a search box emits the query so far, one letter at a time,
// to three neurons with different latencies. Each decides in its own adapter
// whether to act. A newer prefix replaces the older one, so slow work made
// stale is aborted and dropped. Run with `npm run demo:typing`.
// The catalogue is made up. The setup is shared with the Rust demo
// (`examples/typing-demo.mjs`).
import { jsTracks, runTyping } from './typing-demo.mjs';

// tracks: fast, acts from 3 characters
await runTyping({ tracksAdapter: jsTracks() });
