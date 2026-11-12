// Looks at what is on GitHub after the effect and answers the record: the run's result for one type.
// In:  { repo, template, action, applied }
// Out: { repo, ok: true, action }  or, when nothing was applied, { repo, dry: true, action }
// A repo that is missing, public, not a template, still empty or not from the template fails the call (exit 1).
import { readFileSync } from 'node:fs';
import { view } from './gh.mjs';

const p = JSON.parse(readFileSync(0, 'utf8'));
if (!p.applied) {
  console.log(JSON.stringify({ repo: p.repo, dry: true, action: p.action }));
  process.exit(0);
}
// GitHub fills a generated repo a moment after it makes it: wait for the content, up to 20 seconds.
let r = view(p.repo);
for (let i = 0; r?.empty && i < 10; i++) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 2000);
  r = view(p.repo);
}
const wrong = !r ? 'is not there' : !r.private ? 'is not private' : !r.isTemplate ? 'is not a template'
  : r.empty ? 'is still empty'
  : r.from?.toLowerCase() !== p.template.toLowerCase() ? `was not generated from ${p.template}` : null;
if (wrong) { console.error(`verify: ${p.repo} ${wrong}`); process.exit(1); }
console.log(JSON.stringify({ repo: p.repo, ok: true, action: p.action }));
