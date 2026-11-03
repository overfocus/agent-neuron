import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { ShellAdapter, json, text } from '../src/index.js';

const binary = fileURLToPath(new URL('../rust/target/release/catalogue', import.meta.url));

describe('a Rust program wrapped by the shell adapter', () => {
  it('has been built', () => {
    expect(existsSync(binary), `${binary} is missing: run \`npm run rust:build\` (needs a Rust toolchain)`).toBe(true);
  });

  it('answers a query with a json payload of titles', async () => {
    const answer = await new ShellAdapter(binary).handle(text('LUM'));
    expect(answer).toEqual(json(['Glass Orchard', 'Ninefold Morning']));
  });

  it('stays quiet for a short query', async () => {
    expect(await new ShellAdapter(binary).handle(text('lu'))).toEqual(text(''));
  });

  it('answers [] for no match, and keeps spaces in the query', async () => {
    const adapter = new ShellAdapter(binary);
    expect(await adapter.handle(text('zzzz'))).toEqual(json([]));
    expect(await adapter.handle(text(' lum '))).toEqual(json([]));
  });
});
