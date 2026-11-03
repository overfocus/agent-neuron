// A loop being cut: two neurons answer each other because an onSignal handler
// forwards every answer onward with `{ cause }`. Without a limit that never ends;
// maxHops on each neuron refuses the signal that goes too deep. Run with `npm run demo:loop`.
import { Network, Neuron, serialize, text } from '../dist/index.js';

// Made-up agents: each adds its own name to what it was sent.
const adder = (name) => ({ handle: async (payload) => text(`${serialize(payload)} > ${name}`) });

const network = new Network({
  onSignal: (s) => {
    console.log(`hop ${s.hops}: ${s.from} -> ${s.to?.join(', ')}: ${serialize(s.payload)}`);
    // An answer goes back to its asker; forward it onward to whoever answered, keeping the trace.
    // Without `cause` this would start a new trace at hop 0 and the limit would never be reached.
    if (s.replyTo && s.to) network.emit(s.to[0], s.payload, { to: [s.from], cause: s });
  },
  onDrop: (s, id, reason) => console.log(`cut: ${id} refused hop ${s.hops} (${reason}); the loop ends here`),
});
network.add(new Neuron('ping', adder('ping'), { maxHops: 5 }));
network.add(new Neuron('pong', adder('pong'), { maxHops: 5 }));
network.connect('ping', 'pong');

network.emit('ping', text('start'));
await network.idle();
