import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { Network, Neuron, ShellAdapter, text, type Signal } from '../src/index.js';

const binary = fileURLToPath(new URL('../rust/target/release/music', import.meta.url));
process.env.MUSIC_NO_LATENCY = '1'; // the lyrics neuron's simulated wait is for the demo, not the tests

/** A box wired to the given composed neurons. Records every signal onSignal sees. */
function setup(...musicIds: string[]) {
  const seen: Signal[] = [];
  const network = new Network({ onSignal: (s) => seen.push(s) });
  network.add(new Neuron('box', { handle: async () => text('') }));
  for (const id of musicIds) {
    network.add(new Neuron(id, new ShellAdapter(binary)));
    network.connect('box', id);
  }
  return { network, seen };
}

describe('a composed neuron: the Rust `music` program with its own inner network', () => {
  it('has been built', () => {
    expect(existsSync(binary), `${binary} is missing: run \`npm run rust:build\` (needs a Rust toolchain)`).toBe(true);
  });

  it('seals the inside: inner neurons are not in the network and cannot be linked or addressed', async () => {
    const { network } = setup('music');
    for (const inner of ['tracks', 'artist', 'lyrics']) {
      expect(() => network.connect('box', inner)).toThrow(`unknown neuron: ${inner}`);
      await expect(network.send('box', text('lumen harbor'), { to: [inner] })).rejects.toThrow(
        `unknown neuron: ${inner}`,
      );
    }
  });

  it('never lets an inner id reach the outer network or its answers', async () => {
    const { network, seen } = setup('music');
    network.emit('box', text('lumen harbor'));
    await network.idle();
    // the inner signal ids, as music --inspect prints them for the same query
    const inside = spawnSync(binary, ['--inspect'], { input: 'lumen harbor', encoding: 'utf8' });
    const ids = [...inside.stderr.matchAll(/^signal (\d+) /gm)].map((m) => m[1]!);
    expect(ids.length).toBeGreaterThan(0);
    const inner = ['in', 'out', 'tracks', 'artist', 'lyrics'];
    for (const s of seen) {
      expect(inner).not.toContain(s.from);
      for (const t of s.to ?? []) expect(inner).not.toContain(t);
    }
    const answer = JSON.stringify(seen[1]!.payload);
    expect(answer).not.toMatch(/trace|replyTo|signal/);
    for (const id of ids) expect(answer).not.toMatch(new RegExp(`\\b${id}\\b`));
  });

  it('shows only a signal in and an answer out, with no inner ids or trace', async () => {
    const { network, seen } = setup('music');
    const request = network.emit('box', text('lumen harbor'));
    await network.idle();

    expect(seen).toHaveLength(2);
    const [req, answer] = seen as [Signal, Signal];
    expect(req.id).toBe(request.id);
    expect(req.from).toBe('box');
    expect(answer.from).toBe('music');
    expect(answer.replyTo).toBe(request.id);
    expect(answer.trace).toBe(request.trace);

    // the answer's content is the inner results, nothing else
    expect(answer.payload.type).toBe('json');
    const data = (answer.payload as { data: Record<string, unknown> }).data;
    expect(Object.keys(data).sort()).toEqual(['artist', 'lyrics', 'tracks']);
    expect(data).toEqual({
      tracks: ['Glass Orchard', 'Ninefold Morning'],
      artist: 'Lumen Harbor',
      lyrics: 'carry me home to the lumen harbor',
    });
  });

  it('stays quiet below the threshold', async () => {
    const { network, seen } = setup('music');
    network.emit('box', text('lu'));
    await network.idle();
    expect(seen.map((s) => s.from)).toEqual(['box', 'music']);
    expect(seen[1]?.payload).toEqual(text(''));
  });

  it('two uses are two neurons that answer independently', async () => {
    const { network, seen } = setup('music-a', 'music-b');
    const a = network.emit('box', text('lumen harbor'), { to: ['music-a'] });
    const b = network.emit('box', text('quiet k'), { to: ['music-b'] });
    await network.idle();

    const answerTo = (id: string) => seen.find((s) => s.replyTo === id)!;
    expect(answerTo(a.id).from).toBe('music-a');
    expect(answerTo(b.id).from).toBe('music-b');
    expect((answerTo(a.id).payload as { data: { tracks: string[] } }).data.tracks).toEqual([
      'Glass Orchard',
      'Ninefold Morning',
    ]);
    expect((answerTo(b.id).payload as { data: { tracks: string[]; artist: unknown } }).data).toMatchObject({
      tracks: ['Quiet Kettles'],
      artist: null,
    });
  });
});
