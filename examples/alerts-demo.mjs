// The alert storm triage scenario, shared by alerts.mjs (the demo) and the test.
// 40 invented alerts for five invented services arrive in under four seconds.
//
//   alerts --> logs, deploys, metrics   (detectors: cheap, each decides for itself)
//   detectors --flag--> hypothesis      (threshold 2: acts only when two detectors agree on one alert)
//   hypothesis --result--> diagnosis    (composed: owns an inner network, returns one verdict)
//
// Nothing passes answers on by itself, so onSignal forwards them with `cause`,
// the documented idiom. A newer alert for a service makes older work for it stale
// (`replaces` keyed by service, on the source and on every forward).
// The library is passed in: the demo gives it dist/, the test src/.
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { composedDiagnosis, INNER_IDS } from './alerts/diagnosis.mjs';

export { INNER_IDS };

const here = (name) => fileURLToPath(new URL(`./alerts/${name}`, import.meta.url));
export const alerts = JSON.parse(readFileSync(here('alerts.json'), 'utf8'));

const HYPOTHESIS_MS = 500; // the simulated model call
const WITHIN_MS = 500; // how long a lone flag waits for a second detector
// Timing is chosen so ordering is stable: alerts for one service come either 150 ms apart
// (a burst, so the newer one is still inside the 500 ms call) or 900+ ms apart.

const sleep = (ms, abort) =>
  new Promise((resolve, reject) => {
    const stop = () => (clearTimeout(timer), reject(abort.reason));
    const timer = setTimeout(() => (abort?.removeEventListener('abort', stop), resolve()), ms);
    abort?.addEventListener('abort', stop, { once: true });
  });

// Invented metrics: error rate (%) per service. Only critical alerts are checked against them.
const errorRate = { gateway: 9.4, payments: 7.8, checkout: 1.1, search: 0.6, inventory: 0.9 };

export async function runAlerts(lib, { log = () => {} } = {}) {
  const { Network, Neuron, ShellAdapter, json, text } = lib;
  const quiet = () => text('');
  const stats = { alerts: 0, flags: 0, calls: 0, cut: 0, heldReplaced: 0, expired: 0, diagnoses: 0 };
  const runs = []; // every hypothesis run: the detectors that agreed
  const signals = []; // every signal the outer network reported
  const verdicts = [];
  const cutSince = new Map(); // service -> calls cut short since its last finished call
  const callOf = new Map(); // signal id of the input that crossed the threshold -> call number

  const metrics = {
    async handle(payload) {
      const a = payload.data;
      await sleep(20); // a metrics API call
      if (a.severity !== 'crit' || errorRate[a.service] <= 5) return quiet();
      return json({ detector: 'metrics', alert: a.id, service: a.service, detail: `error rate ${errorRate[a.service]}%` });
    },
  };
  const hypothesis = {
    async handle(_payload, signal, context) {
      const flags = context.inputs.map((s) => s.payload.data);
      runs.push({ trace: signal.trace, inputs: context.inputs });
      const call = ++stats.calls; // one expensive call
      callOf.set(signal.id, call);
      await sleep(HYPOTHESIS_MS, context.abort); // stands in for a model
      return json({ call, ...flags[0], detectors: flags.map((f) => f.detector).sort() });
    },
  };

  const net = new Network({
    onSignal: (s) => {
      signals.push(s);
      if (!s.replyTo || s.payload.type === 'text') return; // only non-empty answers matter
      const d = s.payload.data;
      if (['logs', 'deploys', 'metrics'].includes(s.from)) {
        stats.flags++;
        net.emit(s.from, s.payload, { to: ['hypothesis'], cause: s, replaces: d.service });
      } else if (s.from === 'hypothesis') {
        net.emit('hypothesis', s.payload, { to: ['diagnosis'], cause: s, replaces: d.service });
      } else if (s.from === 'diagnosis') {
        stats.diagnoses++;
        verdicts.push(d);
        const cut = cutSince.get(d.service) ?? 0;
        cutSince.set(d.service, 0);
        log(`call ${String(d.call).padStart(2)}  alert ${String(d.alert).padStart(2)}/${alerts.length} ${d.service.padEnd(9)} ${d.detectors.join(' + ')} agree -> root cause ${d.root}, deploy ${d.deploy}${cut ? `   (${cut} earlier ${d.service} call${cut > 1 ? 's' : ''} cut short as stale)` : ''}`);
      }
    },
    onDrop: (s, id, reason) => {
      if (reason === 'expired') stats.expired++;
      else if (reason === 'replaced' && id === 'hypothesis' && callOf.has(s.id)) {
        stats.cut++;
        cutSince.set(s.payload.data.service, (cutSince.get(s.payload.data.service) ?? 0) + 1);
      } else if (reason === 'replaced') stats.heldReplaced++;
    },
    onError: (e, s, id) => log(`error ${id}: ${e.message}`),
  });

  // the verdict comes from the inner network; carry the hypothesis's own fields alongside it
  const diagnosis = composedDiagnosis(lib);
  const neurons = [
    new Neuron('alerts', { handle: async () => quiet() }),
    new Neuron('logs', new ShellAdapter('sh', [here('detect-logs.sh'), here('logs.txt')])),
    new Neuron('deploys', new ShellAdapter('sh', [here('detect-deploys.sh'), here('deploys.txt')])),
    new Neuron('metrics', metrics),
    new Neuron('hypothesis', hypothesis, { threshold: 2, within: WITHIN_MS }),
    new Neuron('diagnosis', {
      handle: async (payload, signal, context) => {
        const v = (await diagnosis.handle(payload, signal, context)).data;
        return json({ ...v, call: payload.data.call, alert: payload.data.alert, detectors: payload.data.detectors });
      },
    }),
  ];
  for (const n of neurons) net.add(n);
  for (const d of ['logs', 'deploys', 'metrics']) {
    net.connect('alerts', d);
    net.connect(d, 'hypothesis');
  }
  net.connect('hypothesis', 'diagnosis');

  const start = Date.now();
  for (const a of alerts) {
    await sleep(Math.max(0, a.at - (Date.now() - start)));
    stats.alerts++;
    net.emit('alerts', json(a), { replaces: a.service });
  }
  await net.idle();
  await sleep(WITHIN_MS + 100); // lone flags that never found a second detector expire
  const final = verdicts.at(-1);
  return { stats, runs, signals, verdicts, final, neurons };
}

export function printSummary(r, log = console.log) {
  const { stats: s, final } = r;
  log(`
Summary
  alerts received                       ${s.alerts}
  detector flags                        ${s.flags}  (a lone flag waits for a second detector; ${s.expired} gave up waiting)
  hypothesis (expensive) calls          ${s.calls} instead of ${s.alerts}: ${s.alerts - s.calls} expensive calls avoided
    finished / cut short as stale       ${s.calls - s.cut} / ${s.cut}
  stale analyses dropped                ${s.cut + s.heldReplaced}  (${s.cut} calls cut short, ${s.heldReplaced} held flags replaced)
  diagnoses                             ${s.diagnoses}
  final diagnosis                       root cause ${final.root}: deploy ${final.deploy}, ${final.minutes} min before the storm`);
}
