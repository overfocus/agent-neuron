// The multiplayer desk proxy: one small stdlib-Node server (no build, no
// CDNs, every value rendered as text) in front of the engine desk of
// decision 0008. It drives the desk with the x-desk-token contract and one
// Connection-close http.request per call - a fetch's pool keeps a closed
// desk's socket around and dies on its next write, and the desk dies with
// the run. Three person routes, /p/ada and /p/lin (role ic) and /p/sam
// (role lead): each person sees the waiting items of their own neuron,
// filtered by the payload's answerable_by, votes or gates from buttons, and
// reads the shared decision ledger at /log - {who, item, kind, text, t},
// written here when the person answers, right after the POST to the desk
// lands. The person's name lives in the ledger (demo surface) and nowhere
// in the protocol body.
import http from 'node:http';

const DESK_PORT = Number(process.env.TEAM_DEMO_DESK_PORT);
const DESK_TOKEN = process.env.TEAM_DEMO_DESK_TOKEN || '';
if (!DESK_PORT || !DESK_TOKEN) {
  console.error('team: pageserver needs TEAM_DEMO_DESK_PORT and TEAM_DEMO_DESK_TOKEN');
  process.exit(2);
}

const persons = {
  ada: { name: 'Ada', role: 'ic', neuron: 'ic1', says: 'IC reviewer - votes with Lin on which patch merges' },
  lin: { name: 'Lin', role: 'ic', neuron: 'ic2', says: 'IC reviewer - votes with Ada on which patch merges' },
  sam: { name: 'Sam', role: 'lead', neuron: 'lead', says: 'the lead - gates the merge, and breaks a tie' },
};

const ledger = []; // {who, item, kind, text, t}
let runEnded = false; // the desk closed with the run

// One connection per call (`Connection: close`), the timbre run.mjs pattern:
// plain http.request, never a pooled fetch, against the desk.
const desk = (route, method, body) =>
  new Promise((resolve, reject) => {
    const req = http.request(
      {
        host: '127.0.0.1',
        port: DESK_PORT,
        path: route,
        method,
        headers: {
          host: `127.0.0.1:${DESK_PORT}`,
          ...(method === 'POST' && {
            origin: `http://127.0.0.1:${DESK_PORT}`,
            'content-type': 'application/json',
            'content-length': Buffer.byteLength(body),
          }),
          'x-desk-token': DESK_TOKEN,
          connection: 'close',
        },
      },
      (res) => {
        let raw = '';
        res.setEncoding('utf8');
        res.on('data', (c) => (raw += c));
        res.on('end', () => resolve({ ok: res.statusCode >= 200 && res.statusCode < 300, status: res.statusCode, text: raw }));
      },
    );
    req.on('error', reject);
    if (body != null) req.write(body);
    req.end();
  });

const deskState = async () => {
  try {
    const r = await desk('/state', 'GET', null);
    const j = JSON.parse(r.text);
    if (j.over) runEnded = true;
    return { over: !!j.over, items: j.items || [] };
  } catch {
    return { over: true, items: [], closed: true };
  }
};

const forPerson = (person, items) => (items || []).filter((it) => it.neuron === person.neuron && it.payload?.answerable_by === person.role);

const branchOf = (text) => String(text || '').trim().split('|')[0].trim().split(/\s+/)[0];

const quorumOf = () => {
  const votes = ledger.filter((l) => l.kind === 'vote');
  const gate = ledger.find((l) => l.kind === 'merge' || l.kind === 'refuse');
  const branches = votes.map((v) => branchOf(v.text));
  const direction = votes.length < 2 ? null : branches[0] === branches[1] ? branches[0] : 'tie';
  return { voted: votes.length, needed: 2, direction, gate: gate ? gate.kind : null, ended: runEnded };
};

const readBody = (req) =>
  new Promise((resolve, reject) => {
    let raw = '';
    req.on('data', (c) => (raw += c));
    req.on('end', () => resolve(raw));
    req.on('error', reject);
  });

const sendJson = (res, code, value) => {
  const body = JSON.stringify(value);
  res.writeHead(code, {
    'content-type': 'application/json; charset=utf-8',
    'cache-control': 'no-store',
    'content-length': Buffer.byteLength(body),
  });
  res.end(body);
};

const sendPage = (res, html) => {
  res.writeHead(200, {
    'content-type': 'text/html; charset=utf-8',
    'cache-control': 'no-store',
    'content-length': Buffer.byteLength(html),
  });
  res.end(html);
};

