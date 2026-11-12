import { spawn } from 'node:child_process';
import { parsePayload, serialize } from '../signal.js';
export class ShellError extends Error {
    exitCode;
    stderr;
    constructor(exitCode, stderr) {
        super(`command exited with ${exitCode}: ${stderr.trim()}`);
        this.exitCode = exitCode;
        this.stderr = stderr;
        this.name = 'ShellError';
    }
}
/** Runs a command with the payload on stdin; stdout is the answer. No shell is involved. */
export class ShellAdapter {
    command;
    args;
    constructor(command, args = []) {
        this.command = command;
        this.args = args;
    }
    handle(payload, _signal, context) {
        return new Promise((resolve, reject) => {
            const child = spawn(this.command, this.args, { stdio: ['pipe', 'pipe', 'pipe'] });
            let stdout = '';
            let stderr = '';
            child.stdout.setEncoding('utf8').on('data', (d) => (stdout += d));
            child.stderr.setEncoding('utf8').on('data', (d) => (stderr += d));
            child.stdin.on('error', () => { }); // the command may exit without reading stdin
            child.on('error', reject);
            const stop = () => {
                child.kill();
                reject(context?.abort.reason);
            };
            if (context?.abort.aborted)
                stop();
            else
                context?.abort.addEventListener('abort', stop, { once: true });
            child.on('close', (code) => {
                context?.abort.removeEventListener('abort', stop);
                if (code === 0)
                    resolve(parsePayload(stdout.trim()));
                else
                    reject(new ShellError(code, stderr));
            });
            child.stdin.end(serialize(payload));
        });
    }
}
//# sourceMappingURL=shell.js.map