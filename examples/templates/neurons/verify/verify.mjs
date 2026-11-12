// Looks at what is on GitHub after the effect and answers the record: the run's result for one type.
// In:  { repo, template, action, applied }
// Out: { repo, ok: true, action }  or, when nothing was applied, { repo, dry: true, action }
// A repo that is missing, public, not a template, still empty or not from the template fails the call (exit 1).
import { readFileSync } from 'node:fs';
import { setTimeout as sleep } from 'node:timers/promises';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

// This neuron's own settings: `.env` in its folder (git-ignored; `env.example` shows the names).
// A variable the engine passes through by name (the neuron's `env`) wins over the file.
const env = fileURLToPath(new URL('.env', import.meta.url));
if (existsSync(env)) process.loadEnvFile(env);
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

/** Whether the repo has no commit yet: GitHub fills a generated repo a moment after it makes it. */
async function empty(repo) {
  const r = await api('GET', `/repos/${repo}/commits?per_page=1`);
  if (r.status === 409) return true;
  if (r.status !== 200) throw fail(`reading the commits of ${repo}`, r);
  return r.data.length === 0;
}

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
