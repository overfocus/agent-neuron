// Level 0 neuron with a side effect: reads a build update (JSON) on stdin, takes 300 ms to "send a message", records the
// build id in the file named by SENT_LOG, and answers with the id. An update with "flaky": true loses its connection after
// the message left: the process is killed before it can answer, so the engine cannot know the message was sent.
import { appendFileSync, readFileSync } from 'node:fs';

const build = JSON.parse(readFileSync(0, 'utf8'));
await new Promise((done) => setTimeout(done, 300));
appendFileSync(process.env.SENT_LOG, `${build.id}\n`);
if (build.flaky) process.kill(process.pid, 'SIGKILL');
console.log(JSON.stringify({ sent: build.id }));
