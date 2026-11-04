// Level 0 neuron: reads one alert (JSON) on stdin, prints a flag, or nothing (quiet).
// Invented error rates (%) per service; only critical alerts are checked.
import { readFileSync } from 'node:fs';

const errorRate = { gateway: 9.4, payments: 7.8, checkout: 1.1, search: 0.6, inventory: 0.9 };
const alert = JSON.parse(readFileSync(0, 'utf8'));
if (alert.severity === 'crit' && errorRate[alert.service] > 5) {
  console.log(JSON.stringify({ detector: 'metrics', alert: alert.id, service: alert.service, detail: `error rate ${errorRate[alert.service]}%` }));
}
