import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { check, validate } from '../src/engine/index.js';

const examples = fileURLToPath(new URL('../spec/examples/', import.meta.url));
const schema = JSON.parse(readFileSync(new URL('../spec/network.schema.json', import.meta.url), 'utf8')) as Record<string, any>;
/** The messages of the errors `engine check` finds in `file` (warnings are not errors). */
const checkFile = (file: string) => check(file).filter((f) => f.severity === 'error').map((f) => f.message);
const read = (name: string) => JSON.parse(readFileSync(join(examples, name), 'utf8')) as Record<string, any>;

/** Write a copy of the alert storm (and its composed file) with `edit` applied to the outer file; return the errors `check` finds. */
function broken(edit: (net: Record<string, any>) => void, inner?: (net: Record<string, any>) => void): string[] {
  const dir = mkdtempSync(join(tmpdir(), 'spec-'));
  const outer = read('alerts.network.json');
  edit(outer);
  const sub = read('diagnosis.network.json');
  inner?.(sub);
  writeFileSync(join(dir, 'alerts.network.json'), JSON.stringify(outer));
  writeFileSync(join(dir, 'diagnosis.network.json'), JSON.stringify(sub));
  return checkFile(join(dir, 'alerts.network.json'));
}

describe('network schema v0', () => {
  it('is closed and versioned', () => {
    expect(schema.$schema).toContain('json-schema.org');
    expect(schema.$id).toBe('urn:agent-neuron:schema:network:0');
    expect(schema.additionalProperties).toBe(false);
    expect(validate({ version: 0, neurons: [{ id: 'a', source: true }] })).toEqual([]);
    expect(validate({ version: 1, neurons: [{ id: 'a', source: true }] })).not.toEqual([]);
    // every neuron kind is a closed object
    const kinds = (schema.$defs as any).neuron.oneOf as Record<string, unknown>[];
    expect(kinds.every((k) => k.additionalProperties === false)).toBe(true);
  });

  it('accepts the alert storm and its composed neuron, files and all', () => {
    expect(checkFile(join(examples, 'alerts.network.json'))).toEqual([]);
    expect(checkFile(join(examples, 'diagnosis.network.json'))).toEqual([]);
    expect(checkFile(join(examples, 'alerts-python.network.json'))).toEqual([]);
    expect(check(join(examples, 'alerts-python-serve.network.json')).filter((f) => !f.message.startsWith('executable'))).toEqual([]);
    expect(check(join(examples, 'alerts-python.network.json')).filter((f) => !f.message.startsWith('executable'))).toEqual([]); // no other warnings (python3 itself may be absent)
  });

  it('reports file, line and column, and suggests the nearest id', () => {
    const dir = mkdtempSync(join(tmpdir(), 'spec-'));
    const file = join(dir, 'a.network.json');
    const net = { version: 0, neurons: [{ id: 'logs', run: ['true'] }, { id: 'hypothesis', run: ['true'] }], links: [{ from: 'logs', to: 'hypothsis' }] };
    writeFileSync(file, JSON.stringify(net, null, 2));
    expect(check(file)).toEqual([{ severity: 'error', file, line: 20, column: 7, message: 'link names unknown neuron "hypothsis" (did you mean "hypothesis"?)' }]);
    writeFileSync(file, '{ "version": 0, "neurons": [');
    expect(check(file)[0]?.message).toContain('not valid JSON');
  });

  it('names a composed file relative to the outer file, and checks that cwd exists', () => {
    const dir = mkdtempSync(join(tmpdir(), 'spec-'));
    writeFileSync(join(dir, 'in.network.json'), JSON.stringify({ version: 0, entry: 'a', exit: 'a', neurons: [{ id: 'a', run: ['true'], cwd: 'nowhere' }] }));
    writeFileSync(join(dir, 'out.network.json'), JSON.stringify({ version: 0, neurons: [{ id: 'c', network: 'in.network.json' }] }));
    expect(check(join(dir, 'out.network.json'))).toEqual([expect.objectContaining({ severity: 'error', file: 'in.network.json', message: 'working directory "nowhere" not found' })]);
  });

  it('warns, and does not fail, for an executable it cannot find', () => {
    const dir = mkdtempSync(join(tmpdir(), 'spec-'));
    const file = join(dir, 'a.network.json');
    writeFileSync(file, JSON.stringify({ version: 0, neurons: [{ id: 'a', run: ['no-such-program-here'] }] }));
    expect(check(file)).toEqual([expect.objectContaining({ severity: 'warning', message: 'executable "no-such-program-here" not found' })]);
  });

  it('refuses an unknown field, at the top and on a neuron', () => {
    expect(broken((n) => (n.when = 'severity == crit'))).toEqual([expect.stringContaining('unknown field "when"')]);
    expect(broken((n) => (n.neurons[1].filter = 'crit'))).toEqual([expect.stringContaining('unknown field "filter"')]);
  });

  it('refuses the common interpolation syntaxes where a literal is expected (and only those)', () => {
    expect(broken((n) => (n.neurons[1].run = ['sh', 'detect.sh', '${service}'])).join()).toContain('must not match');
    expect(broken((n) => (n.neurons[1].cwd = '{{service}}')).join()).toContain('must not match');
    // shortcut by design: a string like "service == 'gateway'" is a valid literal argument; the schema cannot tell
    expect(broken((n) => (n.neurons[1].run = ['sh', 'detect.sh', "service == 'gateway'"]))).toEqual([]);
  });

  it('needs entry and exit together', () => {
    expect(broken((n) => (n.entry = 'logs')).join()).toContain('"entry" needs "exit"');
    expect(broken((n) => (n.exit = 'logs')).join()).toContain('"exit" needs "entry"');
  });

  it('refuses a neuron of two kinds, an option on the wrong kind, and within without threshold', () => {
    expect(broken((n) => (n.neurons[1].http = 'http://127.0.0.1:9000')).length).toBeGreaterThan(0);
    expect(broken((n) => (n.neurons[0].threshold = 2)).join()).toContain('unknown field "threshold"');
    expect(broken((n) => (n.neurons[1].within = 100)).join()).toContain('"within" needs "threshold"');
    expect(broken((n) => (n.neurons[5].threshold = 2)).join()).toContain('unknown field "threshold"');
  });

  it('finds a link naming a missing neuron, a duplicated one-way link, a link into a source', () => {
    expect(broken((n) => n.links.push({ from: 'logs', to: 'nobody' }))).toEqual([expect.stringContaining('unknown neuron "nobody"')]);
    expect(broken((n) => n.links.push({ from: 'alerts', to: 'logs' }))).toEqual([expect.stringContaining('listed twice')]);
    expect(broken((n) => n.links.push({ from: 'logs', to: 'alerts' })).join()).toContain('a source receives nothing');
  });

  it('finds duplicate ids and a cycle with no maxHops, and accepts one with maxHops', () => {
    expect(broken((n) => n.neurons.push({ id: 'logs', run: ['true'] })).join()).toContain('duplicate neuron id "logs"');
    const loop = (n: Record<string, any>) => n.links.push({ from: 'diagnosis', to: 'hypothesis' });
    expect(broken(loop).join()).toContain('a cycle with no maxHops');
    expect(broken((n) => (loop(n), (n.neurons[4].maxHops = 6)))).toEqual([]);
  });

  it('finds a composed network file that is missing, or lacks entry or exit', () => {
    expect(broken((n) => (n.neurons[5].network = 'gone.network.json')).join()).toContain('file not found');
    expect(broken(() => {}, (s) => delete s.entry).join()).toContain('needs "entry"');
    expect(broken(() => {}, (s) => (s.exit = 'nobody')).join()).toContain('names unknown neuron "nobody"');
    expect(broken(() => {}, (s) => (s.neurons.push({ id: 'extra', run: ['true'] }), s.links.push({ from: 'check', to: 'extra' }))).join()).toContain('"exit" may not have outgoing links');
  });
});

