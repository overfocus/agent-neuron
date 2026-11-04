// The network view: a local page that draws a demo's network as it runs.
//   npm run view     (builds first, then listens on 127.0.0.1 on a free port and prints the URL)
// Serves four static files and one Server-Sent Events endpoint, /events?demo=<name>, which
// runs that demo in this process and streams what its Network reports (onSignal, onDrop,
// onError) as trace events. Nothing is fetched from outside and the page makes no outside requests.
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { scenarios } from '../trace/scenarios.mjs';
import { tap } from '../trace/tap.mjs';

export { scenarios };

const FILES = {
  '/': ['index.html', 'text/html'],
  '/view.css': ['view.css', 'text/css'],
  '/view.js': ['view.js', 'text/javascript'],
  '/replay.js': ['../trace/replay.mjs', 'text/javascript'], // the page folds events with the same module any trace reader can use
};

// A payload as one short line: text as is, a list joined with " | ", an object as "key: value" pairs.
const plain = (v) => (typeof v === 'object' && v !== null ? JSON.stringify(v) : String(v));
const brief = (p) => {
  const d = p.data;
  let s;
  if (p.type === 'text') s = p.text;
  else if (Array.isArray(d)) s = d.map(plain).join(' | ');
  else if (typeof d === 'object' && d !== null) s = Object.entries(d).map(([k, v]) => `${k}: ${plain(v)}`).join(', ');
  else s = plain(d);
  return s.length > 80 ? `${s.slice(0, 77)}...` : s;
};
// The events are the trace's (docs/TRACE.md), plus `text`: the payload as one line for the log.
const wire = (e) => ({ ...e, ...(e.payload && { text: brief(e.payload) }) });

/** Start the server on `port` (0: any free one). `lib` is the library the alerts scenario runs on. */
export function startViewServer(lib, port = 0) {
  const server = createServer(async (req, res) => {
    const url = new URL(req.url ?? '/', 'http://x');
    const headers = { 'Content-Security-Policy': "default-src 'self'; img-src data:", 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff' };
    const refuse = () => void res.writeHead(403, headers).end('forbidden');
    // The Host check stops DNS rebinding (another site's name pointed at 127.0.0.1); only GET is served.
    const host = req.headers.host ?? '';
    if (req.method !== 'GET' || !/^(127\.0\.0\.1|localhost):\d+$/.test(host)) return refuse();
    if (url.pathname === '/events') {
      // Starting a run costs CPU, so a page on another site (an <img src=".../events?demo=alerts">) must not be
      // able to start one: only this page itself, or a request with no browser context (curl), gets through.
      const site = req.headers['sec-fetch-site'];
      const origin = req.headers.origin;
      if ((site && site !== 'same-origin' && site !== 'none') || (origin && origin !== `http://${host}`)) return refuse();
      return stream(url.searchParams.get('demo') ?? '', req, res, headers, lib);
    }
    const file = FILES[url.pathname];
    if (!file) return void res.writeHead(404, headers).end('not found');
    try {
      const body = await readFile(fileURLToPath(new URL(file[0], import.meta.url)));
      res.writeHead(200, { ...headers, 'Content-Type': `${file[1]}; charset=utf-8` }).end(body);
    } catch {
      res.writeHead(500, headers).end('could not read the file');
    }
  });
  return new Promise((resolve) => server.listen(port, '127.0.0.1', () => resolve(server)));
}

// Each request is its own run in its own Network; runs that overlap share the CPU, so their timing changes.
function stream(name, req, res, headers, lib) {
  const scenario = Object.hasOwn(scenarios, name) ? scenarios[name] : undefined;
  if (!scenario) return void res.writeHead(404, headers).end('unknown demo');
  res.writeHead(200, { ...headers, 'Content-Type': 'text/event-stream', Connection: 'keep-alive' });
  const send = (event) => {
    if (res.writableEnded || res.destroyed) return;
    res.write(`data: ${JSON.stringify(event)}\n\n`);
  };
  send({ type: 'start', demo: name, neurons: scenario.neurons, links: scenario.links });
  // A listener that throws would skip the demo's own handler for that event, so these only serialise and write.
  const { listeners, fail } = tap((e) => send(wire(e)), scenario.neurons);
  scenario
    .run(listeners, lib)
    .catch(fail)
    .finally(() => {
      send({ type: 'done' });
      res.end();
    });
  // shortcut: a closed page does not stop its run (the demos last seconds); the run just writes nowhere.
  req.on('close', () => res.end());
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const lib = await import('../../dist/index.js');
  const server = await startViewServer(lib);
  console.log(`Network view: http://127.0.0.1:${server.address().port}/  (pid ${process.pid}; Ctrl+C or kill ${process.pid} to stop)`);
}
