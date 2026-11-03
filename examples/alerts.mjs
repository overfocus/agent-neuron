// Alert storm triage: 40 invented alerts in about six seconds, cheap detectors
// (two wrapped shell scripts, one JavaScript), a `hypothesis` neuron standing in for
// an expensive model that acts only when two detectors agree, and a composed
// `diagnosis` neuron. Scenario in alerts-demo.mjs, data in alerts/.
// Run with `npm run demo:alerts` (it builds first).
import * as lib from '../dist/index.js';
import { alerts, printSummary, runAlerts } from './alerts-demo.mjs';

console.log(`Alert storm: ${alerts.length} alerts arrive over ${(alerts.at(-1).at / 1000).toFixed(1)} s.`);
console.log('logs (sh), deploys (sh) and metrics (js) look first; hypothesis (a model, 500 ms a call) runs');
console.log('only when two of them flag the same alert; diagnosis is composed and answers with one verdict.\n');
printSummary(await runAlerts(lib, { log: (line) => console.log(line) }));