async function answer(req, res, person) {
  let body;
  try {
    body = JSON.parse((await readBody(req)) || '{}');
  } catch {
    return sendJson(res, 400, { error: 'the body is not one JSON value' });
  }
  const { item, choice, branch, why } = body;
  if (!['vote', 'merge', 'refuse'].includes(choice)) {
    return sendJson(res, 400, { error: `the choice '${choice}' is not vote, merge or refuse` });
  }
  const state = await deskState();
  const items = forPerson(person, state.items);
  const target = (item && items.find((i) => i.id === item)) || items.find((i) => i.status === 'waiting');
  if (!target || target.status !== 'waiting') {
    return sendJson(res, 409, { error: `${person.name} has no waiting item to answer` });
  }
  const options = (target.payload?.options ?? []).map((o) => o.branch);
  let kind;
  let deskKind;
  let text;
  if (choice === 'vote') {
    if (person.role !== 'ic' || !options.includes(branch)) {
      return sendJson(res, 400, { error: `a vote needs an option of this item (${options.join(', ')})` });
    }
    kind = 'vote';
    deskKind = 'edit'; // a vote is an edit at the desk level: the person's text
    text = why ? `${branch} | why: ${why}` : branch;
  } else if (choice === 'merge') {
    if (person.role !== 'lead' || !options.includes(branch)) {
      return sendJson(res, 400, { error: `the gate needs an option of this item (${options.join(', ')})` });
    }
    kind = 'merge';
    deskKind = 'edit';
    text = `merge ${branch}`;
  } else {
    if (person.role !== 'lead') return sendJson(res, 400, { error: 'only the lead can refuse' });
    kind = 'refuse';
    deskKind = 'no'; // quiet: nothing is forwarded, nothing merges
    text = '';
  }
  const posted = await desk('/answer', 'POST', JSON.stringify({ item: target.id, kind: deskKind, text }));
  let value;
  try {
    value = JSON.parse(posted.text);
  } catch {
    value = {};
  }
  if (!posted.ok) {
    return sendJson(res, posted.status === 200 ? 502 : posted.status, { error: value.error || `the desk refused (${posted.status})` });
  }
  const line = { who: person.name, item: target.id, kind, text, t: Date.now() };
  ledger.push(line);
  console.log(`ledger: ${JSON.stringify(line)}`);
  return sendJson(res, 200, { ok: true, line, quorum: quorumOf() });
}

