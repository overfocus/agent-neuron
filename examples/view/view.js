// The network view's page. It draws only what the traffic shows: a node appears when a neuron
// first sends or receives a signal, an edge when a signal first crosses it. No libraries.
const $ = (id) => document.getElementById(id);
const NS = 'http://www.w3.org/2000/svg';
const svg = $('graph');
const logEl = $('log');
const reduced = matchMedia('(prefers-reduced-motion: reduce)');
const MAX_LOG = 400;
const WHY = { replaced: ['stale', 'replaced by a newer one (stale)'], expired: ['gave up', 'gave up waiting'], hops: ['hops', 'too many hops'] };

const el = (name, attrs = {}, parent) => {
  const e = document.createElementNS(NS, name);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  parent?.append(e);
  return e;
};

let source, nodes, edges, held, holds, lines;

function reset() {
  source?.close();
  svg.replaceChildren();
  logEl.replaceChildren();
  el('g', { id: 'edges' }, svg);
  el('g', { id: 'nodes' }, svg);
  el('g', { id: 'markers' }, svg);
  nodes = new Map(); edges = new Map(); held = new Map(); holds = {}; lines = 0;
}

function node(id, col) {
  let n = nodes.get(id);
  if (n) return n;
  const g = el('g', { class: 'node' }, svg.querySelector('#nodes'));
  const rect = el('rect', { rx: 8, height: 44 }, g);
  el('text', { class: 'name', y: -4 }, g).textContent = id;
  const status = el('text', { class: 'status', y: 12 }, g);
  const badge = el('g', { class: 'badge', display: 'none' }, g);
  el('rect', { rx: 8, width: 68, height: 16, y: -8 }, badge);
  const badgeText = el('text', { x: 34, y: 3 }, badge);
  n = { id, col, g, rect, status, badge, badgeText, x: 0, y: 0 };
  nodes.set(id, n);
  layout();
  return n;
}

function edge(a, b) {
  const key = [a, b].sort().join('\0');
  if (!edges.has(key)) {
    edges.set(key, { a, b, line: el('line', { class: 'edge' }, svg.querySelector('#edges')) });
    layout();
  }
  return edges.get(key);
}

// Columns follow who reached whom first; rows are spread evenly down each column.
function layout() {
  const W = svg.clientWidth, H = svg.clientHeight;
  const cols = Math.max(1, ...[...nodes.values()].map((n) => n.col + 1));
  const w = Math.max(76, Math.min(120, W / cols - 12));
  const rows = new Map();
  for (const n of nodes.values()) rows.set(n.col, [...(rows.get(n.col) ?? []), n]);
  for (const [col, list] of rows) {
    list.forEach((n, i) => {
      n.x = (W / cols) * (col + 0.5);
      n.y = (H / list.length) * (i + 0.5);
      n.g.setAttribute('transform', `translate(${n.x} ${n.y})`);
      n.rect.setAttribute('width', w);
      n.rect.setAttribute('x', -w / 2);
      n.rect.setAttribute('y', -22);
      n.badge.setAttribute('transform', `translate(${w / 2 - 34} -22)`);
    });
  }
  for (const e of edges.values()) {
    const a = nodes.get(e.a), b = nodes.get(e.b);
    e.line.setAttribute('x1', a.x); e.line.setAttribute('y1', a.y);
    e.line.setAttribute('x2', b.x); e.line.setAttribute('y2', b.y);
  }
}
new ResizeObserver(() => nodes && layout()).observe(svg);

function setStatus(id, kind, text) {
  const n = node(id, 0);
  n.status.dataset.kind = kind;
  n.status.textContent = text;
}

function showWaiting(id) {
  const n = nodes.get(id);
  const count = held.get(id)?.size ?? 0;
  n.badge.setAttribute('display', count ? 'inline' : 'none');
  n.badgeText.textContent = `waiting ${count}`;
}

