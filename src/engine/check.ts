// `engine check`: validates a network file against spec/network.schema.json and makes the checks a schema cannot
// (SPEC 2.7): ids, links, composed files, cycles, executables. Positions (line, column) come from the file text.
// shortcut: the validator implements only the JSON Schema keywords that file uses (the ones handled in `validate`);
// upgrade to a real validator if the schema grows keywords beyond these.
import { existsSync, readFileSync } from 'node:fs';
import { delimiter, dirname, join, normalize, resolve } from 'node:path';

export type NeuronSpec = {
  id: string; description?: string;
  source?: true; run?: string[]; serve?: string[]; http?: string; human?: true; network?: string;
  threshold?: number; within?: number; maxHops?: number; timeout?: number; env?: string[]; cwd?: string; cost?: Record<string, number>; effect?: true;
};
export type NetworkFile = {
  version: 0; name?: string; description?: string; entry?: string; exit?: string;
  neurons: NeuronSpec[]; links?: { from: string; to: string }[]; budget?: Record<string, number>;
};
export type Finding = { severity: 'error' | 'warning'; file: string; line?: number; column?: number; message: string };

type Schema = { [key: string]: unknown };
type Problem = { path: string; message: string };
let schema: Schema | undefined;
const loadSchema = (): Schema => (schema ??= JSON.parse(readFileSync(new URL('../../spec/network.schema.json', import.meta.url), 'utf8')) as Schema);

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/** Problems with `value` against `node` of the schema; each has a JSON path such as `$.neurons[1].filter`. */
export function validate(value: unknown, node: Schema = loadSchema(), path = '$'): Problem[] {
  if (typeof node.$ref === 'string') {
    const target = node.$ref.replace('#/', '').split('/').reduce<unknown>((n, k) => (n as Schema)[k], loadSchema());
    return validate(value, target as Schema, path);
  }
  const errors: Problem[] = [];
  const fail = (message: string, at = path) => errors.push({ path: at, message });
  const type = node.type;
  if (type === 'object' && !isObject(value)) return [{ path, message: 'expected an object' }];
  if (type === 'array' && !Array.isArray(value)) return [{ path, message: 'expected an array' }];
  if (type === 'string' && typeof value !== 'string') return [{ path, message: 'expected a string' }];
  if (type === 'integer' && !Number.isInteger(value)) return [{ path, message: 'expected an integer' }];
  if ('const' in node && !same(value, node.const)) fail(`expected ${JSON.stringify(node.const)}`);
  if (typeof value === 'string') {
    if (typeof node.pattern === 'string' && !new RegExp(node.pattern).test(value)) fail(`does not match ${node.pattern}`);
    if (typeof node.minLength === 'number' && value.length < node.minLength) fail('too short');
    if (typeof node.maxLength === 'number' && value.length > node.maxLength) fail('too long');
  }
  if (typeof value === 'number') {
    if (typeof node.minimum === 'number' && value < node.minimum) fail(`below ${node.minimum}`);
    if (typeof node.maximum === 'number' && value > node.maximum) fail(`above ${node.maximum}`);
  }
  if (Array.isArray(value)) {
    if (typeof node.minItems === 'number' && value.length < node.minItems) fail(`needs at least ${node.minItems} items`);
    if (node.uniqueItems && new Set(value.map((v) => JSON.stringify(v))).size !== value.length) fail('items must be unique');
    if (isObject(node.items)) value.forEach((v, i) => errors.push(...validate(v, node.items as Schema, `${path}[${i}]`)));
  }
  if (isObject(value)) {
    const properties = (node.properties ?? {}) as Record<string, Schema>;
    for (const key of (node.required ?? []) as string[]) if (!(key in value)) fail(`missing "${key}"`);
    for (const [key, needs] of Object.entries((node.dependentRequired ?? {}) as Record<string, string[]>)) {
      if (key in value) for (const n of needs) if (!(n in value)) fail(`"${key}" needs "${n}"`);
    }
    if (typeof node.minProperties === 'number' && Object.keys(value).length < node.minProperties) fail(`needs at least ${node.minProperties} members`);
    for (const [key, v] of Object.entries(value)) {
      if (isObject(node.propertyNames)) errors.push(...validate(key, node.propertyNames as Schema, `${path}.${key}`));
      if (properties[key]) errors.push(...validate(v, properties[key], `${path}.${key}`));
      else if (isObject(node.additionalProperties)) errors.push(...validate(v, node.additionalProperties as Schema, `${path}.${key}`));
      else if (node.additionalProperties === false) fail(`unknown field "${key}"`, `${path}.${key}`);
    }
  }
  if (isObject(node.not) && validate(value, node.not as Schema, path).length === 0) fail('must not match');
  for (const sub of (node.allOf ?? []) as Schema[]) errors.push(...validate(value, sub, path));
  if (Array.isArray(node.oneOf)) {
    const results = (node.oneOf as Schema[]).map((sub) => validate(value, sub, path));
    const matches = results.filter((r) => r.length === 0).length;
    // none match: report the closest branch (fewest errors), which names the real problem
    if (matches === 0) errors.push(...results.reduce((best, r) => (r.length < best.length ? r : best)));
    else if (matches > 1) fail('matches more than one kind');
  }
  return errors;
}

