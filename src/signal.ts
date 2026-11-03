import { randomUUID } from 'node:crypto';

/** What a signal carries: free text or JSON. */
export type Payload = { type: 'text'; text: string } | { type: 'json'; data: unknown };

/** One message on the network. `to` lists the neurons it was delivered to (for a broadcast, every linked neuron). */
export type Signal = { id: string; from: string; to?: string[]; payload: Payload };

export const text = (value: string): Payload => ({ type: 'text', text: value });

export const json = (data: unknown): Payload => ({ type: 'json', data });

/** JSON if the string parses, otherwise text. */
export function parsePayload(raw: string): Payload {
  try {
    return json(JSON.parse(raw));
  } catch {
    return text(raw);
  }
}

/** The wire form of a payload: the text itself, or the JSON stringified. */
export const serialize = (payload: Payload): string =>
  payload.type === 'text' ? payload.text : JSON.stringify(payload.data);

export function newSignal(from: string, payload: Payload, to?: string[]): Signal {
  return to ? { id: randomUUID(), from, to, payload } : { id: randomUUID(), from, payload };
}
