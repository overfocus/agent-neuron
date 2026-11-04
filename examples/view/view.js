// The network view's page. It draws only what the traffic shows: a node appears when a neuron
// first sends or receives a signal, an edge when a signal first crosses it. No libraries.
// Each node counts what it did (answers, quiet, dropped, sent) and each edge how many signals crossed it,
// so the end state shows which neurons did the work and how much was dropped.
const $ = (id) => document.getElementById(id);
const NS = 'http://www.w3.org/2000/svg';
const svg = $('graph');
const logEl = $('log');
const reduced = matchMedia('(prefers-reduced-motion: reduce)');
const MAX_LOG = 400;
const NODE_H = 68;
const WHY = { replaced: ['stale', 'replaced by a newer one (stale)'], expired: ['gave up', 'gave up waiting'], hops: ['hops', 'too many hops'] };

const el = (name, attrs = {}, parent) => {
  const e = document.createElementNS(NS, name);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  parent?.append(e);
  return e;
};
const layer = (name) => svg.querySelector(`#${name}`);

let source, nodes, edges, held, holds, lines, quietLine;

function reset() {
  source?.close();
  svg.replaceChildren();
  logEl.replaceChildren();
  // Drawn in this order: markers pass under the node they reach, never over its words.
  for (const name of ['edges', 'markers', 'nodes']) el('g', { id: name }, svg);
  nodes = new Map(); edges = new Map(); held = new Map(); holds = {}; lines = 0; quietLine = null;
}

function node(id, col) {
  let n = nodes.get(id);
  if (n) return n;
  const g = el('g', { class: 'node' }, layer('nodes'));
  const title = el('title', {}, g);
  const rect = el('rect', { rx: 8, height: NODE_H, y: -NODE_H / 2 }, g);
  const text = (cls, y) => el('text', { class: cls, y }, g);
  const [name, status, work, lost] = [text('name', -18), text('status', -4), text('count', 11), text('count', 24)];
  name.textContent = id;
  const badge = el('g', { class: 'badge', display: holds[id] ? 'inline' : 'none' }, g);
  el('rect', { rx: 8, width: 56, height: 16 }, badge);
  const badgeText = el('text', { x: 28, y: 12 }, badge);
  n = { id, col, g, title, rect, status, work, lost, badge, badgeText, x: 0, y: 0, sent: 0, answers: 0, quiet: 0, dropped: 0 };
  nodes.set(id, n);
  showCounts(n);
  showWaiting(n);
  layout();
  return n;
}

function edge(a, b) {
  const key = [a, b].sort().join('\0');
  let e = edges.get(key);
  if (!e) {
    e = { a, b, count: 0, line: el('line', { class: 'edge' }, layer('edges')) };
    edges.set(key, e);
    layout();
  }
  e.count++;
  e.line.setAttribute('stroke-width', Math.min(8, 1 + Math.log2(e.count)).toFixed(2));
  return e;
}

// Columns follow who reached whom first; rows are spread evenly down each column. A node is never wider than its
// column minus a gap, so neighbours never overlap and the outer columns stay inside the picture.
function layout() {
  const W = svg.clientWidth, H = svg.clientHeight;
  const cols = Math.max(1, ...[...nodes.values()].map((n) => n.col + 1));
  const w = Math.min(120, W / cols - 8);
  const rows = new Map();
  for (const n of nodes.values()) rows.set(n.col, [...(rows.get(n.col) ?? []), n]);
  for (const [col, list] of rows) {
    list.forEach((n, i) => {
      n.x = (W / cols) * (col + 0.5);
      n.y = (H / list.length) * (i + 0.5) - 8;
      n.g.setAttribute('transform', `translate(${n.x} ${n.y})`);
      n.rect.setAttribute('width', w);
      n.rect.setAttribute('x', -w / 2);
      n.badge.setAttribute('transform', `translate(${-w / 2 + 2} ${NODE_H / 2 + 3})`);
    });
  }
  for (const e of edges.values()) {
    const a = nodes.get(e.a), b = nodes.get(e.b);
    e.line.setAttribute('x1', a.x); e.line.setAttribute('y1', a.y);
    e.line.setAttribute('x2', b.x); e.line.setAttribute('y2', b.y);
  }
  svg.toggleAttribute('data-clipped', !fits());
}

// A self-check the layout test reads: every node (badge included) is inside the picture and clear of the others.
function fits() {
  const frame = svg.getBoundingClientRect();
  const boxes = [...nodes.values()].map((n) => n.g.getBoundingClientRect());
  const inside = (b) => b.left >= frame.left - 0.5 && b.right <= frame.right + 0.5 && b.top >= frame.top - 0.5 && b.bottom <= frame.bottom + 0.5;
  const apart = (a, b) => a.right <= b.left + 0.5 || b.right <= a.left + 0.5 || a.bottom <= b.top + 0.5 || b.bottom <= a.top + 0.5;
  return boxes.every((b, i) => inside(b) && boxes.slice(i + 1).every((o) => apart(b, o)));
}
new ResizeObserver(() => nodes && layout()).observe(svg);

function setStatus(n, kind, text) {
  n.status.dataset.kind = kind;
  n.status.textContent = text;
}

