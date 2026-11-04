import { defineConfig } from 'vitest/config';

// Only this checkout's tests. Worktrees live under .claude/worktrees/ inside the repo,
// and their tests must never run as part of this checkout's verify.
export default defineConfig({
  test: {
    include: ['test/**/*.test.ts'],
    exclude: ['**/node_modules/**', '**/dist/**', '.claude/**'],
  },
});