const page = (person, key) => {
  const others = Object.values(persons)
    .filter((p) => p.name !== person.name)
    .map((p) => `<span class="addr">${p.name}: http://127.0.0.1:${own_port()}/p/${p.name.toLowerCase()}</span>`);
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>team decisions - ${person.name}</title>
<style>
  :root { color-scheme: dark; }
  body { margin: 0; padding: 24px; background: #000; color: #e5e7eb;
         font-family: ui-sans-serif, system-ui, "Helvetica Neue", Arial, sans-serif;
         font-size: 15px; line-height: 1.5; }
  h1 { font-size: 17px; margin: 0 0 2px; color: #39ff00; letter-spacing: .02em; }
  h1 span { color: #9ca3af; font-weight: normal; }
  .role { color: #9ca3af; font-size: 13px; margin: 0 0 10px; }
  .hint { border: 1px solid #1f2937; padding: 8px 12px; margin: 0 0 18px; max-width: 720px;
          color: #9ca3af; font-size: 13px; }
  .hint b { color: #e5e7eb; font-weight: normal; }
  .addr { display: block; white-space: nowrap; overflow-wrap: anywhere; }
  #quorum { border: 1px solid #06b6d4; color: #06b6d4; max-width: 720px; margin: 0 0 18px;
            padding: 6px 10px; font-size: 14px; }
  .card { border: 1px solid #1f2937; margin: 0 0 20px; max-width: 720px; padding: 14px 16px; }
  .card.waiting h2 { color: #39ff00; }
  .card h2 { font-size: 15px; margin: 2px 0 10px; color: #4b5563; }
  .neuron { color: #9ca3af; font-size: 13px; }
  .why, .draft, .proposal { margin: 0 0 8px; color: #9ca3af; font-size: 13px; }
  .draft b, .proposal b { color: #9ca3af; font-weight: normal; display: block; font-size: 12px; }
  .draft, .proposal { background: #060606; border: 1px solid #1f2937; padding: 8px 10px;
                      white-space: pre-wrap; overflow-wrap: anywhere; font-size: 14px; color: #e5e7eb; }
  .notes { margin: 0 0 10px; padding-left: 18px; color: #9ca3af; font-size: 13px; }
  .row { margin-top: 10px; display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  .whyline { background: #060606; border: 1px solid #1f2937; color: #e5e7eb; font: inherit;
             padding: 4px 8px; width: 320px; max-width: 100%; }
  button { background: #060606; border: 1px solid #39ff00; color: #39ff00; cursor: pointer;
           font: inherit; padding: 4px 14px; }
  button.nah { border-color: #1f2937; color: #9ca3af; }
  button:disabled { border-color: #1f2937; color: #4b5563; cursor: default; }
  .state { color: #9ca3af; font-size: 12px; margin-left: 8px; }
  .waiting .state { color: #39ff00; }
  .err { color: #f5a0a0; font-size: 13px; margin-top: 6px; min-height: 18px; }
  h2.feedhead { font-size: 14px; color: #9ca3af; margin: 26px 0 8px; }
  #feed { border: 1px solid #1f2937; max-width: 720px; margin: 0; padding: 10px 10px 10px 26px;
          color: #9ca3af; font-size: 13px; }
  #feed .who { color: #e5e7eb; }
  .ended { border: 1px solid #f5a0a0; color: #f5a0a0; max-width: 720px; padding: 6px 10px;
           margin: 0 0 18px; display: none; }
</style>
</head>
<body>
<h1>team decisions - ${person.name} <span>(desk of ${person.neuron})</span></h1>
<p class="role">${person.says}</p>
<div class="hint"><b>multiplayer:</b> open the other desks in their own browser windows:
${others.join(' ')}</div>
<div id="ended">The run has ended - the desk closed with it. Reload nothing; the ledger below stands.</div>
<div id="quorum">quorum: reading the desk</div>
<div id="items"></div>
<h2 class="feedhead">Decision ledger (shared)</h2>
<ul id="feed"></ul>
<script>
"use strict";
const ME = '${key}';
const IS_LEAD = ${person.role === 'lead'};
const el = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};
const post = async (body) => {
  const r = await fetch('/p/' + ME + '/answer', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
  const j = await r.json().catch(() => ({}));
  return { ok: r.ok, j };
};
let stopped = false;
const quorumLine = (q) => {
  if (q.gate === 'merge') return 'quorum met - gate: merge (' + q.voted + '/' + q.needed + ' voted, ' + q.direction + ')';
  if (q.gate === 'refuse') return 'quorum met - gate: refused (' + q.voted + '/' + q.needed + ' voted, ' + (q.direction || 'tie') + ')';
  if (q.direction === 'tie') return 'the ICs tie - the lead decides next (' + q.voted + '/' + q.needed + ' voted)';
  if (q.direction) return 'quorum met: ' + q.direction + ' - waiting for the lead to gate (' + q.voted + '/' + q.needed + ' voted)';
  return 'quorum: ' + q.voted + ' of ' + q.needed + ' IC votes in, waiting';
};
function card(item) {
  if (item.status === 'answered') return null; // the feed below carries it
  const p = item.payload || {};
  const card = el('div', 'card ' + item.status);
  card.appendChild(el('div', 'neuron', 'asked ' + item.neuron + ' - item ' + item.id));
  card.appendChild(el('h2', null, p.title || item.neuron));
  if (p.why) card.appendChild(el('p', 'why', p.why));
  if (item.draft !== undefined) {
    const d = el('div', 'draft');
    d.appendChild(el('b', null, 'draft'));
    d.appendChild(document.createTextNode(item.draft));
    card.appendChild(d);
  }
  if (item.proposal !== undefined) {
    const pr = el('div', 'proposal');
    pr.appendChild(el('b', null, 'proposal'));
    pr.appendChild(document.createTextNode(item.proposal));
    card.appendChild(pr);
  }
  for (const n of p.notes || []) card.appendChild(el('li', 'notes', n));
  const options = p.options || [];
  const isLead = IS_LEAD;
  const row = el('div', 'row');
  const whyline = isLead ? null : el('input', 'whyline');
  if (whyline) { whyline.placeholder = 'optional one-line why, rides on your vote'; whyline.disabled = item.status !== 'waiting'; }
  for (const o of options) {
    const b = el('button', null, isLead ? 'Merge ' + o.branch : 'Vote ' + o.branch);
    b.disabled = item.status !== 'waiting';
    b.addEventListener('click', async () => {
      b.disabled = true;
      const r = await post({ item: item.id, choice: isLead ? 'merge' : 'vote', branch: o.branch, why: whyline ? whyline.value : '' });
      if (!r.ok && r.j && r.j.error) { card.querySelector('.err').textContent = r.j.error; b.disabled = false; }
    });
    row.appendChild(b);
  }
  if (isLead) {
    const nah = el('button', 'nah', 'Refuse');
    nah.disabled = item.status !== 'waiting';
    nah.addEventListener('click', async () => {
      nah.disabled = true;
      const r = await post({ item: item.id, choice: 'refuse' });
      if (!r.ok && r.j && r.j.error) { card.querySelector('.err').textContent = r.j.error; nah.disabled = false; }
    });
    row.appendChild(nah);
    row.appendChild(el('span', 'state', 'Merge applies the patch for real; Refuse merges nothing and is recorded'));
  } else if (whyline) {
    row.appendChild(whyline);
  }
  card.appendChild(row);
  card.appendChild(el('div', 'err'));
  card.appendChild(el('span', 'state', item.status));
  return card;
}
function render(state, log) {
  const q = document.getElementById('quorum');
  q.textContent = quorumLine(log.quorum || {});
  const ended = document.getElementById('ended');
  ended.style.display = (state.over || state.closed) ? 'block' : 'none';
  const root = document.getElementById('items');
  root.textContent = '';
  if (!state.items || !state.items.length) {
    root.appendChild(el('p', null, 'Nothing is waiting on you. The decision feed is below.'));
  }
  for (const item of state.items) {
    const c = card(item);
    if (c) root.appendChild(c);
  }
  const feed = document.getElementById('feed');
  feed.textContent = '';
  for (const line of log.lines || []) {
    const li = el('li', null, '');
    li.appendChild(el('span', 'who', line.who));
    li.appendChild(document.createTextNode(' - ' + line.kind + (line.text ? ': ' : ':') + ' ' + line.text));
    feed.appendChild(li);
  }
  if (!(log.lines || []).length) feed.appendChild(el('li', null, 'No decision yet.'));
}
async function tick() {
  if (stopped) return;
  try {
    const state = await (await fetch('/p/' + ME + '/state')).json();
    const log = await (await fetch('/log')).json();
    if (state.items === undefined && state.error) throw new Error(state.error);
    render(state, log);
  } catch (e) {
    document.getElementById('quorum').textContent =
      e && /Failed to fetch/i.test(String(e))
        ? 'the demo has ended - the pageserver went with it'
        : 'the pageserver is not answering: ' + e;
  }
  setTimeout(tick, 1200);
}
tick();
</script>
</body>
</html>`;
};

// -- The server --------------------------------------------------------------

let PORT = 0;
const own_port = () => PORT;
const server = http.createServer((req, res) => {
  void route(req, res);
});

async function route(req, res) {
  try {
    const path = (req.url || '/').split('?')[0];
    let m;
    if (req.method === 'GET' && (m = path.match(/^\/p\/(ada|lin|sam)$/))) {
      return sendPage(res, page(persons[m[1]], m[1]));
    }
    if (req.method === 'GET' && (m = path.match(/^\/p\/(ada|lin|sam)\/state$/))) {
      const person = persons[m[1]];
      const state = await deskState();
      const items = forPerson(person, state.items);
      return sendJson(res, 200, { over: !!state.over, closed: !!state.closed, items, neuron: person.neuron });
    }
    if (req.method === 'POST' && (m = path.match(/^\/p\/(ada|lin|sam)\/answer$/))) {
      return await answer(req, res, persons[m[1]]);
    }
    if (req.method === 'GET' && path === '/log') {
      return sendJson(res, 200, { over: runEnded, quorum: quorumOf(), lines: ledger });
    }
    if (req.method === 'GET' && path === '/') {
      res.writeHead(302, { location: '/p/ada' });
      return res.end();
    }
    return sendJson(res, 404, { error: 'nothing here' });
  } catch (e) {
    return sendJson(res, 500, { error: `pageserver: ${e?.message || e}` });
  }
}

server.listen(0, '127.0.0.1', () => {
  PORT = server.address().port;
  console.log(`team: pageserver on http://127.0.0.1:${PORT} (desk 127.0.0.1:${DESK_PORT} behind it, Connection-close requests)`);
  for (const p of Object.values(persons)) {
    console.log(`team: ${p.name} ${p.role === 'ic' ? '(role ic)' : '(role lead)'} http://127.0.0.1:${PORT}/p/${p.name.toLowerCase()}`);
  }
  console.log('team: open Ada\'s and Lin\'s pages in two browser windows, Sam\'s in a third (or answer with --headless through the same POST path)');
  console.log(`team: ledger at http://127.0.0.1:${PORT}/log`);
});
