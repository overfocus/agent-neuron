# Checking pages in the browser

Not used yet: agent-neuron is a library with no UI, so the feature loop's
browser gate is skipped and the tests are the gate (`feature-loop.md`, Agent
notes).

When a UI lands (a graph visualiser, a demo page), re-run `/loop:init`: it
copies the kit's `browser.md` here, adds `check:browser` (and `check:layout`
if needed) to `package.json` and `playwright-core` as a dev dependency.
