// A composed neuron's adapter: `diagnosis` owns a private inner network (a
// topology lookup and a read-only deploy check) and returns ONE verdict.
// Sealed by construction: the inner neurons are never added to the outer
// network, so nothing outside can address them and no inner id leaves in a
// signal. Each call builds a fresh inner network, so nothing is shared
// between calls. The library is passed in so tests can use src/ and the demo dist/.
import { readFile } from 'node:fs/promises';

export const INNER_IDS = ['door', 'topology', 'check'];

const read = (name) => readFile(new URL(name, import.meta.url), 'utf8');

export function composedDiagnosis({ Network, Neuron, json }) {
  return {
    async handle(payload, _signal, context) {
      const live = () => context?.abort.throwIfAborted(); // stop inner work when the outer emit is replaced
      const { service } = payload.data;
      const inner = new Network();
      inner.add(new Neuron('door', { handle: async () => json({}) }));
      // topology: follow "depends on" to the root of the failure chain.
      inner.add(
        new Neuron('topology', {
          handle: async (p) => {
            live();
            const topology = JSON.parse(await read('topology.json'));
            let root = p.data.service;
            while (topology[root]) root = topology[root];
            return json({ root });
          },
        }),
      );
      // check: read-only look at the deploy list for the root.
      inner.add(
        new Neuron('check', {
          handle: async (p) => {
            live();
            const line = (await read('deploys.txt')).split('\n').find((l) => l.startsWith(`${p.data.root} `));
            const [, deploy, minutes] = line?.split(' ') ?? [];
            return json({ deploy, minutes: Number(minutes) });
          },
        }),
      );
      inner.connect('door', 'topology');
      inner.connect('door', 'check');
      const [{ payload: top }] = await inner.send('door', json({ service }), { to: ['topology'] });
      const [{ payload: chk }] = await inner.send('door', top, { to: ['check'] });
      return json({ service, root: top.data.root, deploy: chk.data.deploy, minutes: chk.data.minutes });
    },
  };
}
