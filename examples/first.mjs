// The first example: a shell script, a website API and a human, as one network.
// Run with `npm run example`. Everything stays on localhost.
import { createServer } from 'node:http';
import { fileURLToPath } from 'node:url';
import { HttpAdapter, HumanAdapter, Network, Neuron, ShellAdapter, serialize, text } from '../dist/index.js';

// A local "website API": answers JSON describing what it was sent.
const server = createServer((req, res) => {
  let body = '';
  req.on('data', (chunk) => (body += chunk));
  req.on('end', () => {
    res.setHeader('content-type', 'application/json');
    res.end(JSON.stringify({ received: JSON.parse(body), status: 'ok' }));
  });
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const url = `http://127.0.0.1:${server.address().port}/`;

const network = new Network({
  onSignal: (s) => console.log(`${s.from} -> ${s.to?.join(', ')}: ${serialize(s.payload)}`),
});
const tally = network.add(
  new Neuron('script', new ShellAdapter('sh', [fileURLToPath(new URL('./first/tally.sh', import.meta.url))])),
);
network.add(new Neuron('api', new HttpAdapter(url)));
network.add(new Neuron('human', new HumanAdapter()));
network.connect('script', 'api');
network.connect('script', 'human');

try {
  // The script summarises some text, then tells everything it is linked to.
  const summary = await tally.query(text('ship the first example\nneurons can talk'));
  const answers = await network.send('script', summary);
  const reply = answers.find((a) => a.from === 'human');
  console.log(`human replied: ${serialize(reply.payload)}`);
} finally {
  server.close();
}