/** Offset in `text` of each object member (its key) and array item, by JSON path. `text` must be valid JSON. */
function positions(text: string): Map<string, number> {
  const at = new Map<string, number>();
  let i = 0;
  const ws = () => { while (/\s/.test(text[i] ?? '')) i++; };
  const string = () => {
    const start = i++;
    while (text[i] !== '"') i += text[i] === '\\' ? 2 : 1;
    return JSON.parse(text.slice(start, ++i)) as string;
  };
  const value = (path: string): void => {
    ws();
    const c = text[i];
    if (c === '{' || c === '[') {
      i++; ws();
      for (let n = 0; text[i] !== '}' && text[i] !== ']'; n++) {
        ws();
        if (c === '[') { at.set(`${path}[${n}]`, i); value(`${path}[${n}]`); } else {
          const start = i, key = string();
          at.set(`${path}.${key}`, start);
          ws(); i++; // the colon
          value(`${path}.${key}`);
        }
        ws();
        if (text[i] === ',') i++;
        ws();
      }
      i++;
    } else if (c === '"') string();
    else while (!/[\s,\]}]/.test(text[i] ?? ',')) i++;
  };
  value('$');
  return at;
}

/** The ids of `ids` nearest to `id` by edit distance, if any is close enough to be a typo. */
function nearest(id: string, ids: Iterable<string>): string | undefined {
  const distance = (a: string, b: string) => {
    let row = Array.from({ length: b.length + 1 }, (_, j) => j);
    for (let i = 1; i <= a.length; i++) {
      const next = [i];
      for (let j = 1; j <= b.length; j++) next[j] = Math.min(row[j]! + 1, next[j - 1]! + 1, row[j - 1]! + (a[i - 1] === b[j - 1] ? 0 : 1));
      row = next;
    }
    return row[b.length]!;
  };
  const best = [...ids].map((c) => [c, distance(id, c)] as const).sort((x, y) => x[1] - y[1])[0];
  return best && best[1] <= Math.max(2, Math.floor(id.length / 3)) ? best[0] : undefined;
}

const onPath = (cmd: string, cwd: string) =>
  cmd.includes('/') ? existsSync(resolve(cwd, cmd)) : (process.env.PATH ?? '').split(delimiter).some((d) => d && existsSync(join(d, cmd)));

/** Reads and parses a network file; throws if it is not JSON. Does not validate. */
export const readNetwork = (file: string): NetworkFile => JSON.parse(readFileSync(file, 'utf8')) as NetworkFile;

/**
 * Everything `check` must report (SPEC 2.7); composed files are checked too, relative to `file`. Errors make the file unusable.
 * `shown` is only for composed files: their path relative to the outer file's directory, which findings name them by.
 */