// Two short word lines under the status: what it did, and what was dropped.
function showCounts(n) {
  n.work.textContent = n.answers ? `${n.answers} answer${n.answers === 1 ? '' : 's'}` : n.sent ? `sent ${n.sent}` : '';
  n.lost.textContent = n.dropped ? `${n.dropped} dropped` : '';
  n.title.textContent = `${n.id}: ${n.sent} sent, ${n.answers} answers, ${n.quiet} quiet, ${n.dropped} dropped`;
}

function showWaiting(n) {
  if (!holds[n.id]) return;
  const count = held.get(n.id)?.size ?? 0;
  n.badge.classList.toggle('idle', count === 0);
  n.badgeText.textContent = `waiting ${count}`;
}

// A marker for a request (circle) or an answer (diamond) travelling along an edge.
function travel(from, to, answer) {
  if (reduced.matches) return;
  const a = nodes.get(from), b = nodes.get(to);
  const shape = answer ? 'M0 -6 L6 0 L0 6 L-6 0Z' : 'M-5 0a5 5 0 1 0 10 0a5 5 0 1 0 -10 0';
  const m = el('path', { class: answer ? 'marker answer' : 'marker', d: shape }, layer('markers'));
  const at = (n) => `translate(${n.x}px, ${n.y}px)`;
  const run = m.animate([{ transform: at(a) }, { transform: at(b) }], { duration: 600, easing: 'ease-in-out' });
  run.finished.then(() => m.remove(), () => m.remove());
}

function addLine(kind, tag, text, ms) {
  const li = document.createElement('li');
  li.dataset.kind = kind;
  const time = document.createElement('time');
  time.textContent = `${(ms / 1000).toFixed(1)}s`;
  const label = document.createElement('span');
  label.className = 'tag';
  label.textContent = tag;
  li.append(time, label, ` ${text}`);
  const stick = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 40;
  logEl.append(li);
  if (++lines > MAX_LOG) {
    logEl.firstChild.remove();
    lines--;
  }
  if (stick) logEl.scrollTop = logEl.scrollHeight;
  return li;
}

// Quiet answers (a neuron that chose not to act) are the commonest event, so a run of them is one line.
function logQuiet(id, ms) {
  if (quietLine && quietLine.li === logEl.lastElementChild) {
    quietLine.names.add(id);
    quietLine.count++;
    quietLine.li.lastChild.textContent = ` \u00D7${quietLine.count} ${[...quietLine.names].join(', ')}`;
    return;
  }
  quietLine = { li: addLine('quiet', '◇ quiet', id, ms), names: new Set([id]), count: 1 };
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
      setStatus(nodes.get(id), 'acting', '▸ acting');
    }
    showWaiting(nodes.get(id));
  }
}

function onSignal(s) {
  const known = s.to.map((id) => nodes.get(id)).find(Boolean);
  const from = node(s.from, known ? Math.max(0, known.col - 1) : 0);
  for (const id of s.to) {
    node(id, from.col + 1);
    edge(s.from, id);
  }
  const where = s.to.join(', ');
  if (s.replyTo) {
    setStatus(from, s.quiet ? 'quiet' : 'answer', s.quiet ? '◇ quiet' : '◆ answer');
    if (s.quiet) {
      from.quiet++;
      logQuiet(s.from, s.ms);
    } else {
      from.answers++;
      for (const id of s.to) travel(s.from, id, true);
      addLine('answer', '◆ answer', `${s.from} → ${where}: ${s.text}`, s.ms);
    }
  } else {
    from.sent++;
    for (const id of s.to) travel(s.from, id, false);
    addLine('signal', '● signal', `${s.from} → ${where}: ${s.text}`, s.ms);
    hold(s);
  }
  showCounts(from);
}

function onDrop(e) {
  const [short, long] = WHY[e.reason] ?? [e.reason, e.reason];
  const n = node(e.neuron, 0);
  n.dropped++;
  setStatus(n, 'drop', `✕ ${short}`);
  showCounts(n);
  held.get(e.neuron)?.delete(e.signal.id);
  showWaiting(n);
  addLine('drop', '✕ dropped', `${e.neuron}: ${long} (${e.signal.from}: ${e.signal.text})`, e.ms);
}

function onError(e) {
  // A run-level failure names no neuron: it is logged and draws nothing.
  if (e.neuron) setStatus(node(e.neuron, 0), 'error', '▲ error');
  addLine('error', '▲ error', `${e.neuron ?? 'the run'}: ${e.message}`, e.ms);
}

function finish(done) {
  $('state').textContent = done ? `Finished: ${nodes.size} neurons, ${edges.size} links` : 'Connection lost';
  source.close();
}

function run() {
  reset();
  $('run').textContent = 'Run again';
  $('state').textContent = 'Running';
  source = new EventSource(`/events?demo=${encodeURIComponent($('demo').value)}`);
  source.onmessage = (m) => {
    const e = JSON.parse(m.data);
    if (e.type === 'start') holds = e.holds;
    else if (e.type === 'signal') onSignal(e);
    else if (e.type === 'drop') onDrop(e);
    else if (e.type === 'error' || e.type === 'fail') onError({ ...e, neuron: e.type === 'fail' ? null : e.neuron });
    else if (e.type === 'done') finish(true);
  };
  // EventSource would reconnect, and a reconnect would run the demo again.
  source.onerror = () => source.readyState !== 2 && finish(false);
}

const preset = new URLSearchParams(location.search).get('demo');
if ([...$('demo').options].some((o) => o.value === preset)) $('demo').value = preset;

$('controls').addEventListener('submit', (e) => {
  e.preventDefault();
  run();
});
reset();
