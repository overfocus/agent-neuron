# The feature loop

How a change gets made to agent-neuron, written so that an agent can run it
end to end and a person can follow the same steps by hand. Every feature is
defined by checks that can be run, and the work repeats until they pass.
Nothing is called done on the strength of "it should work".

```
Frame ──▶ Build ──▶ Verify ──▶ Review ──▶ Preview ──▶ Ship ──▶ Record
            ▲          │          │           │
            └──────────┴──────────┴───────────┘
                 any failure returns to Build
```

A real problem found along the way that is not this feature's is logged as a
GitHub issue (`npm run issue`, `/loop:issues`), not fixed in the branch and
not dropped; see "Findings outside the scope" below.

## Who does what

The session that runs the loop is the lead: it frames, routes the work and
decides. The roles come from the agent-kit `loop` plugin; a project agent in
`.claude/agents/<role>.md` replaces the plugin's one of the same role. A small
change can stay in the lead's own hands.

| Stage | Who |
|---|---|
| Frame | the lead, with `loop:scout` for "is this live?" and "where is it?" |
| Build | `loop:builder`, on its own `feat/` branch in a worktree; `loop:sourcer` for sourced content (`content-loop.md`) |
| Verify | `npm run verify` (writes the receipt), then `npm run check:browser`, then `loop:browser-check` for what needs eyes |
| Review | `loop:reviewer`, the whole diff with fresh eyes |
| Preview | the browser gate again on a local production build; run the demos listed in `docs/DEMOS.md` |
| Ship | Kevin's go-ahead, then `/loop:ship`; the `guard-main` hook asks before any push to `main` |
| Record | the lead, from the reviewer's record list and each agent's `Learned:`; `/loop:issues` for what was found and left |

### Agent notes for this repo

The plugin's agents are generic and read this section for what is true only
here. One line each:

- **Gates:** `npm run verify` is the kit's verify over the config's
  `verify.steps`: typecheck, Rust test and build, test (Vitest), build (`tsc` to `dist/`); it
  writes `.verify/receipt.json`.
- **Browser gate:** none yet. This is a library with no UI; the gate is the
  tests. When a UI lands (a graph visualiser), add `check:browser` and fill
  `browser.md`.
- **Dev server:** none. Use `npm run test:watch` while building.
- **Branches:** `feat/` branches may be pushed to the private GitHub repo as
  a backup (no previews); only `main` is the release line.
- **Production:** nothing deploys; `origin` is unset in
  `agent-kit.config.json`, so `npm run ship` stops at its config check: merge
  `--no-ff` and push by hand after Kevin's go-ahead. TODO(Kevin): whether this
  becomes an npm package or a deployed service.
- **Acceptance checks** here are tests: each check is a Vitest case that
  fails before the change and passes after it.
- **Analytics, facts, shared files:** none; no files are shared with portfolio.

## 1. Frame

Before writing code, state in the conversation (not in a file):