describe('the alert storm programs (level 0)', () => {
  const run = (cmd: string[], stdin: unknown) => {
    const out = spawnSync(cmd[0] as string, cmd.slice(1), { cwd: examples, input: JSON.stringify(stdin), encoding: 'utf8' });
    expect(out.status).toBe(0);
    return out.stdout.trim() ? JSON.parse(out.stdout) : undefined;
  };
  const neuron = (id: string) => read('alerts.network.json').neurons.find((n: any) => n.id === id).run as string[];
  const inner = (id: string) => read('diagnosis.network.json').neurons.find((n: any) => n.id === id).run as string[];
  // the walk-through's alert: a warning for checkout, whose root cause is the gateway
  const alert = { id: 5, at: 530, service: 'checkout', severity: 'warn', message: 'cart submit failing for some users' };

  it('follows one alert to a verdict that carries the whole context', () => {
    const logs = run(neuron('logs'), alert);
    const deploys = run(neuron('deploys'), alert);
    expect(run(neuron('metrics'), alert)).toBeUndefined(); // quiet: not critical
    expect(logs).toMatchObject({ detector: 'logs', alert: 5, service: 'checkout' });
    expect(deploys).toMatchObject({ detector: 'deploys', alert: 5, service: 'checkout' });
    const hypothesis = run([...neuron('hypothesis').slice(0, -1), '0'], { payload: deploys, inputs: [logs, deploys] });
    expect(hypothesis).toEqual({ alert: 5, service: 'checkout', detectors: ['deploys', 'logs'] });
    const verdict = run(inner('check'), run(inner('topology'), hypothesis));
    expect(verdict).toEqual({ alert: 5, service: 'checkout', detectors: ['deploys', 'logs'], root: 'gateway', deploy: 'dep-8b21', minutes: 14 });
  });
});
