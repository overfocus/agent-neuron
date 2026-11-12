// Names the repo one business type gets. Pure: reads the payload and the settings, answers the plan.
// In:  { type }                            (anything else in the payload is ignored)
// Out: { id, repo, template, type }        id is the repo's full name: the effect downstream is idempotent by it (SPEC 11.4)
import { readFileSync } from 'node:fs';
// Its settings arrive in its environment: the engine fills the names in this neuron's `env` from
// `.env` in this folder (git-ignored; `env.example` shows the names), SPEC 2.4.
const setting = (name) => {
  const v = process.env[name];
  if (!v) { console.error(`${name} is not set: put it in this neuron's .env`); process.exit(1); }
  return v;
};

const p = JSON.parse(readFileSync(0, 'utf8'));
const owner = setting('TEMPLATES_OWNER');
const source = setting('TEMPLATES_SOURCE');
const bad = (what) => { console.error(`plan: ${what}`); process.exit(1); };
if (!/^[A-Za-z0-9](?:[A-Za-z0-9-]{0,37}[A-Za-z0-9])?$/.test(owner)) bad('TEMPLATES_OWNER is not an account name');
if (!/^[A-Za-z0-9._-]{1,100}$/.test(source)) bad('TEMPLATES_SOURCE is not a repo name');
if (typeof p?.type !== 'string' || !/^[a-z0-9](?:[a-z0-9-]{0,60}[a-z0-9])?$/.test(p.type)) bad('type is not a slug');

const repo = `${owner}/${p.type}-template`;
console.log(JSON.stringify({ id: repo, repo, template: `${owner}/${source}`, type: p.type }));
