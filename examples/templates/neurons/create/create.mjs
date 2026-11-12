// Makes the repo: private, generated from the template, and itself marked as a template. The effect.
// In:  { id, repo, template, type }
// Out: { ...in, action: 'create' | 'mark' | 'none', applied }
//
// It does nothing unless TEMPLATES_APPLY=1: without it, it reads GitHub and answers what it would do.
// Idempotent by the repo's name: a repo that is already there and right is left alone ('none'), one that
// is there but not yet a template is only marked ('mark'). A repo of that name that is public, or that
// was not generated from this template, is somebody else's: it is refused, never changed.
import { readFileSync } from 'node:fs';
// Its settings arrive in its environment: the engine fills the names in this neuron's `env` from
// `.env` in this folder (git-ignored; `env.example` shows the names), SPEC 2.4.
const setting = (name) => {
  const v = process.env[name];
  if (!v) { console.error(`${name} is not set: put it in this neuron's .env`); process.exit(1); }
  return v;
};

// GitHub's API, asked directly: no command-line tool.
async function api(method, path, body) {
  const res = await fetch(`https://api.github.com${path}`, {
    method,
    headers: {
      accept: 'application/vnd.github+json',
      authorization: `Bearer ${setting('GITHUB_TOKEN')}`,
      'x-github-api-version': '2022-11-28',
      'user-agent': 'agent-neuron-templates',
      ...(body ? { 'content-type': 'application/json' } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await res.text();
  return { status: res.status, data: text ? JSON.parse(text) : null };
}
const fail = (what, r) => new Error(`${what}: HTTP ${r.status} ${r.data?.message ?? ''}`.trim());

/** What GitHub says about a repo, or null when there is none. Any other failure throws. */
async function view(repo) {
  const r = await api('GET', `/repos/${repo}`);
  if (r.status === 404) return null;
  if (r.status !== 200) throw fail(`reading ${repo}`, r);
  return { repo: r.data.full_name, private: r.data.private, isTemplate: r.data.is_template, from: r.data.template_repository?.full_name ?? null };
}

/** A private repo generated from a template repo. */
async function generate(template, repo) {
  const [owner, name] = repo.split('/');
  const r = await api('POST', `/repos/${template}/generate`, { owner, name, private: true });
  if (r.status !== 201) throw fail(`generating ${repo} from ${template}`, r);
}

/** Marks a repo as a template. */
async function markTemplate(repo) {
  const r = await api('PATCH', `/repos/${repo}`, { is_template: true });
  if (r.status !== 200) throw fail(`marking ${repo} as a template`, r);
}

const p = JSON.parse(readFileSync(0, 'utf8'));
const apply = process.env.TEMPLATES_APPLY === '1';
const stop = (what) => { console.error(`create: ${what}`); process.exit(1); };

try {
  const source = await view(p.template);
  if (!source) stop(`no such template repo: ${p.template}`);
  if (!source.isTemplate) stop(`${p.template} is not marked as a template`);

  const have = await view(p.repo);
  if (have && !have.private) stop(`${p.repo} exists and is not private`);
  if (have && have.from !== source.repo) stop(`${p.repo} exists and was not generated from ${source.repo}`);
  const action = !have ? 'create' : have.isTemplate ? 'none' : 'mark';

  if (apply) {
    if (action === 'create') await generate(p.template, p.repo);
    if (action !== 'none') await markTemplate(p.repo);
  }
  console.log(JSON.stringify({ ...p, action, applied: apply }));
} catch (e) {
  stop(e.message);
}