// A marker for a request (circle) or an answer (diamond) travelling along an edge.
function travel(from, to, answer) {
  if (reduced.matches) return;
  const a = nodes.get(from), b = nodes.get(to);
  const m = el('path', { class: answer ? 'marker answer' : 'marker', d: answer ? 'M0 -6 L6 0 L0 6 L-6 0Z' : 'M-5 0a5 5 0 1 0 10 0a5 5 0 1 0 -10 0' }, svg.querySelector('#markers'));
  const at = (n) => `translate(${n.x}px, ${n.y}px)`;
  m.animate([{ transform: at(a) }, { transform: at(b) }], { duration: 600, easing: 'ease-in-out' }).finished.then(() => m.remove(), () => m.remove());
}

function log(kind, tag, text, ms) {
  const li = document.createElement('li');
  li.dataset.kind = kind;
  const time = document.createElement('time');
  time.textContent = `${(ms / 1000).toFixed(1)}s`;
  const t = document.createElement('span');
  t.className = 'tag';
  t.textContent = tag;
  li.append(time, t, ` ${text}`);
  const stick = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 40;
  logEl.append(li);
  if (++lines > MAX_LOG) (logEl.firstChild.remove(), lines--);
  if (stick) logEl.scrollTop = logEl.scrollHeight;
}

// A threshold neuron holds inputs until enough distinct senders have sent one on a trace.
function hold(s) {
  for (const id of s.to) {
    if (!holds[id]) continue;
    const mine = held.get(id) ?? new Map();
    held.set(id, mine);
    mine.set(s.id, { trace: s.trace, from: s.from });
    const senders = new Set([...mine.values()].filter((h) => h.trace === s.trace).map((h) => h.from));
    if (senders.size >= holds[id]) {
      for (const [k, h] of mine) if (h.trace === s.trace) mine.delete(k);
      setStatus(id, 'acting', '▸ acting');
    }
    showWaiting(id);
  }
}

function onSignal(s, ms) {
  const known = s.to.map((id) => nodes.get(id)).find(Boolean);
  const from = node(s.from, known ? Math.max(0, known.col - 1) : 0);
  for (const id of s.to) {
    node(id, from.col + 1);
    edge(s.from, id);
  }
  const where = s.to.join(', ');
  if (s.replyTo) {
    const kind = s.quiet ? 'quiet' : 'answer';
    setStatus(s.from, kind, s.quiet ? '◇ quiet' : '◆ answer');
    if (!s.quiet) for (const id of s.to) travel(s.from, id, true);
    log(kind, s.quiet ? '◇ quiet' : '◆ answer', `${s.from} → ${where}${s.quiet ? '' : `: ${s.text}`}`, ms);
  } else {
    for (const id of s.to) travel(s.from, id, false);
    log('signal', '● signal', `${s.from} → ${where}: ${s.text}`, ms);
    hold(s);
  }
}

function onDrop(e, ms) {
  const [short, long] = WHY[e.reason] ?? [e.reason, e.reason];
  setStatus(e.neuron, 'drop', `✕ ${short}`);
  held.get(e.neuron)?.delete(e.signal.id);
  if (holds[e.neuron]) showWaiting(e.neuron);
  log('drop', '✕ dropped', `${e.neuron}: ${long} (${e.signal.from}: ${e.signal.text})`, ms);
}

function onError(e, ms) {
  setStatus(e.neuron, 'error', '▲ error');
  log('error', '▲ error', `${e.neuron}: ${e.message}`, ms);
}

function run() {
  reset();
  const demo = $('demo').value;
  $('run').textContent = 'Run again';
  $('state').textContent = 'Running';
  source = new EventSource(`/events?demo=${encodeURIComponent(demo)}`);
  let done = false;
  source.onmessage = (m) => {
    const e = JSON.parse(m.data);
    if (e.type === 'start') holds = e.holds;
    else if (e.type === 'signal') onSignal(e, e.ms);
    else if (e.type === 'drop') onDrop(e, e.ms);
    else if (e.type === 'error') onError(e, e.ms);
    else if (e.type === 'done') {
      done = true;
      source.close();
      $('state').textContent = `Finished: ${nodes.size} neurons, ${edges.size} links`;
    }
  };
  // EventSource would reconnect, and a reconnect would run the demo again.
  source.onerror = () => {
    if (!done) $('state').textContent = 'Connection lost';
    source.close();
  };
}

$('controls').addEventListener('submit', (e) => (e.preventDefault(), run()));
reset();
