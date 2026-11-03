@AGENTS.md

## Claude Code

The agent-kit `loop` plugin is enabled in `.claude/settings.json`:

- `/loop:feature <what to build>` runs the feature loop from
  `docs/agents/feature-loop.md`.
- `/loop:ship` ships a finished branch after Kevin's go-ahead, through
  `npm run ship`.
- `/loop:issues [finding | sweep | triage]` logs what a run found and left as
  GitHub issues through `npm run issue`, or lists the open ones.
- `/loop:init` re-checks this repo's loop set-up without overwriting anything.
