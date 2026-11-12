// The neurons' settings, and the three things they ask of GitHub's API. No command-line tool is needed.
//
// Settings come from `.env` beside this file (git-ignored; `env.example` shows the names). A variable the
// engine passes through by name (the neuron's `env`) wins over the file, so a run can override one.
//   GITHUB_TOKEN      a token that may create private repos for the owner
//   TEMPLATES_OWNER   the account or organisation the repos belong to
//   TEMPLATES_SOURCE  the template repo they are generated from (a name, under the same owner)
//   TEMPLATES_APPLY   1 to really create; anything else and `create` only says what it would do
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const file = fileURLToPath(new URL('.env', import.meta.url));
if (existsSync(file)) process.loadEnvFile(file);

export function setting(name) {
  const v = process.env[name];
  if (!v) { console.error(`${name} is not set: put it in neurons/.env`); process.exit(1); }
  return v;
}

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
export async function view(repo) {
  const r = await api('GET', `/repos/${repo}`);
  if (r.status === 404) return null;
  if (r.status !== 200) throw fail(`reading ${repo}`, r);
  return { repo: r.data.full_name, private: r.data.private, isTemplate: r.data.is_template, from: r.data.template_repository?.full_name ?? null };
}

/** Whether the repo has no commit yet: GitHub fills a generated repo a moment after it makes it. */
export async function empty(repo) {
  const r = await api('GET', `/repos/${repo}/commits?per_page=1`);
  if (r.status === 409) return true;
  if (r.status !== 200) throw fail(`reading the commits of ${repo}`, r);
  return r.data.length === 0;
}

/** A private repo generated from a template repo. */
export async function generate(template, repo) {
  const [owner, name] = repo.split('/');
  const r = await api('POST', `/repos/${template}/generate`, { owner, name, private: true });
  if (r.status !== 201) throw fail(`generating ${repo} from ${template}`, r);
}

/** Marks a repo as a template. */
export async function markTemplate(repo) {
  const r = await api('PATCH', `/repos/${repo}`, { is_template: true });
  if (r.status !== 200) throw fail(`marking ${repo} as a template`, r);
}