export function check(file: string, seen: string[] = [], shown?: string): Finding[] {
  const out: Finding[] = [];
  if (!existsSync(file)) return [{ severity: 'error', file, message: 'file not found' }];
  const text = readFileSync(file, 'utf8');
  let net: NetworkFile;
  try {
    net = JSON.parse(text) as NetworkFile;
  } catch (e) {
    return [{ severity: 'error', file: shown ?? file, message: `not valid JSON: ${(e as Error).message}` }];
  }
  const where = positions(text);
  const add = (severity: Finding['severity'], path: string, message: string) => {
    const offset = where.get(path);
    const before = offset === undefined ? undefined : text.slice(0, offset).split('\n');
    out.push({ severity, file: shown ?? file, ...(before && { line: before.length, column: before.at(-1)!.length + 1 }), message });
  };
  const problems = validate(net);
  if (problems.length) {
    for (const p of problems) add('error', p.path, `${p.path}: ${p.message}`);
    return out; // the rest assumes the shape is right
  }
  const ids = new Set<string>();
  net.neurons.forEach((n, i) => {
    if (ids.has(n.id)) add('error', `$.neurons[${i}].id`, `duplicate neuron id "${n.id}"`);
    ids.add(n.id);
  });
  const unknown = (what: string, id: string) => `${what} names unknown neuron "${id}"${nearest(id, ids) ? ` (did you mean "${nearest(id, ids)}"?)` : ''}`;
  const byId = new Map(net.neurons.map((n) => [n.id, n]));
  const links = net.links ?? [];
  const pairs = new Set<string>();
  links.forEach(({ from, to }, i) => {
    for (const side of ['from', 'to'] as const) if (!ids.has(links[i]![side])) add('error', `$.links[${i}].${side}`, unknown('link', links[i]![side]));
    if (pairs.has(`${from}\0${to}`)) add('error', `$.links[${i}]`, `link ${from} -> ${to} is listed twice (links are one-way: a two-way link is two entries, once each)`);
    pairs.add(`${from}\0${to}`);
    if (byId.get(to)?.source) add('error', `$.links[${i}]`, `link ${from} -> ${to}: a source receives nothing`);
  });
  // a cost names units the file's budget has (SPEC 2.7); amounts are integers from 0 (the schema)
  const units = Object.keys(net.budget ?? {});
  net.neurons.forEach((n, i) => {
    for (const unit of Object.keys(n.cost ?? {})) {
      if (units.includes(unit)) {
        if (n.cost![unit]! > net.budget![unit]!) add('warning', `$.neurons[${i}].cost.${unit}`, `neuron "${n.id}" costs ${n.cost![unit]} ${unit}, more than the budget's ${net.budget![unit]}: it can never run`);
        continue;
      }
      const near = nearest(unit, units);
      add('error', `$.neurons[${i}].cost.${unit}`, `cost names unit "${unit}", which is not in the budget${units.length ? ` (${units.join(', ')})` : ' (the file has none)'}${near ? `: did you mean "${near}"?` : ''}`);
    }
  });
  // with a budget every neuron but a source declares a cost, 0 allowed: a neuron that could spend without one would escape it (SPEC 10.1)
  if (net.budget) net.neurons.forEach((n, i) => { if (!n.source && !n.cost) add('error', `$.neurons[${i}]`, `neuron "${n.id}" has no cost, and the file has a budget: every neuron but a source declares one (0 is allowed)`); });
  for (const key of ['entry', 'exit'] as const) if (net[key] && !ids.has(net[key]!)) add('error', `$.${key}`, unknown(`"${key}"`, net[key]!));
  // a cycle in which no neuron has maxHops never ends
  const open = (id: string) => byId.get(id)?.maxHops === undefined;
  const state = new Map<string, 1 | 2>();
  const visit = (id: string): boolean => {
    if (state.get(id) === 1) return true;
    if (state.get(id) === 2) return false;
    state.set(id, 1);
    const loops = links.some((l) => l.from === id && open(l.to) && open(id) && visit(l.to));
    state.set(id, 2);
    return loops;
  };
  const looped = net.neurons.findIndex((n) => open(n.id) && visit(n.id));
  if (looped >= 0) add('error', `$.neurons[${looped}]`, 'a cycle with no maxHops on any neuron in it');
  const here = resolve(file);
  net.neurons.forEach((n, i) => {
    const at = `$.neurons[${i}]`;
    const cwd = resolve(dirname(file), n.cwd ?? '.');
    if (n.cwd !== undefined && !existsSync(cwd)) add('error', `${at}.cwd`, `working directory "${n.cwd}" not found`);
    const cmd = n.run?.[0] ?? n.serve?.[0];
    if (cmd && !onPath(cmd, cwd)) add('warning', `${at}.${n.run ? 'run' : 'serve'}`, `executable "${cmd}" not found`);
    if (n.http) add('warning', at, `neuron "${n.id}" is http: this engine runs source, run, serve, network and human neurons only`);
    if (n.network === undefined) return;
    const inner = resolve(dirname(file), n.network);
    if (seen.includes(inner) || inner === here) return add('error', `${at}.network`, 'a network file may not contain itself');
    if (!existsSync(inner)) return add('error', `${at}.network`, `file not found: ${n.network}`);
    const found = check(inner, [...seen, here], normalize(join(dirname(shown ?? '.'), n.network)));
    out.push(...found);
    if (found.some((f) => f.severity === 'error')) return;
    const sub = readNetwork(inner);
    if (sub.neurons.some((x) => x.id === n.id)) add('error', `${at}.id`, `neuron "${n.id}" has the same id as a neuron inside ${n.network}: a composed neuron's id must not be one of its inner ids`);
    for (const key of ['entry', 'exit'] as const) if (!sub[key]) add('error', `${at}.network`, `${n.network} is used as a composed neuron and needs "${key}"`);
    if (sub.neurons.some((x) => x.effect)) add('error', `${at}.network`, `${n.network} is used as a composed neuron and may not contain an effect neuron (SPEC 11.5)`);
    if (sub.neurons.some((x) => x.source)) add('error', `${at}.network`, `${n.network} is used as a composed neuron and may not contain a source`);
    const lacking = [...new Set(sub.neurons.flatMap((x) => Object.keys(x.cost ?? {})))].filter((u) => n.cost && !(u in n.cost));
    if (lacking.length) add('warning', at, `${n.network} has neurons costing ${lacking.join(', ')}, which the cost of "${n.id}" does not name: those calls are always refused (SPEC 10.5)`);
    if (sub.exit && (sub.links ?? []).some((l) => l.from === sub.exit)) add('error', `${at}.network`, `${n.network}: "exit" may not have outgoing links`);
  });
  return out;
}

export const formatFinding = (f: Finding) => `${f.file}${f.line ? `:${f.line}:${f.column}` : ''}: ${f.severity}: ${f.message}`;
