// Types for server.mjs, so the test can import it under strict TypeScript.
import type { Server } from 'node:http';

export const scenarios: Record<string, { label: string; holds: Record<string, number> }>;
export function startViewServer(lib: typeof import('../../src/index.js'), port?: number): Promise<Server>;
