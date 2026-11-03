import { beforeAll, describe, expect, it } from 'vitest';
import { INNER_IDS, alerts, runAlerts, type AlertRun } from '../examples/alerts-demo.mjs';
import * as lib from '../src/index.js';

// Runs the whole storm once (about 6 s of simulated time; the scenario's timing is its point).
let run: AlertRun;
beforeAll(async () => {
  run = await runAlerts(lib);
}, 30_000);

describe('alert storm triage', () => {
  it('replays 40 alerts with the same numbers every time', () => {
    expect(alerts).toHaveLength(40);
    expect(run.stats).toEqual({ alerts: 40, flags: 52, calls: 17, cut: 10, heldReplaced: 11, expired: 7, diagnoses: 7 });
    expect(run.final).toMatchObject({ root: 'gateway', deploy: 'dep-8b21' });
  });

  it('runs hypothesis only when at least two distinct detectors agree on one alert', () => {
    expect(run.runs).toHaveLength(run.stats.calls);
    for (const { trace, inputs } of run.runs) {
      const detectors = new Set(inputs.map((s) => s.from));
      expect(detectors.size).toBeGreaterThanOrEqual(2);
      for (const s of inputs) {
        expect(s.trace).toBe(trace);
        expect(['logs', 'deploys', 'metrics']).toContain(s.from);
      }
      expect(new Set(inputs.map((s) => (s.payload as { data: { alert: number } }).data.alert)).size).toBe(1);
    }
  });

  it('wraps at least two detectors as shell scripts', () => {
    const shells = run.neurons.filter((n) => n.adapter instanceof lib.ShellAdapter);
    expect(shells.map((n) => n.id)).toEqual(['logs', 'deploys']);
  });

  it('keeps diagnosis composed: no outer signal names an inner neuron', () => {
    expect(run.signals.length).toBeGreaterThan(0);
    for (const s of run.signals) {
      expect(INNER_IDS).not.toContain(s.from);
      for (const t of s.to ?? []) expect(INNER_IDS).not.toContain(t);
    }
    expect(run.signals.some((s) => s.from === 'diagnosis')).toBe(true);
  });
});
