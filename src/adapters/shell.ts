import { spawn } from 'node:child_process';
import type { Adapter, AdapterContext } from '../adapter.js';
import { parsePayload, serialize, type Payload, type Signal } from '../signal.js';

export class ShellError extends Error {
  constructor(
    readonly exitCode: number | null,
    readonly stderr: string,
  ) {
    super(`command exited with ${exitCode}: ${stderr.trim()}`);
    this.name = 'ShellError';
  }
}

/** Runs a command with the payload on stdin; stdout is the answer. No shell is involved. */
export class ShellAdapter implements Adapter {
  constructor(
    readonly command: string,
    readonly args: string[] = [],
  ) {}

  handle(payload: Payload, _signal?: Signal, context?: AdapterContext): Promise<Payload> {
    return new Promise((resolve, reject) => {
      const child = spawn(this.command, this.args, { stdio: ['pipe', 'pipe', 'pipe'] });
      let stdout = '';
      let stderr = '';
      child.stdout.setEncoding('utf8').on('data', (d: string) => (stdout += d));
      child.stderr.setEncoding('utf8').on('data', (d: string) => (stderr += d));
      child.stdin.on('error', () => {}); // the command may exit without reading stdin
      child.on('error', reject);
      const stop = () => {
        child.kill();
        reject(context?.abort.reason);
      };
      if (context?.abort.aborted) stop();
      else context?.abort.addEventListener('abort', stop, { once: true });
      child.on('close', (code) => {
        context?.abort.removeEventListener('abort', stop);
        if (code === 0) resolve(parsePayload(stdout.trim()));
        else reject(new ShellError(code, stderr));
      });
      child.stdin.end(serialize(payload));
    });
  }
}
