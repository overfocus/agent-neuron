import { createInterface } from 'node:readline';
import type { Readable, Writable } from 'node:stream';
import type { Adapter, AdapterContext } from '../adapter.js';
import { serialize, text, type Payload, type Signal } from '../signal.js';

/** Shows the payload as a question and answers with the next line typed. */
export class HumanAdapter implements Adapter {
  constructor(
    readonly input: Readable = process.stdin,
    readonly output: Writable = process.stdout,
  ) {}

  // shortcut: one readline per question can drop lines buffered after the first; share one interface if humans get chatty.
  handle(payload: Payload, _signal?: Signal, context?: AdapterContext): Promise<Payload> {
    return new Promise((resolve, reject) => {
      const rl = createInterface({ input: this.input });
      const stop = () => {
        reject(context?.abort.reason);
        rl.close();
      };
      rl.once('line', (line) => {
        resolve(text(line));
        rl.close();
      });
      rl.once('close', () => {
        context?.abort.removeEventListener('abort', stop);
        reject(new Error('input closed before an answer'));
      });
      if (context?.abort.aborted) return stop();
      context?.abort.addEventListener('abort', stop, { once: true });
      this.output.write(`${serialize(payload)}\n> `);
    });
  }
}
