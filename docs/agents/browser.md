# Checking pages in the browser

The browser gate (`feature-loop.md` step 3) has two parts: `npm run
check:browser` for everything a machine can assert, then Claude in Chrome for
what needs eyes. Headless Chrome counts as "a browser" for the rule that pages
are verified in one.

## `npm run check:browser`

```sh
npm run view   # in one terminal: builds, prints http://127.0.0.1:<port>/ (and the server's pid)
npm run check:browser -- --base http://127.0.0.1:<port> --routes / --checks examples/view/checks.json
```

The only route is `/` (the network view, `examples/view/`; the server also serves `examples/trace/replay.mjs` as `/replay.js`, which the page imports). `agent-kit.config.json` has no `browser` key, so the kit's default widths (320, 390, 768, 1280) apply; `--widths` and `--ignore "text"` are the other options.

The agent-kit loop plugin's `check-browser.mjs`, run through `scripts/kit.mjs`,
drives the installed Google Chrome headless (`playwright-core`, a dev
dependency here; nothing is downloaded). Every route at every width fails on a
status other than 200, a console or uncaught page error, or horizontal scroll.
A checks file adds DOM assertions per route (`exists`, `absent`, `count`,
`text`, and `widths` to limit them); the format is in the script's header.
Widths and console lines to ignore default from `agent-kit.config.json`
(`browser`).

- Point `--base` at the view server (`npm run view`). Nothing deploys, so there is no production run; `/loop:ship` has no live check here.
- Assert on the DOM, never on page source or a response body.
- The view has no dev mode: the server serves `examples/view/` as it is on disk, so edit and reload. A
  `--ignore` needs a reason written here; there are none.

## `npm run check:layout`

```sh
npm run check:layout -- --base http://127.0.0.1:<port> --file examples/view/layout.json \
  [--only "<state>"] [--viewport 390x664] [--watch]
```

Use it for layout and motion: where things land at each screen size and after each interaction (click, tap,
scroll, back, reload). The file lists viewports and states and asserts `inView` (at least N px on screen without
scrolling), `top`, `bottom`, `belowFold`, `noHScroll`, `minSize` and `count`; the format is in the script's header
and the plugin's `scripts/README.md`. It reuses one Chrome for every measurement and builds nothing, so it is
fast enough to iterate on (`--watch` runs again on Enter or a change to the file).

- "Visible" is `inView`, never an element's existence or size: an element can exist, be sized, and sit off screen.
- `npm run check:layout -- --base http://127.0.0.1:<port> --file examples/view/layout.json` checks the phone (390x664: controls and graph on screen, log below) and desktop (1280x720: log beside the graph) layout, and that an alerts run adds nodes. `--file examples/view/layout-suggest.json` runs suggest at 320, 390 and 1280 and checks the waiting badge is shown. Neither measures a clipped element for you (a clipped part of an SVG still reports a position), so the page sets `data-clipped` on the graph when a node, with its badge, is outside the picture or overlaps another, and the files assert it is absent.

## Claude in Chrome

- **A hidden tab doesn't animate:** `requestAnimationFrame` never fires while
  `document.visibilityState` is `hidden`, so canvases freeze on their last
  frame. Check it first.
- **The window may not resize.** For phone width, load the page in a
  same-origin `<iframe>` 390px wide and read its DOM through
  `contentDocument`.
- **Measure, don't eyeball:** screenshots are scaled. Read positions and
  sizes from the DOM.
- **Never submit a real form** (an enquiry, a quote, a checkout).
- **Don't trigger dialogs** (`alert`, `confirm`): they block the extension.
- Close the tabs you open.
- **The server runs demos in its own process.** Each `/events?demo=...` request is its own run (alerts takes about 7 s, so wait for "Finished" in `#state`, not for a fixed time). Overlapping runs share the CPU and change each other's timing, so check with one tab and one run. `/?demo=suggest` preselects a demo.
- **A free port, never a fixed one.** `npm run view` binds port 0 and prints the URL and its pid. Stop it with `kill <that pid>`: never kill a server you did not start, and note that `npm run` is a wrapper, so `kill` the printed pid, not npm's.
- **Closing the page does not stop a run;** the run finishes and writes nowhere. A re-run reconnects fresh.
- **No outside requests:** the server sends `Content-Security-Policy: default-src 'self'; img-src data:`, so a stray CDN or font link shows up as a console error in `check:browser`.
- **The view infers the graph from traffic.** A neuron that never sends or receives never appears (a composed neuron's inner neurons never reach the outer network), so absence of a node is not a bug.
