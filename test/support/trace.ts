// The validity checks every trace must pass (docs/TRACE.md): committed recordings (recording.test.ts) and engine runs (engine.test.ts).
import { expect } from 'vitest';
import type { Trace } from '../../examples/trace/replay.mjs';

const REASONS = ['replaced', 'expired', 'hops'];
const TYPES = ['signal', 'answer', 'quiet', 'drop', 'error', 'fail'];

export function expectValidTrace(trace: Trace): void {
  expect(trace).toMatchObject({ format: 'agent-neuron-trace', version: 1 });
  expect(trace.description).toEqual(expect.any(String));
  expect(trace.library).toMatch(/^\d+\.\d+\.\d+/);
  const ids = new Set(trace.neurons.map((n) => n.id));
  expect(ids.size).toBe(trace.neurons.length);
  for (const n of trace.neurons) expect([n.kind, n.description].every((s) => typeof s === 'string' && s)).toBe(true);
  const hops = new Map<string, number>(); // signal id -> hops, from the first event that names it
  let last = 0;
  for (const e of trace.events) {
    if (!TYPES.includes(e.type)) continue; // TRACE.md: a reader ignores event types it does not know
    expect(Number.isInteger(e.t)).toBe(true);
    expect(e.t).toBeGreaterThanOrEqual(last);
    last = e.t;
    if (e.type === 'fail') continue;
    for (const id of [e.from!, ...(e.to ?? []), ...(e.neuron ? [e.neuron] : [])]) expect(ids).toContain(id);
    if (e.type === 'drop') expect(REASONS).toContain(e.reason);
    if (e.type === 'drop' || e.type === 'error') {
      expect(hops.has(e.id!)).toBe(true); // it is about a signal already reported
      continue;
    }
    const origin = e.replyTo ?? e.cause;
    if (origin === undefined) expect(e.hops).toBe(0);
    else {
      expect(hops.has(origin)).toBe(true); // an earlier event's signal
      expect(e.hops).toBe(hops.get(origin)! + 1);
    }
    for (const link of [e.replyTo, e.cause]) if (link !== undefined) expect(hops.has(link)).toBe(true);
    hops.set(e.id!, e.hops!);
  }
}
