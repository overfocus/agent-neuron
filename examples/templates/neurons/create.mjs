// Makes the repo: private, generated from the template, and itself marked as a template. The effect.
// In:  { id, repo, template, type }
// Out: { ...in, action: 'create' | 'mark' | 'none', applied }
//
// It does nothing unless TEMPLATES_APPLY=1: without it, it reads GitHub and answers what it would do.
// Idempotent by the repo's name: a repo that is already there and right is left alone ('none'), one that
// is there but not yet a template is only marked ('mark'). A repo of that name that is public, or that
// was not generated from this template, is somebody else's: it is refused, never changed.
import { readFileSync } from 'node:fs';
import { gh, view } from './gh.mjs';

const p = JSON.parse(readFileSync(0, 'utf8'));
const apply = process.env.TEMPLATES_APPLY === '1';
const stop = (what) => { console.error(`create: ${what}`); process.exit(1); };

const source = view(p.template);
if (!source) stop(`no such template repo: ${p.template}`);
if (!source.isTemplate) stop(`${p.template} is not marked as a template`);

const have = view(p.repo);
if (have && !have.private) stop(`${p.repo} exists and is not private`);
if (have && have.from !== source.repo) stop(`${p.repo} exists and was not generated from ${source.repo}`);
const action = !have ? 'create' : have.isTemplate ? 'none' : 'mark';

if (apply) {
  if (action === 'create') gh('repo', 'create', p.repo, '--private', '--template', p.template);
  if (action !== 'none') gh('api', '--silent', '-X', 'PATCH', `repos/${p.repo}`, '-F', 'is_template=true');
}
console.log(JSON.stringify({ ...p, action, applied: apply }));
