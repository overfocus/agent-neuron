// `engine check`: validates a network file against spec/network.schema.json and makes the checks a schema cannot
// (SPEC 2.7): ids, links, composed files, cycles, executables. Positions (line, column) come from the file text.
// shortcut: the validator implements only the JSON Schema keywords that file uses (the ones handled in `validate`);
// upgrade to a real validator if the schema grows keywords beyond these.
import { existsSync, readFileSync } from 'node:fs';
import { delimiter, dirname, join, normalize, resolve } from 'node:path';
const MAY = ['connect', 'listen', 'read', 'write'];
let schema;
const loadSchema = () => (schema ??= JSON.parse(readFileSync(new URL('../../spec/network.schema.json', import.meta.url), 'utf8')));
let neuronSchema;
const loadNeuronSchema = () => (neuronSchema ??= JSON.parse(readFileSync(new URL('../../spec/neuron.schema.json', import.meta.url), 'utf8')));
const isObject = (v) => typeof v === 'object' && v !== null && !Array.isArray(v);
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
/** Problems with `value` against `node` of the schema; each has a JSON path such as `$.neurons[1].filter`. */
export function validate(value, node = loadSchema(), path = '$', root = loadSchema()) {
    if (typeof node.$ref === 'string') {
        const target = node.$ref.replace('#/', '').split('/').reduce((n, k) => n[k], root);
        return validate(value, target, path, root);
    }
    const errors = [];
    const fail = (message, at = path) => errors.push({ path: at, message });
    const type = node.type;
    if (type === 'object' && !isObject(value))
        return [{ path, message: 'expected an object' }];
    if (type === 'array' && !Array.isArray(value))
        return [{ path, message: 'expected an array' }];
    if (type === 'string' && typeof value !== 'string')
        return [{ path, message: 'expected a string' }];
    if (type === 'integer' && !Number.isInteger(value))
        return [{ path, message: 'expected an integer' }];
    if (type === 'number' && (typeof value !== 'number' || !Number.isFinite(value)))
        return [{ path, message: 'expected a number' }];
    if ('const' in node && !same(value, node.const))
        fail(`expected ${JSON.stringify(node.const)}`);
    if (typeof value === 'string') {
        if (typeof node.pattern === 'string' && !new RegExp(node.pattern).test(value))
            fail(`does not match ${node.pattern}`);
        if (typeof node.minLength === 'number' && value.length < node.minLength)
            fail('too short');
        if (typeof node.maxLength === 'number' && value.length > node.maxLength)
            fail('too long');
    }
    if (typeof value === 'number') {
        if (typeof node.minimum === 'number' && value < node.minimum)
            fail(`below ${node.minimum}`);
        if (typeof node.maximum === 'number' && value > node.maximum)
            fail(`above ${node.maximum}`);
        if (typeof node.exclusiveMinimum === 'number' && value <= node.exclusiveMinimum)
            fail(`above ${node.exclusiveMinimum}`);
    }
    if (Array.isArray(value)) {
        if (typeof node.minItems === 'number' && value.length < node.minItems)
            fail(`needs at least ${node.minItems} items`);
        if (typeof node.maxItems === 'number' && value.length > node.maxItems)
            fail(`has more than ${node.maxItems} items`);
        if (node.uniqueItems && new Set(value.map((v) => JSON.stringify(v))).size !== value.length)
            fail('items must be unique');
        if (isObject(node.items))
            value.forEach((v, i) => errors.push(...validate(v, node.items, `${path}[${i}]`, root)));
    }
    if (isObject(value)) {
        const properties = (node.properties ?? {});
        for (const key of (node.required ?? []))
            if (!(key in value))
                fail(`missing "${key}"`);
        for (const [key, needs] of Object.entries((node.dependentRequired ?? {}))) {
            if (key in value)
                for (const n of needs)
                    if (!(n in value))
                        fail(`"${key}" needs "${n}"`);
        }
        if (typeof node.minProperties === 'number' && Object.keys(value).length < node.minProperties)
            fail(`needs at least ${node.minProperties} members`);
        for (const [key, v] of Object.entries(value)) {
            if (isObject(node.propertyNames))
                errors.push(...validate(key, node.propertyNames, `${path}.${key}`, root));
            if (properties[key])
                errors.push(...validate(v, properties[key], `${path}.${key}`, root));
            else if (isObject(node.additionalProperties))
                errors.push(...validate(v, node.additionalProperties, `${path}.${key}`, root));
            else if (node.additionalProperties === false)
                fail(`unknown field "${key}"`, `${path}.${key}`);
        }
    }
    if (isObject(node.not) && validate(value, node.not, path, root).length === 0)
        fail('must not match');
    for (const sub of (node.allOf ?? []))
        errors.push(...validate(value, sub, path, root));
    if (Array.isArray(node.oneOf)) {
        const results = node.oneOf.map((sub) => validate(value, sub, path, root));
        const matches = results.filter((r) => r.length === 0).length;
        // none match: report the closest branch (fewest errors), which names the real problem
        if (matches === 0)
            errors.push(...results.reduce((best, r) => (r.length < best.length ? r : best)));
        else if (matches > 1)
            fail('matches more than one kind');
    }
    return errors;
}
/** Offset in `text` of each object member (its key) and array item, by JSON path. `text` must be valid JSON. */
function positions(text) {
    const at = new Map();
    let i = 0;
    const ws = () => { while (/\s/.test(text[i] ?? ''))
        i++; };
    const string = () => {
        const start = i++;
        while (text[i] !== '"')
            i += text[i] === '\\' ? 2 : 1;
        return JSON.parse(text.slice(start, ++i));
    };
    const value = (path) => {
        ws();
        const c = text[i];
        if (c === '{' || c === '[') {
            i++;
            ws();
            for (let n = 0; text[i] !== '}' && text[i] !== ']'; n++) {
                ws();
                if (c === '[') {
                    at.set(`${path}[${n}]`, i);
                    value(`${path}[${n}]`);
                }
                else {
                    const start = i, key = string();
                    at.set(`${path}.${key}`, start);
                    ws();
                    i++; // the colon
                    value(`${path}.${key}`);
                }
                ws();
                if (text[i] === ',')
                    i++;
                ws();
            }
            i++;
        }
        else if (c === '"')
            string();
        else
            while (!/[\s,\]}]/.test(text[i] ?? ','))
                i++;
    };
    value('$');
    return at;
}
/** The ids of `ids` nearest to `id` by edit distance, if any is close enough to be a typo. */
function nearest(id, ids) {
    const distance = (a, b) => {
        let row = Array.from({ length: b.length + 1 }, (_, j) => j);
        for (let i = 1; i <= a.length; i++) {
            const next = [i];
            for (let j = 1; j <= b.length; j++)
                next[j] = Math.min(row[j] + 1, next[j - 1] + 1, row[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
            row = next;
        }
        return row[b.length];
    };
    const best = [...ids].map((c) => [c, distance(id, c)]).sort((x, y) => x[1] - y[1])[0];
    return best && best[1] <= Math.max(2, Math.floor(id.length / 3)) ? best[0] : undefined;
}
const onPath = (cmd, cwd) => cmd.includes('/') ? existsSync(resolve(cwd, cmd)) : (process.env.PATH ?? '').split(delimiter).some((d) => d && existsSync(join(d, cmd)));
/**
 * The neuron an entry with `neuron` stands for: the file's kind and own options, the entry's on top. Relative paths
 * of the file are relative to its directory, so the `cwd` is written absolute. Assumes `check` passed.
 */
function resolved(entry, of, neuronFile) {
    const { version: _version, ...own } = of;
    delete own.$schema;
    const { neuron: _neuron, ...mine } = entry;
    const neuron = { ...own, ...mine, ...(of.run || of.serve ? { cwd: resolve(dirname(neuronFile), of.cwd ?? '.') } : {}) };
    ownDir.set(neuron, dirname(neuronFile));
    return neuron;
}
/** Where a neuron named by its own file keeps its settings: that file's directory (decision 0022, point 6). Not a field: it is no part of the neuron. */
const ownDir = new WeakMap();
export class BadSettings extends Error {
}
/**
 * A neuron's own settings (SPEC 2.4, decision 0022 point 6): `.env` in the directory of the file that declares its program
 * (`dir` for a neuron written inline), as the values of the names in its `env` that the file has. Lines are `NAME=value`;
 * blank lines and `#` lines are skipped, a quoted value loses its quotes, and anything else throws, naming the line and never its text.
 */
export function settingsOf(spec, dir) {
    const names = spec.env ?? [], file = join(ownDir.get(spec) ?? dir, '.env'), out = {};
    if (!names.length || !existsSync(file))
        return out;
    readFileSync(file, 'utf8').split(/\r?\n/).forEach((raw, i) => {
        const line = raw.trim();
        if (!line || line.startsWith('#'))
            return;
        const m = /^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)$/.exec(line);
        if (!m)
            throw new BadSettings(`${file} line ${i + 1}: not NAME=value`);
        const v = m[2], quoted = v.length >= 2 && (v[0] === '"' || v[0] === "'") && v.endsWith(v[0]);
        if (names.includes(m[1]))
            out[m[1]] = quoted ? v.slice(1, -1) : v;
    });
    return out;
}
/**
 * Reads and parses a network file; throws if it is not JSON. Does not validate. Every entry that names a neuron's own
 * file is replaced by the neuron it describes (SPEC 2.10), so nothing after this sees the `neuron` kind.
 */
export function readNetwork(file) {
    const net = JSON.parse(readFileSync(file, 'utf8'));
    if (!Array.isArray(net.neurons))
        return net;
    net.neurons = net.neurons.map((n) => {
        if (typeof n?.neuron !== 'string')
            return n;
        const neuronFile = resolve(dirname(file), n.neuron);
        return resolved(n, JSON.parse(readFileSync(neuronFile, 'utf8')), neuronFile);
    });
    return net;
}
/**
 * Everything `check` must report (SPEC 2.7); composed files are checked too, relative to `file`. Errors make the file unusable.
 * `shown` is only for composed files: their path relative to the outer file's directory, which findings name them by.
 */
export function check(file, seen = [], shown) {
    const out = [];
    if (!existsSync(file))
        return [{ severity: 'error', file, message: 'file not found' }];
    const text = readFileSync(file, 'utf8');
    let net;
    try {
        net = JSON.parse(text);
    }
    catch (e) {
        return [{ severity: 'error', file: shown ?? file, message: `not valid JSON: ${e.message}` }];
    }
    const where = positions(text);
    const add = (severity, path, message) => {
        const offset = where.get(path);
        const before = offset === undefined ? undefined : text.slice(0, offset).split('\n');
        out.push({ severity, file: shown ?? file, ...(before && { line: before.length, column: before.at(-1).length + 1 }), message });
    };
    const problems = validate(net);
    if (problems.length) {
        for (const p of problems)
            add('error', p.path, `${p.path}: ${p.message}`);
        return out; // the rest assumes the shape is right
    }
    // a neuron named by its own file (SPEC 2.10, decision 0019): the file is read and checked, the entry may only narrow
    // it, and from here on the neuron is the one the file describes
    let unresolved = false;
    net.neurons = net.neurons.map((n, i) => {
        if (n.neuron === undefined)
            return n;
        const at = `$.neurons[${i}]`, neuronFile = resolve(dirname(file), n.neuron);
        const stop = (path, message) => { add('error', path, message); unresolved = true; return n; };
        if (!existsSync(neuronFile))
            return stop(`${at}.neuron`, `file not found: ${n.neuron}`);
        let of;
        try {
            of = JSON.parse(readFileSync(neuronFile, 'utf8'));
        }
        catch (e) {
            return stop(`${at}.neuron`, `${n.neuron}: not valid JSON: ${e.message}`);
        }
        const wrong = validate(of, loadNeuronSchema(), '$', loadNeuronSchema());
        if (wrong.length)
            return stop(`${at}.neuron`, `${n.neuron}: ${wrong.map((p) => `${p.path}: ${p.message}`).join('; ')} (a neuron file holds what is the neuron's own, SPEC 2.10)`);
        const program = of.run !== undefined || of.serve !== undefined;
        for (const key of ['env', 'may'])
            if (n[key] !== undefined && !program)
                return stop(`${at}.${key}`, `"${key}" narrows a program's, and ${n.neuron} is not a run or serve neuron`);
        const limit = of.timeout ?? 30000;
        if (n.timeout !== undefined && n.timeout > limit)
            return stop(`${at}.timeout`, `timeout ${n.timeout} is wider than the neuron's own (${limit}, ${n.neuron}): the entry that names a neuron may only narrow it (SPEC 2.10)`);
        // `maxInflight` (SPEC 2.4): an option of run and http neurons' files; a file that states none has no limit, so any limit narrows it
        if (n.maxInflight !== undefined && of.run === undefined && of.http === undefined)
            return stop(`${at}.maxInflight`, `"maxInflight" is an option of run, http and network neurons, and ${n.neuron} is not a run or http neuron (SPEC 2.4)`);
        if (n.maxInflight !== undefined && of.maxInflight !== undefined && n.maxInflight > of.maxInflight)
            return stop(`${at}.maxInflight`, `maxInflight ${n.maxInflight} is wider than the neuron's own (${of.maxInflight}, ${n.neuron}): the entry that names a neuron may only narrow it (SPEC 2.10)`);
        for (const name of n.env ?? [])
            if (!(of.env ?? []).includes(name))
                return stop(`${at}.env`, `env "${name}" is wider than the neuron's own (${n.neuron}): the entry that names a neuron may only narrow it (SPEC 2.10)`);
        for (const m of MAY)
            for (const v of n.may?.[m] ?? [])
                if (!(of.may?.[m] ?? []).includes(v))
                    return stop(`${at}.may`, `may ${m} "${v}" is wider than the neuron's own (${n.neuron}): the entry that names a neuron may only narrow it (SPEC 2.10)`);
        return resolved(n, of, neuronFile);
    });
    if (unresolved)
        return out; // the rest assumes every neuron has its kind
    const ids = new Set();
    net.neurons.forEach((n, i) => {
        if (ids.has(n.id))
            add('error', `$.neurons[${i}].id`, `duplicate neuron id "${n.id}"`);
        ids.add(n.id);
    });
    const unknown = (what, id) => `${what} names unknown neuron "${id}"${nearest(id, ids) ? ` (did you mean "${nearest(id, ids)}"?)` : ''}`;
    const byId = new Map(net.neurons.map((n) => [n.id, n]));
    const links = net.links ?? [];
    const pairs = new Set();
    links.forEach(({ from, to }, i) => {
        for (const side of ['from', 'to'])
            if (!ids.has(links[i][side]))
                add('error', `$.links[${i}].${side}`, unknown('link', links[i][side]));
        if (pairs.has(`${from}\0${to}`))
            add('error', `$.links[${i}]`, `link ${from} -> ${to} is listed twice (links are one-way: a two-way link is two entries, once each)`);
        pairs.add(`${from}\0${to}`);
        if (byId.get(to)?.source)
            add('error', `$.links[${i}]`, `link ${from} -> ${to}: a source receives nothing`);
    });
    // a cost names units the file's budget has (SPEC 2.7); amounts are integers from 0 (the schema)
    const units = Object.keys(net.budget ?? {});
    net.neurons.forEach((n, i) => {
        for (const unit of Object.keys(n.cost ?? {})) {
            if (units.includes(unit)) {
                if (n.cost[unit] > net.budget[unit])
                    add('warning', `$.neurons[${i}].cost.${unit}`, `neuron "${n.id}" costs ${n.cost[unit]} ${unit}, more than the budget's ${net.budget[unit]}: it can never run`);
                continue;
            }
            const near = nearest(unit, units);
            add('error', `$.neurons[${i}].cost.${unit}`, `cost names unit "${unit}", which is not in the budget${units.length ? ` (${units.join(', ')})` : ' (the file has none)'}${near ? `: did you mean "${near}"?` : ''}`);
        }
    });
    // with a budget every neuron but a source declares a cost, 0 allowed: a neuron that could spend without one would escape it (SPEC 10.1)
    if (net.budget)
        net.neurons.forEach((n, i) => { if (!n.source && !n.cost)
            add('error', `$.neurons[${i}]`, `neuron "${n.id}" has no cost, and the file has a budget: every neuron but a source declares one (0 is allowed)`); });
    for (const key of ['entry', 'exit'])
        if (net[key] && !ids.has(net[key]))
            add('error', `$.${key}`, unknown(`"${key}"`, net[key]));
    // the ground's deposit rule may restrict its writes to one declared neuron (SPEC 4.5, decision 0012)
    if (net.ground?.deposit?.by !== undefined && !ids.has(net.ground.deposit.by))
        add('error', '$.ground.deposit.by', unknown('"deposit.by"', net.ground.deposit.by));
    // a cycle in which no neuron has maxHops never ends
    const open = (id) => byId.get(id)?.maxHops === undefined;
    const state = new Map();
    const visit = (id) => {
        if (state.get(id) === 1)
            return true;
        if (state.get(id) === 2)
            return false;
        state.set(id, 1);
        const loops = links.some((l) => l.from === id && open(l.to) && open(id) && visit(l.to));
        state.set(id, 2);
        return loops;
    };
    const looped = net.neurons.findIndex((n) => open(n.id) && visit(n.id));
    if (looped >= 0)
        add('error', `$.neurons[${looped}]`, 'a cycle with no maxHops on any neuron in it');
    const here = resolve(file);
    net.neurons.forEach((n, i) => {
        const at = `$.neurons[${i}]`;
        const cwd = resolve(dirname(file), n.cwd ?? '.');
        if (n.cwd !== undefined && !existsSync(cwd))
            add('error', `${at}.cwd`, `working directory "${n.cwd}" not found`);
        const cmd = n.run?.[0] ?? n.serve?.[0];
        if (cmd && !onPath(cmd, cwd))
            add('warning', `${at}.${n.run ? 'run' : 'serve'}`, `executable "${cmd}" not found`);
        if (cmd)
            try {
                settingsOf(n, dirname(file));
            }
            catch (e) {
                if (!(e instanceof BadSettings))
                    throw e;
                add('error', `${at}.env`, `neuron "${n.id}": its settings file ${e.message}`);
            }
        if (n.http)
            add('warning', at, `neuron "${n.id}" is http: this engine runs source, run, serve, network and human neurons only`);
        if (n.network === undefined)
            return;
        const inner = resolve(dirname(file), n.network);
        if (seen.includes(inner) || inner === here)
            return add('error', `${at}.network`, 'a network file may not contain itself');
        if (!existsSync(inner))
            return add('error', `${at}.network`, `file not found: ${n.network}`);
        const found = check(inner, [...seen, here], normalize(join(dirname(shown ?? '.'), n.network)));
        out.push(...found);
        if (found.some((f) => f.severity === 'error'))
            return;
        const sub = readNetwork(inner);
        if (sub.neurons.some((x) => x.id === n.id))
            add('error', `${at}.id`, `neuron "${n.id}" has the same id as a neuron inside ${n.network}: a composed neuron's id must not be one of its inner ids`);
        for (const key of ['entry', 'exit'])
            if (!sub[key])
                add('error', `${at}.network`, `${n.network} is used as a composed neuron and needs "${key}"`);
        if (sub.neurons.some((x) => x.effect))
            add('error', `${at}.network`, `${n.network} is used as a composed neuron and may not contain an effect neuron (SPEC 11.5)`);
        if (sub.neurons.some((x) => x.source))
            add('error', `${at}.network`, `${n.network} is used as a composed neuron and may not contain a source`);
        if (sub.ground !== undefined)
            add('error', `${at}.network`, `${n.network} is used as a composed neuron and may not declare a ground (SPEC 4.5)`);
        // the fold (SPEC 2.7, decision 0016): what a composed neuron declares it may touch covers what its inside declares
        for (const x of sub.neurons)
            for (const m of MAY)
                for (const v of x.may?.[m] ?? []) {
                    if (!(n.may?.[m] ?? []).includes(v))
                        add('error', n.may ? `${at}.may` : at, `the may of "${n.id}" does not cover its inside: "${x.id}" in ${n.network} may ${m} "${v}" (SPEC 2.7)`);
                }
        const lacking = [...new Set(sub.neurons.flatMap((x) => Object.keys(x.cost ?? {})))].filter((u) => n.cost && !(u in n.cost));
        if (lacking.length)
            add('warning', at, `${n.network} has neurons costing ${lacking.join(', ')}, which the cost of "${n.id}" does not name: those calls are always refused (SPEC 10.5)`);
        if (sub.exit && (sub.links ?? []).some((l) => l.from === sub.exit))
            add('error', `${at}.network`, `${n.network}: "exit" may not have outgoing links`);
    });
    return out;
}
export const formatFinding = (f) => `${f.file}${f.line ? `:${f.line}:${f.column}` : ''}: ${f.severity}: ${f.message}`;
//# sourceMappingURL=check.js.map