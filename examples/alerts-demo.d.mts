// Types for alerts-demo.mjs, so the test can import it under strict TypeScript.
import type { Neuron, Signal } from '../src/index.js';

export const INNER_IDS: string[];
export const alerts: { id: number; at: number; service: string; severity: string; message: string }[];
export type Verdict = { call: number; alert: number; service: string; detectors: string[]; root: string; deploy: string; minutes: number };
export type Stats = {
  alerts: number;
  flags: number;
  calls: number;
  cut: number;
  heldReplaced: number;
  expired: number;
  diagnoses: number;
};
export type AlertRun = {
  stats: Stats;
  runs: { trace: string; inputs: Signal[] }[];
  signals: Signal[];
  verdicts: Verdict[];
  final: Verdict;
  neurons: Neuron[];
};
export function runAlerts(lib: typeof import('../src/index.js'), options?: { log?: (line: string) => void; listeners?: import('../src/index.js').NetworkOptions; threshold?: number }): Promise<AlertRun>;
export function printSummary(run: AlertRun, log?: (line: string) => void): void;
