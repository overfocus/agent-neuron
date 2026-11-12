import { Neuron } from './neuron.js';
import { newSignal } from './signal.js';
/** A neuron's limits refuse a signal that is too deep in its chain. `Neuron.query` is a direct ask and skips them. */
const refuses = (target, signal) => target.options.maxHops !== undefined && signal.hops > target.options.maxHops;
/** How much one sender counts toward a threshold. shortcut: always 1; strength will return the receiver's rating of `sender`. */
const weight = (_target, _sender) => 1;
export class Network {
    options;
    neurons = new Map();
    pending = new Set();
    // Inputs held for a threshold, by neuron and trace, then by sender. Not in `pending`: `idle` does not wait for them.
    held = new Map();
    // shortcut: one controller kept per sender+key forever; prune if keys are unbounded.
    replaceable = new Map();
    constructor(options = {}) {
        this.options = options;
    }
    add(neuron) {
        this.neurons.set(neuron.id, neuron);
        return neuron;
    }
    connect(a, b) {
        this.get(a).connect(this.get(b));
    }
    /**
     * Send from one neuron to everything it is linked to, or only to `to`
     * (which must be linked). Delivers in link order, one at a time, and
     * returns each answer as a signal from the neuron that gave it. A target
     * whose `maxHops` the signal exceeds is skipped: its answer is left out,
     * with no `onDrop` (the signal itself is still reported to `onSignal`).
     * `threshold` does not apply to `send`, which is a direct exchange: every
     * target handles the one signal at once, with no `context.inputs`.
     */
    async send(fromId, payload, { to, cause } = {}) {
        const { sender, targets } = this.resolve(fromId, to);
        const signal = newSignal(fromId, payload, { to: targets.map((t) => t.id), ...(cause && { cause }) });
        this.options.onSignal?.(signal);
        const answers = [];
        for (const target of targets) {
            if (refuses(target, signal))
                continue;
            const answer = newSignal(target.id, await target.receive(signal), { to: [sender.id], replyTo: signal });
            this.options.onSignal?.(answer);
            answers.push(answer);
        }
        return answers;
    }
    /**
     * Like `send`, but returns the outgoing signal at once and does not wait.
     * Targets work in parallel; answers arrive through `onSignal`, failures
     * through `onError`. With `replaces`, a newer emit under the same key from
     * the same sender aborts the older one's work (adapters get an abort
     * signal) and nothing more is reported for it except `onDrop`. A target
     * whose `maxHops` the signal exceeds never runs: `onDrop` with 'hops'.
     *
     * A target with a `threshold` holds each input until enough distinct
     * senders have sent one on the same trace: a sender's newer input replaces
     * its older one ('replaced'), the sum of senders reaching the threshold runs
     * the adapter once with those inputs in `context.inputs` and answers the
     * last of them (the others get no answer and no drop, even if that run is aborted or fails), and an input still held
     * after the neuron's `within` is dropped as 'expired'. A held input whose
     * emit is replaced is dropped as 'replaced'. Held inputs are waiting, not
     * working: `idle` does not wait for them, and their timers never keep the
     * process alive, so a program that ends first never sees the 'expired'.
     * A running call is cut when any input it acted on is replaced, not only the
     * crossing one: one `onDrop` 'replaced' is reported, for the crossing input.
     * A later input on a trace that already ran starts a new hold.
     */
    emit(fromId, payload, { to, replaces, cause } = {}) {
        const { targets } = this.resolve(fromId, to);
        const signal = newSignal(fromId, payload, { to: targets.map((t) => t.id), ...(cause && { cause }) });
        const controller = new AbortController();
        if (replaces !== undefined) {
            const key = `${fromId}\0${replaces}`;
            this.replaceable.get(key)?.abort();
            this.replaceable.set(key, controller);
        }
        this.notify(() => this.options.onSignal?.(signal));
        for (const target of targets) {
            const work = this.run(target, signal, controller.signal).finally(() => this.pending.delete(work));
            this.pending.add(work);
        }
        return signal;
    }
    /** Resolves once all emitted work has finished or been dropped, including work started meanwhile. */
    async idle() {
        while (this.pending.size)
            await Promise.allSettled([...this.pending]);
    }
    async run(target, signal, abort) {
        if (refuses(target, signal))
            return this.notify(() => this.options.onDrop?.(signal, target.id, 'hops'));
        const crossed = target.options.threshold === undefined ? undefined : this.hold(target, signal, abort);
        if (target.options.threshold !== undefined && !crossed)
            return; // held, waiting for the others
        const inputs = crossed?.map((held) => held.signal);
        // The call is stale as soon as any input it acted on is: its emit replaced, not only the crossing one's.
        if (crossed)
            abort = AbortSignal.any(crossed.map((held) => held.abort));
        let payload;
        let failure;
        try {
            payload = await target.receive(signal, { abort, ...(inputs && { inputs }) });
        }
        catch (error) {
            failure = { error };
        }
        // Listeners run outside the try, so a listener's bug is never blamed on the neuron.
        // An aborted run is never an error, whatever the adapter threw or returned.
        if (abort.aborted)
            this.notify(() => this.options.onDrop?.(signal, target.id, 'replaced'));
        else if (failure)
            this.notify(() => this.options.onError?.(failure.error, signal, target.id));
        else
            this.notify(() => this.options.onSignal?.(newSignal(target.id, payload, { to: [signal.from], replyTo: signal })));
    }
    /** Hold `signal` for `target`'s threshold. Returns the held inputs to act on if it is met, otherwise undefined. */
    hold(target, signal, abort) {
        const key = `${target.id}\0${signal.trace}`;
        // Drop the older input first: its onDrop listener may emit into this neuron and trace, so
        // no listener may run between reading `held` and writing it below.
        this.held.get(key)?.get(signal.from)?.drop('replaced');
        const group = this.held.get(key) ?? new Map();
        this.held.set(key, group);
        const settle = () => {
            clearTimeout(timer);
            abort.removeEventListener('abort', onAbort);
        };
        const entry = {
            signal,
            abort,
            settle,
            drop: (reason) => {
                settle();
                group.delete(signal.from);
                if (!group.size)
                    this.held.delete(key);
                this.notify(() => this.options.onDrop?.(signal, target.id, reason));
            },
        };
        const onAbort = () => entry.drop('replaced');
        const timer = setTimeout(() => entry.drop('expired'), target.options.within ?? 10_000);
        timer.unref();
        abort.addEventListener('abort', onAbort, { once: true });
        group.set(signal.from, entry);
        let total = 0;
        for (const sender of group.keys())
            total += weight(target, sender);
        // shortcut: exact comparison; fractional strengths (0.1 + 0.2 < 0.3) will need a small tolerance.
        if (total < target.options.threshold)
            return undefined;
        const inputs = [...group.values()];
        for (const held of inputs)
            held.settle();
        this.held.delete(key);
        return inputs;
    }
    /**
     * Listeners called for `emit` work (onSignal, onError, onDrop) cannot take
     * the network down: whatever they throw is swallowed, since nothing is
     * waiting to catch it. Catch errors inside your own listener if you care.
     */
    notify(call) {
        try {
            call();
        }
        catch {
            // swallowed on purpose, see above
        }
    }
    resolve(fromId, to) {
        const sender = this.get(fromId);
        const targets = to
            ? to.map((id) => {
                const target = this.get(id);
                if (!sender.links.has(target))
                    throw new Error(`${fromId} is not linked to ${id}`);
                return target;
            })
            : [...sender.links];
        return { sender, targets };
    }
    get(id) {
        const neuron = this.neurons.get(id);
        if (!neuron)
            throw new Error(`unknown neuron: ${id}`);
        return neuron;
    }
}
//# sourceMappingURL=network.js.map