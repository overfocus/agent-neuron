// Looks at what is on GitHub after the effect and answers the record: the run's result for one type.
// In:  { repo, template, action, applied }
// Out: { repo, ok: true, action }  or, when nothing was applied, { repo, dry: true, action }
// A repo that is missing, public, not a template, still empty or not from the template fails the call (exit 1).
import { readFileSync } from 'node:fs';
import { setTimeout as sleep } from 'node:timers/promises';
import { empty, view } from './settings.mjs';

const p = JSON.parse(readFileSync(0, 'utf8'));
if (!p.applied) {
  console.log(JSON.stringify({ repo: p.repo, dry: true, action: p.action }));
  process.exit(0);
}
try {
  const r = await view(p.repo);
  // GitHub fills a generated repo a moment after it makes it: wait for the content, up to 20 seconds.
  let hollow = r ? await empty(p.repo) : false;
  for (let i = 0; hollow && i < 10; i++) {
    await sleep(2000);
    hollow = await empty(p.repo);
  }
  const wrong = !r ? 'is not there' : !r.private ? 'is not private' : !r.isTemplate ? 'is not a template'
    : hollow ? 'is still empty'
    : r.from?.toLowerCase() !== p.template.toLowerCase() ? `was not generated from ${p.template}` : null;
  if (wrong) throw new Error(`${p.repo} ${wrong}`);
  console.log(JSON.stringify({ repo: p.repo, ok: true, action: p.action }));
} catch (e) {
  console.error(`verify: ${e.message}`);
  process.exit(1);
}
