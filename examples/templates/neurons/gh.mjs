// The two things these neurons ask of the gh command line: run it, and look a repo up.
import { execFileSync } from 'node:child_process';

export const gh = (...args) => execFileSync('gh', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });

/** What GitHub says about a repo, or null when there is none. Any other failure throws. */
export function view(repo) {
  try {
    const r = JSON.parse(gh('repo', 'view', repo, '--json', 'nameWithOwner,isTemplate,isEmpty,visibility,templateRepository'));
    const t = r.templateRepository;
    return { repo: r.nameWithOwner, private: r.visibility === 'PRIVATE', isTemplate: r.isTemplate, empty: r.isEmpty, from: t ? `${t.owner.login}/${t.name}` : null };
  } catch (e) {
    if (/Could not resolve to a Repository/.test(String(e.stderr))) return null;
    throw new Error(String(e.stderr || e.message).trim());
  }
}
