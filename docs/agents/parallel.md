# Working in parallel

Several agents may work on this repo at once, each in its own git worktree
under `.claude/worktrees/` on its own `feat/` branch.

## Dev servers

None: this is a library. Run `npm run test:watch` in your own worktree. If a
dev server is added later, never assume a port is yours, and never restart or
kill a server you didn't start; stop yours by its PID.

## Worktrees

- Make one with `npm run wt -- new <slug>` (`--prefix fix` for a fix): it
  branches `feat/<slug>` from `main` at `.claude/worktrees/<slug>`, clones the
  main checkout's `node_modules` (APFS, no extra disk) and copies `.env.local` when git ignores
  it. `npm ci` there after a dependency change.
- A worktree has the docs of the commit it was made at. A gotcha another agent
  needs now goes to `main` as a small doc fix, not into a feature branch.
- `/loop:ship` removes the worktree and branch after the merge;
  `npm run wt -- gc` lists the others that are merged and pushed, clean and
  unlocked (the session brief counts them), and `--apply` removes them. The
  `guard-worktrees` hook blocks deleting a symlink in `.claude/worktrees/`,
  `.claude/worktrees` or `.claude` itself, a locked worktree, `--force`, and
  `git clean -d/-x/-X` where worktrees live.

## Shipping

- `feat/` branches stay local (no remote yet); never push `main` without Kevin's go-ahead for that change. The
  `guard-main` hook asks before any push to `main`.
- `/loop:ship` merges `--no-ff` after the go-ahead, pauses at the push (run the
  printed command so the hook asks) and again after the deploy for the live
  check; `--from` never skips its go-ahead, receipt or main-checkout checks.
