// Runs a script of the agent-kit loop plugin in this repo:
//
//   node scripts/kit.mjs <ship|check-browser|verify-receipt> [args...]
//
// package.json: "ship": "node scripts/kit.mjs ship", "check:browser": "node scripts/kit.mjs check-browser",
// "verify:receipt": "node scripts/kit.mjs verify-receipt". The repo's own settings are in agent-kit.config.json.
//
// The plugin is found, first match wins:
//   1. $AGENT_KIT: a kit checkout (or its plugins/loop), e.g. a kit branch under test
//   2. an agent-kit checkout beside the main checkout or beside any folder above it (../agent-kit, then
//      ../../agent-kit, ...: a repo nested one level down, like krux42/muz, finds it too), the local marketplace
//   3. the installed plugin (~/.claude/plugins/installed_plugins.json, loop@agent-kit: this project's, else the user's)
// Prints which one on stderr. Copied from the loop plugin's templates/scripts/kit.mjs: keep it equal to that file.

import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import path from 'node:path';

const [name, ...args] = process.argv.slice(2);
const fail = (msg) => {
  console.error(`kit: ${msg}`);
  process.exit(2);
};
if (!name || !/^[\w-]+$/.test(name)) fail('usage: node scripts/kit.mjs <ship|check-browser|verify-receipt> [args...]');

const common = spawnSync('git', ['rev-parse', '--path-format=absolute', '--git-common-dir'], { encoding: 'utf8' });
const main = common.status === 0 ? path.dirname(common.stdout.trim()) : process.cwd();
const loopOf = (dir) => [dir, path.join(dir, 'plugins', 'loop')].find((d) => existsSync(path.join(d, 'scripts', 'lib', 'config.mjs')));

function installed() {
  try {
    const all = JSON.parse(readFileSync(path.join(homedir(), '.claude', 'plugins', 'installed_plugins.json'), 'utf8')).plugins?.['loop@agent-kit'] ?? [];
    const pick = all.find((p) => p.projectPath === main) ?? all.find((p) => p.scope === 'user');
    return pick?.installPath;
  } catch {
    return undefined;
  }
}

const candidates = [
  ['$AGENT_KIT', process.env.AGENT_KIT],
  ...(() => {
    const out = [];
    for (let dir = path.dirname(main); dir !== path.dirname(dir); dir = path.dirname(dir)) out.push(['checkout', path.join(dir, 'agent-kit')]);
    return out;
  })(),
  ['installed', installed()],
];
let found;
for (const [how, dir] of candidates) {
  const loop = dir && loopOf(dir);
  if (loop) {
    found = { how, loop };
    break;
  }
  if (how === '$AGENT_KIT' && dir) fail(`$AGENT_KIT=${dir} has no loop plugin scripts`);
}
if (!found) fail('the agent-kit loop plugin was not found: no ../agent-kit checkout and no installed loop@agent-kit (claude plugin marketplace add overfocus/agent-kit)');

const script = path.join(found.loop, 'scripts', `${name}.mjs`);
if (!existsSync(script)) fail(`the loop plugin at ${found.loop} has no scripts/${name}.mjs`);
console.error(`kit: ${name} from ${found.loop} (${found.how})`);
const r = spawnSync(process.execPath, [script, ...args], { stdio: 'inherit' });
process.exit(r.status ?? 1);
