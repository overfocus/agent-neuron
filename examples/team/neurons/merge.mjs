// The merge, an effect (SPEC 11): a started call is never cut by staleness,
// and a failure nobody can read as "did not happen" is an unknown, not an
// error. The engine feeds the gate answer to stdin as one JSON value - the
// lead's edit text, "merge <branch>". The branch must be exactly one of the
// argv whitelist entries; anything else is refused with a clear nonzero exit
// before any git call runs. No shell, no eval: git runs through execFileSync
// with an explicit argument array, and the neuron's stdout stays one JSON
// value (the answer), every git byte captured instead of forwarded.
import { execFileSync } from 'node:child_process';

let raw = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (c) => (raw += c));
process.stdin.on('end', () => {
  const repo = process.argv[2];
  const allow = process.argv.slice(3);
  const refuse = (why, code) => {
    process.stderr.write(`merge refused: ${why}\n`);
    process.exit(code);
  };
  if (!repo || allow.length === 0) refuse('usage: merge.mjs <repo> <allowed branch...>', 3);
  let text;
  try {
    text = JSON.parse(raw);
  } catch {
    refuse('the gate answer is not one JSON value', 3);
  }
  if (typeof text !== 'string') refuse('the gate answer is not a text payload', 3);
  const parts = text.trim().split(/\s+/);
  if (parts.length !== 2 || parts[0] !== 'merge') {
    refuse(`the gate answer is not 'merge <branch>': ${text.trim()}`, 2);
  }
  const branch = parts[1];
  if (!allow.includes(branch)) {
    refuse(`'${branch}' is not on the argv whitelist (${allow.join(', ')})`, 2);
  }
  const git = (args) => execFileSync('git', args, { cwd: repo, encoding: 'utf8' }).trim();
  try {
    git(['rev-parse', '--verify', `${branch}^{commit}`]);
  } catch {
    refuse(`the branch ${branch} is not in the scratch repo ${repo}`, 4);
  }
  try {
    git(['merge', '--no-ff', '--no-edit', branch]);
  } catch (e) {
    // The call started; the engine records unknown, and the last answer's
    // settle ends the run. git's own bytes ride on stderr as logs.
    process.stderr.write(`merge failed: ${e?.stderr || e?.message || e}\n`);
    process.exit(1);
  }
  const sha = git(['rev-parse', '--short', 'HEAD']);
  try {
    git(['tag', '-a', 'team-decision', '-m', `team decision: merged ${branch}`]);
  } catch (e) {
    process.stderr.write(`the tag failed: ${e?.stderr || e?.message || e}\n`);
    process.exit(1);
  }
  process.stdout.write(JSON.stringify(`merged ${branch} at ${sha} (tag team-decision)`));
});