- **The change in one sentence**, and the route or component it changes.
- **Acceptance checks**, three to six, each observable ("`/services/` lists the
  new service with its photo", "`/old-path` returns a 308 to `/new-path`").
  "Looks good" is not a check.
- **For layout or motion (only once a UI exists): where it must be on screen.** Measure at a phone's
  real usable height (about 390x664, not the 390x844 of its screen, since a
  browser's bars take the rest) and a small laptop (1280x720). Say what the
  visitor must be able to see, without scrolling, after each interaction: "an
  animation plays" passes off-screen. Write these as browser checks before Build.
- **What is out of scope.**

**Starting from an issue.** If the work is a logged issue (`/loop:issues`
triage, or "do #12"), read it with `gh issue view <n>`, and for a bug
reproduce it on `main` first: if it no longer reproduces, stop and tell Kevin
rather than building. Its Expected section becomes the acceptance checks, its
Repro the first check to run. Name the issue number in the frame; the ship
carries it (`--closes <n>`), on a `fix/` branch for a bug (`npm run wt -- new
<slug> --prefix fix`) or `feat/` for the rest.

Then read the code it touches, and check that a component is live (has an
importer) before building on it. Ask Kevin only when a check depends on
something the code can't answer: copy, a price or term, a claim about the
business, where a link points.

## 2. Build

- Branch from an up-to-date `main` as `feat/<short-slug>`, in a worktree:
  `npm run wt -- new <short-slug>` (`parallel.md`).
- Build the smallest slice that satisfies one check, then the next. Write the
  check's test first. Match the surrounding code (`AGENTS.md`).
- Run `npm run verify -- --skip-build` as often as useful.

## 3. Verify

The machine gate must pass.

**The machine gate:** `npm run verify` on the committed tree. It writes
`.verify/receipt.json`; `npm run verify:receipt` exits 0 only for a PASSED full
run (with the build) whose tree equals the tree now. Verify hashes the tree
when it starts and again when it ends; a run during which files changed fails.
The builder reports the receipt; the reviewer and `/loop:ship` trust it and
re-run verify only on a mismatch.

**The browser gate:** not used yet (no UI). When one exists, follow
`browser.md`.

If a gate fails, go back to Build. Fix the cause; never relax the check, raise
a baseline or delete the failing case.

## 4. Review

Read the whole diff against `main` as if someone else wrote it:

- anything outside the stated scope, which comes out;
- leftover debugging, commented-out code, unused files;
- new environment variables (they go in `AGENTS.md` and the Vercel project
  before shipping) and new dependencies (each needs a reason);
- a test weakened, skipped or deleted to make verify pass;
- anything personal or private in a request, an event or a log.

Fixing a finding means returning to Verify.

**Split the findings before sending any back.** Blockers (a failed acceptance
check, wrong behaviour a visitor would hit, a broken rule of the repo's) go to the
builder, the fewest that make the feature right, in one round. The rest is
logged with `/loop:issues` and ships later: a long list sent as one job is slower
than two short rounds, because its fixes pile onto the same files before any is
checked.

## Findings outside the scope

During any stage a real problem turns up that the feature did not cause and
does not need: a broken link on a neighbouring route, a failing check on an
untouched page, a reviewer's item about old code, a trap that cost time. Three
answers, and only one is right for each:

- **In scope** (the feature broke it, or a check cannot pass without it): fix
  it in the branch. It is not filed.
- **Out of scope and real** (you saw it, and for a bug you reproduced it): log
  it with `npm run issue -- add ...` (`/loop:issues` has the rules: kind, the
  Observed, Expected and Repro sections of a bug, no email, phone, token, price
  or customer's name in the text, since the repo may be public). Do not fix it
  in this branch: that widens the scope the reviewer checks.
- **Opinion, speculation or not reproducible:** neither fix nor file; mention
  it in the report if it matters.

The same finding reported twice comments on the open issue instead of opening
another. An agent never closes an issue; it closes through a ship.

## 5. Preview

Run every demo listed in `docs/DEMOS.md` that the change touches, against the
built output (not the source), and read what it prints. The full
`npm run verify` (with the build) is still the last gate.

## 6. Ship

Pushing `main` deploys production, so this needs Kevin's go-ahead for this
change. Report the checks and their results, then wait. Before merging: no pre-merge step.

Once approved, `/loop:ship` runs `npm run ship`: the go-ahead quoted
(`--approved`, on every run), the receipt, a rebase if behind, `merge --no-ff`,
a pause for the push (run the printed command yourself so the hook asks), the
deploy status and the live check (neither applies here yet), the Runs note and
the worktree and branch removed. The sequence: push → `--from deploy` (pauses)
→ the live check → `--from indexnow --live-ok`. `--no-update "<reason>"` on the
first run for a branch with nothing to announce; `--dry-run` changes nothing.
A branch that resolves logged issues passes `--closes <n>` for each on the
first run: the merge commit then says `Closes #n` and GitHub closes the issue
when `main` is pushed (the Runs note and ship record list them). A bug the
live check finds after the deploy is logged (`--source live`) before the Runs
note, and reported to Kevin at once.

## 7. Record

`/loop:ship` writes the Runs note and the ship record (`npm run ships --
summary` reads the records). The rest is the lead's:

- Update `AGENTS.md` if routes, commands, environment variables or
  conventions changed.
- Add a gotcha (here or in `AGENTS.md`) only if it cost real time and would
  again.
- Port shared-file changes to the siblings if Frame named any.
- Sweep for what was found and left (`/loop:issues sweep`): the reviewer's
  Elsewhere items, browser-check failures that are not this feature's, the
  builder's Out of scope list, and any `Learned:` line that is a defect. The
  report lists the issues filed (number, kind, title) and what was skipped and
  why.

## When to stop looping

Stop and report to Kevin, with what was tried, when:

- the same gate has failed three times for the same reason;
- passing a check would mean changing the check, a baseline or the scope;
- the work needs something only he has: an owner's answer, credentials, a
  dashboard or DNS setting, a content decision, or the go-ahead to ship.
