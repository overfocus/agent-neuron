// The demos the network view and the trace recorder can run, by name. `neurons` declares each neuron up front:
// what it wraps (`kind`), a short description, and its options. Both consumers learn thresholds from here, and the
// recorder checks the declaration against the neurons the run really built, so it cannot drift.
// Descriptions name scripts by file name only. `run(listeners, lib)` returns what the demo returns.
import { runAlerts } from '../alerts-demo.mjs';

const quiet = () => {};
const options = (threshold, within) => ({ threshold, within });

const typing = (suggest) => async (listeners) => {
  const { jsTracks, runTyping } = await import('../typing-demo.mjs'); // imports dist/, so only when asked
  return runTyping({ tracksAdapter: jsTracks(), suggest, log: quiet, listeners });
};
const typingNeurons = [
  { id: 'box', kind: 'source', description: 'the search box: emits the query so far, one letter at a time' },
  { id: 'tracks', kind: 'js', description: 'looks up track titles; fast, acts from 3 characters' },
  { id: 'artist', kind: 'js', description: 'acts only on an exact artist name' },
  { id: 'lyrics', kind: 'js', description: 'slow lookup in lyrics, acts from 5 characters' },
];
const typingLinks = [{ from: 'box', to: 'tracks' }, { from: 'box', to: 'artist' }, { from: 'box', to: 'lyrics' }];
const agree = { id: 'suggest', kind: 'js', description: 'acts only when two neurons answer the same keystroke', options: options(2, 400) };

/** The alert storm with `hypothesis` acting once `threshold` distinct detectors flag one alert (the demo uses 2). */
export const alertsScenario = (threshold = 2) => ({
  label: 'Alert storm',
  description: 'Forty invented alerts in six seconds; cheap detectors flag them, a model stand-in runs once '
    + `${threshold} detector${threshold === 1 ? '' : 's'} flag${threshold === 1 ? 's' : ''} the same alert.`,
  parameters: { threshold },
  links: [
    ...['logs', 'deploys', 'metrics'].flatMap((d) => [{ from: 'alerts', to: d }, { from: d, to: 'hypothesis' }]),
    { from: 'hypothesis', to: 'diagnosis' },
  ],
  neurons: [
    { id: 'alerts', kind: 'source', description: 'emits each invented alert, newer ones replacing older ones for the same service' },
    { id: 'logs', kind: 'shell script', description: 'detect-logs.sh, a POSIX sh script, scans the invented log lines' },
    { id: 'deploys', kind: 'shell script', description: 'detect-deploys.sh, a POSIX sh script, checks the invented deploy list' },
    { id: 'metrics', kind: 'js', description: 'checks invented error rates for critical alerts' },
    { id: 'hypothesis', kind: 'model stand-in', description: 'a fixed 500 ms wait standing in for a model; acts once enough detectors agree', options: options(threshold, 500) },
    { id: 'diagnosis', kind: 'composed', description: 'owns a private inner network and answers with one verdict' },
  ],
  run: (listeners, lib) => runAlerts(lib, { listeners, threshold }),
  // The demo's own summary numbers.
  summary: ({ stats: s, final }) => ({
    threshold, alerts: s.alerts, flags: s.flags, calls: s.calls, finished: s.calls - s.cut, cut: s.cut,
    heldReplaced: s.heldReplaced, expired: s.expired, diagnoses: s.diagnoses, final: final ?? null, // null: no diagnosis, so every variant has the same fields
  }),
});

export const scenarios = {
  alerts: alertsScenario(),
  typing: {
    label: 'Typing',
    description: 'A search box types "lumen harbor" one letter at a time; three neurons decide for themselves whether to act.',
    neurons: typingNeurons,
    links: typingLinks,
    run: typing(false),
    summary: ({ keystrokes }) => ({ keystrokes }),
  },
  suggest: {
    label: 'Suggest (threshold)',
    description: 'The typing demo plus a suggest neuron that acts only when two neurons answer the same keystroke.',
    neurons: [...typingNeurons, agree],
    links: [...typingLinks, ...['tracks', 'artist', 'lyrics'].map((from) => ({ from, to: 'suggest' }))],
    run: typing(true),
    summary: ({ keystrokes, suggestRuns }) => ({ keystrokes, suggestRuns }),
  },
};
