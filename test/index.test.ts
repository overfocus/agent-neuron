import { describe, expect, it } from 'vitest';
import { VERSION } from '../src/index.js';

describe('agent-neuron', () => {
  it('exports a version', () => {
    expect(VERSION).toMatch(/^\d+\.\d+\.\d+$/);
  });
});
