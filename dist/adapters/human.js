import { createInterface } from 'node:readline';
import { serialize, text } from '../signal.js';
/** Shows the payload as a question and answers with the next line typed. */
export class HumanAdapter {
    input;
    output;
    constructor(input = process.stdin, output = process.stdout) {
        this.input = input;
        this.output = output;
    }
    // shortcut: one readline per question can drop lines buffered after the first; share one interface if humans get chatty.
    handle(payload, _signal, context) {
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
            if (context?.abort.aborted)
                return stop();
            context?.abort.addEventListener('abort', stop, { once: true });
            this.output.write(`${serialize(payload)}\n> `);
        });
    }
}
//# sourceMappingURL=human.js.map