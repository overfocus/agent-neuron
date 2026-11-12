import { randomUUID } from 'node:crypto';
export const text = (value) => ({ type: 'text', text: value });
export const json = (data) => ({ type: 'json', data });
/** JSON if the string parses, otherwise text. */
export function parsePayload(raw) {
    try {
        return json(JSON.parse(raw));
    }
    catch {
        return text(raw);
    }
}
/** The wire form of a payload: the text itself, or the JSON stringified. */
export const serialize = (payload) => payload.type === 'text' ? payload.text : JSON.stringify(payload.data);
export function newSignal(from, payload, { to, replyTo, cause } = {}) {
    const id = randomUUID();
    const origin = replyTo ?? cause;
    const signal = { id, from, payload, trace: origin ? origin.trace : id, hops: origin ? origin.hops + 1 : 0 };
    if (to)
        signal.to = to;
    if (replyTo)
        signal.replyTo = replyTo.id;
    if (cause)
        signal.cause = cause.id;
    return signal;
}
//# sourceMappingURL=signal.js.map