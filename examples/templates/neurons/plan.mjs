// Names the repo one business type gets. Pure: reads the payload, answers the plan, touches nothing.
// In:  { owner, template, type }           (anything else in the payload is ignored)
// Out: { id, repo, template, type }        id is the repo's full name: the effect downstream is idempotent by it (SPEC 11.4)
import { readFileSync } from 'node:fs';

const p = JSON.parse(readFileSync(0, 'utf8'));
const bad = (what) => { console.error(`plan: ${what}`); process.exit(1); };
if (typeof p?.owner !== 'string' || !/^[A-Za-z0-9](?:[A-Za-z0-9-]{0,37}[A-Za-z0-9])?$/.test(p.owner)) bad('owner is not an account name');
if (typeof p.template !== 'string' || !/^[A-Za-z0-9._-]{1,100}$/.test(p.template)) bad('template is not a repo name');
if (typeof p.type !== 'string' || !/^[a-z0-9](?:[a-z0-9-]{0,60}[a-z0-9])?$/.test(p.type)) bad('type is not a slug');

const repo = `${p.owner}/${p.type}-template`;
console.log(JSON.stringify({ id: repo, repo, template: `${p.owner}/${p.template}`, type: p.type }));
