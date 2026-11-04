// A tiny validator for spec/network.schema.json and the checks a schema cannot make.
// shortcut: implements only the JSON Schema keywords that file uses (listed in `check` below), nothing else;
// upgrade to a real validator if the schema grows keywords beyond these.
import { existsSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';

type Schema = { [key: string]: unknown };
export const schemaPath = new URL('../../spec/network.schema.json', import.meta.url);
export const schema = JSON.parse(readFileSync(schemaPath, 'utf8')) as Schema;

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/** Errors for `value` against `node` of the schema; each starts with a JSON path. */
export function validate(value: unknown, node: Schema = schema, path = '$'): string[] {
  const root = schema;
  if (typeof node.$ref === 'string') {
    const target = node.$ref.replace('#/', '').split('/').reduce<unknown>((n, k) => (n as Schema)[k], root);
    return validate(value, target as Schema, path);
  }
  const errors: string[] = [];
  const fail = (message: string) => errors.push(`${path}: ${message}`);
  const type = node.type;
  if (type === 'object' && !isObject(value)) return [`${path}: expected an object`];
  if (type === 'array' && !Array.isArray(value)) return [`${path}: expected an array`];
  if (type === 'string' && typeof value !== 'string') return [`${path}: expected a string`];
  if (type === 'integer' && !Number.isInteger(value)) return [`${path}: expected an integer`];
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
    for (const [key, v] of Object.entries(value)) {
      if (properties[key]) errors.push(...validate(v, properties[key], `${path}.${key}`));
      else if (node.additionalProperties === false) fail(`unknown field "${key}"`);
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

type Neuron = Record<string, unknown> & { id: string };
type Network = { neurons: Neuron[]; links?: { from: string; to: string }[]; entry?: string; exit?: string };

/**
 * Everything `check` must report that the schema alone cannot: ids, links, composed files, cycles.
 * `file` is the network file's path; composed files are read relative to it.
 */
export function checkFile(file: string, seen: string[] = []): string[] {
  const where = (m: string) => `${file}: ${m}`;
  if (!existsSync(file)) return [where('file not found')];
  if (seen.includes(file)) return [where('a network file may not contain itself')];
  const net = JSON.parse(readFileSync(file, 'utf8')) as Network;
  const errors = validate(net).map(where);
  if (errors.length) return errors; // the rest assumes the shape is right
  const ids = new Set<string>();
  for (const n of net.neurons) {
    if (ids.has(n.id)) errors.push(where(`duplicate neuron id "${n.id}"`));
    ids.add(n.id);
  }
  const byId = new Map(net.neurons.map((n) => [n.id, n]));
  const links = net.links ?? [];
  const pairs = new Set<string>();
  for (const { from, to } of links) {
    for (const id of [from, to]) if (!ids.has(id)) errors.push(where(`link names unknown neuron "${id}"`));
    if (pairs.has(`${from}\0${to}`)) errors.push(where(`link ${from} -> ${to} is listed twice (links are one-way: a two-way link is two entries, once each)`));
    pairs.add(`${from}\0${to}`);
    if (byId.get(to)?.source) errors.push(where(`link ${from} -> ${to}: a source receives nothing`));
  }
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
  if (net.neurons.some((n) => open(n.id) && visit(n.id))) errors.push(where('a cycle with no maxHops on any neuron in it'));
  for (const n of net.neurons) {
    if (typeof n.network !== 'string') continue;
    const inner = resolve(dirname(file), n.network);
    const found = checkFile(inner, [...seen, file]);
    errors.push(...found.map((e) => `${n.id} -> ${e}`));
    if (found.length) continue;
    const sub = JSON.parse(readFileSync(inner, 'utf8')) as Network;
    for (const key of ['entry', 'exit'] as const) {
      if (!sub[key]) errors.push(where(`${n.network} is used as a composed neuron and needs "${key}"`));
      else if (!sub.neurons.some((x) => x.id === sub[key])) errors.push(where(`${n.network}: "${key}" names unknown neuron "${sub[key]}"`));
    }
    if (sub.neurons.some((x) => x.source)) errors.push(where(`${n.network} is used as a composed neuron and may not contain a source`));
    if (sub.exit && (sub.links ?? []).some((l) => l.from === sub.exit)) errors.push(where(`${n.network}: "exit" may not have outgoing links`));
  }
  return errors;
}
