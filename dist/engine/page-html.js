// The people page's HTML (decision 0008): kevinferron.com's look, one inline style and one inline script under the page's nonce. Every
// value from a payload is set with textContent: a payload comes from a program and may contain anything. __NONCE__, __TOKEN__ and
// __TITLE__ (a JSON string) are filled in by page.ts. The favicon is the family mark (assets/brand/icon.svg in this repo), inlined
// so the page fetches nothing.
export const PAGE = `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="referrer" content="no-referrer"><title>agent-neuron: people</title><link rel="icon" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 256 256' width='256' height='256'%3E%3Cdefs%3E%3Cfilter id='glow' x='-40%25' y='-40%25' width='180%25' height='180%25'%3E%3CfeDropShadow dx='0' dy='0' stdDeviation='6' flood-color='%2339ff00' flood-opacity='0.55'/%3E%3C/filter%3E%3C/defs%3E%3Crect x='4' y='4' width='248' height='248' rx='58' fill='%230a0a0a'/%3E%3Cg filter='url(%23glow)' stroke='%2339ff00' stroke-width='13' fill='none' stroke-linecap='round' stroke-linejoin='round' %3E%3Cpath d='M60.3 121.5 L26.6 116.8'/%3E%3Cpath d='M68.2 104.6 L34.0 73.8'/%3E%3Cpath d='M87.0 94.4 L80.1 50.9'/%3E%3Cpath d='M80.1 50.9 L83.5 36.3'/%3E%3Cpath d='M80.1 50.9 L72.4 38.1'/%3E%3Ccircle cx='92' cy='126' r='31'/%3E%3Cpath d='M122.1 115.1 C 152 138 178 144 190.0 158.0'/%3E%3C/g%3E%3Ccircle cx='200' cy='168' r='12.5' fill='%2339ff00' filter='url(%23glow)'/%3E%3Cg filter='url(%23glow)' stroke='%2322d3ee' fill='none' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M153.9 135.2 L168.2 142.7' stroke-width='9'/%3E%3C/g%3E%3C/svg%3E">
<style nonce="__NONCE__">
@font-face{font-family:"Josefin Sans";src:url(/fonts/josefin.ttf?t=__TOKEN__) format("truetype");font-weight:100 700;font-display:swap}
@font-face{font-family:"Pixelify Sans";src:url(/fonts/pixelify.ttf?t=__TOKEN__) format("truetype");font-weight:400 700;font-display:swap}
:root{color-scheme:dark;--bg:#0a0a0a;--card:#111;--ink:#f2f2f2;--mute:#a8a8b0;--line:rgba(255,255,255,.15);--acc:#39ff00;--accbg:rgba(57,255,0,.12);--cyan:#22d3ee;--warn:#ffc14d;--bad:#ff7a7a;--badbg:rgba(255,122,122,.13);--pixel:"Pixelify Sans",ui-monospace,monospace;--body:"Josefin Sans",system-ui,-apple-system,sans-serif;--mono:ui-monospace,SFMono-Regular,Menlo,monospace}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--ink);font:17px/1.55 var(--body)}
header{position:sticky;top:0;z-index:2;background:rgba(10,10,10,.94);border-bottom:1px solid var(--line);padding:12px 16px}
.wrap{max-width:1100px;margin:0 auto;padding:0 16px}
.brand{display:flex;flex-wrap:wrap;align-items:baseline;gap:4px 14px}
h1{font:700 20px/1.2 var(--pixel);margin:0;color:var(--acc);text-transform:uppercase;letter-spacing:.06em}
.by{display:inline-flex;align-items:center;gap:8px;color:var(--mute);font-size:14px}.by svg{height:18px;width:auto;overflow:visible}
.sub{color:var(--mute);font-size:15px;margin-top:4px}.sub b{font:700 16px var(--pixel);color:var(--ink)}
.card{background:var(--card);border:1px solid var(--line);border-top:2px solid var(--warn);border-radius:12px;padding:16px;margin:16px 0}
.card.answered{border-top-color:var(--acc)}.card.no{border-top-color:var(--bad)}.card.withdrawn{border-top-color:var(--line);opacity:.75}
.top{display:flex;gap:10px;align-items:center;flex-wrap:wrap}.neuron{font:700 15px/1.3 var(--pixel);color:var(--cyan)}
.tag{font:600 14px/1 var(--pixel);text-transform:uppercase;letter-spacing:.06em;padding:4px 10px;border-radius:99px;color:var(--warn);border:1px solid var(--line)}
.answered .tag{color:var(--acc)}.no .tag{color:var(--bad)}.withdrawn .tag{color:var(--mute)}
.desc{color:var(--mute);font-size:15px;margin:6px 0 0}
.cols{display:grid;grid-template-columns:1fr 1fr;gap:12px;margin-top:12px}@media (max-width:760px){.cols{grid-template-columns:1fr}}
.col h3{font:600 14px/1.2 var(--pixel);letter-spacing:.06em;text-transform:uppercase;color:var(--mute);margin:0 0 6px}
.txt{white-space:pre-wrap;overflow-wrap:anywhere;margin:0;padding:12px;border-radius:10px;background:var(--bg);border:1px solid var(--line)}
.txt.json{font:14px/1.5 var(--mono)}
.qtitle{font:600 17px/1.3 var(--mono);margin:8px 0 0;overflow-wrap:anywhere}
.notes{margin:12px 0 0;padding-left:20px;color:var(--mute);font-size:15px}.notes li{margin:4px 0;overflow-wrap:anywhere}
del+ins{margin-left:3px}del{color:var(--bad);background:var(--badbg);text-decoration:line-through}ins{color:var(--acc);background:var(--accbg);text-decoration:none}
.acts{display:flex;flex-wrap:wrap;gap:8px;margin-top:12px}
button{font:600 15px/1 var(--body);min-height:44px;padding:8px 16px;border-radius:10px;border:1px solid var(--line);background:var(--card);color:var(--ink);cursor:pointer}
button:hover{border-color:var(--mute)}button.go{background:var(--acc);border-color:var(--acc);color:#000}button.no{color:var(--bad)}
textarea{width:100%;min-height:140px;font:inherit;padding:12px;border-radius:10px;border:1px solid var(--line);background:var(--bg);color:var(--ink);margin-top:10px}
.state{font-size:15px;color:var(--mute);margin-top:10px}.err{color:var(--bad)}
.empty{color:var(--mute);margin:40px 0;text-align:center}
:focus-visible{outline:2px solid var(--acc);outline-offset:2px}
@media (prefers-reduced-motion:no-preference){button{transition:border-color .15s,background-color .15s}}
</style></head><body>
<header><div class="wrap"><div class="brand"><h1>People</h1>
<span class="by">agent-neuron, by <svg viewBox="0 0 390 60" role="img" aria-label="kevinferron.com"><defs><linearGradient id="kfg" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="390" y2="0"><stop offset="0" stop-color="#39ff00"/><stop offset=".45" stop-color="#7dffb0"/><stop offset="1" stop-color="#22d3ee"/></linearGradient><filter id="kfglow" x="-10%" y="-60%" width="120%" height="220%" color-interpolation-filters="sRGB"><feDropShadow dx="0" dy="0" stdDeviation=".18" flood-color="#39ff00" flood-opacity=".28" result="t"/><feDropShadow in="t" dx="0" dy="0" stdDeviation="1.2" flood-color="#22d3ee" flood-opacity=".13"/></filter></defs><text x="195" y="32" text-anchor="middle" dominant-baseline="middle" font-family="Pixelify Sans, monospace" font-weight="600" font-size="40" letter-spacing="2" fill="url(#kfg)" filter="url(#kfglow)">kevinferron.com</text></svg></span></div>
<p class="sub" id="sub" role="status" aria-live="polite"></p></div></header>
<main class="wrap" id="list"><p class="empty">Waiting for the first question…</p></main>
<script nonce="__NONCE__">
"use strict";
const TOKEN = "__TOKEN__", TITLE = __TITLE__, H = { "x-neuron-token": TOKEN };
const list = document.getElementById("list"), sub = document.getElementById("sub");
const cards = new Map(); // id -> { el, state }
const known = new Map(); // id -> the item as last seen, for the counts
function el(tag, cls, text) { const n = document.createElement(tag); if (cls) n.className = cls; if (text !== undefined) n.textContent = text; return n; }
/** Word diff of two texts as nodes (no HTML is ever parsed). */
function diff(a, b) {
  const A = a.split(/(\\s+)/), B = b.split(/(\\s+)/), n = A.length, m = B.length, out = document.createDocumentFragment();
  if (n * m > 250000) { out.append(b); return out; }
  const L = Array.from({ length: n + 1 }, () => new Int32Array(m + 1));
  for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) L[i][j] = A[i] === B[j] ? L[i + 1][j + 1] + 1 : Math.max(L[i + 1][j], L[i][j + 1]);
  let i = 0, j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && A[i] === B[j]) { out.append(A[i]); i++; j++; }
    else if (i < n && (j === m || L[i + 1][j] >= L[i][j + 1])) out.append(el("del", "", A[i++]));
    else out.append(el("ins", "", B[j++]));
  }
  return out;
}
const isPair = (p) => p && typeof p === "object" && typeof p.draft === "string" && typeof p.proposal === "string";
const approval = (p) => typeof p === "string" ? p : (p && typeof p === "object" && typeof p.proposal === "string") ? p.proposal : JSON.stringify(p, null, 2);
function body(it) {
  const cols = el("div", "cols"), p = it.payload;
  const col = (title, node, cls) => { const c = el("div", "col"); c.append(el("h3", "", title)); const t = el("p", "txt" + (cls ? " " + cls : "")); t.append(node); c.append(t); cols.append(c); };
  if (isPair(p)) { col("Draft", document.createTextNode(p.draft)); col("Proposal (changes from the draft)", diff(p.draft, p.proposal)); }
  else if (typeof p === "string") col("Asked", document.createTextNode(p));
  else col("Asked", document.createTextNode(JSON.stringify(p, null, 2)), "json");
  return cols;
}
const STATE = { waiting: "Waiting", withdrawn: "Withdrawn", approve: "Approved", edit: "Edited", no: "No" };
async function answer(it, kind, text, card) {
  const err = card.querySelector(".err"); err.textContent = "";
  try {
    const r = await fetch("/api/answer", { method: "POST", headers: { ...H, "content-type": "application/json" }, body: JSON.stringify({ id: it.id, kind, text }) });
    const j = await r.json();
    if (!r.ok) { err.textContent = j.error || ("HTTP " + r.status); return; }
    draw(j.item);
    count();
  } catch (e) { err.textContent = "The engine is not answering: the run may be over."; }
}
function count() {
  const all = [...known.values()], waiting = all.filter((i) => i.state === "waiting").length;
  sub.textContent = "";
  sub.append(document.createTextNode(TITLE + " · "), el("b", "", String(waiting)), document.createTextNode(" waiting · " + (all.length - waiting) + " done"));
}
function draw(it) {
  known.set(it.id, it);
  const label = it.state === "answered" ? it.kind : it.state;
  const shown = cards.get(it.id);
  if (shown && shown.state === label) return;
  const card = el("section", "card " + (it.state === "answered" ? "answered" + (it.kind === "no" ? " no" : "") : it.state));
  card.setAttribute("aria-label", it.neuron + ": " + STATE[label]);
  const top = el("div", "top"); top.append(el("span", "neuron", it.neuron), el("span", "tag", STATE[label]));
  card.append(top);
  if (isPair(it.payload) && typeof it.payload.title === "string") card.append(el("p", "qtitle", it.payload.title));
  if (it.description) card.append(el("p", "desc", it.description));
  card.append(body(it));
  const notes = isPair(it.payload) && Array.isArray(it.payload.notes) ? it.payload.notes.filter((n) => typeof n === "string") : [];
  if (notes.length) { const ul = el("ul", "notes"); for (const n of notes) ul.append(el("li", "", n)); card.append(ul); }
  if (it.state === "waiting") {
    const acts = el("div", "acts"), ok = el("button", "go", "Approve"), ed = el("button", "", "Edit"), no = el("button", "no", "No");
    ok.type = ed.type = no.type = "button";
    ok.addEventListener("click", () => answer(it, "approve", undefined, card));
    no.addEventListener("click", () => answer(it, "no", undefined, card));
    ed.addEventListener("click", () => {
      if (card.querySelector("textarea")) return;
      const t = el("textarea"); t.value = approval(it.payload); t.setAttribute("aria-label", "Your text for " + it.neuron);
      const save = el("button", "go", "Send my text"), cancel = el("button", "", "Cancel"); save.type = cancel.type = "button";
      const row = el("div", "acts"); row.append(save, cancel);
      save.addEventListener("click", () => { if (!t.value.trim()) { card.querySelector(".err").textContent = "Write something, or press No."; return; } answer(it, "edit", t.value, card); });
      cancel.addEventListener("click", () => { t.remove(); row.remove(); ed.focus(); });
      acts.after(t, row); t.focus();
    });
    acts.append(ok, ed, no);
    card.append(acts);
  } else if (it.state === "withdrawn") {
    card.append(el("p", "state", "No longer waiting: a newer version arrived, the time ran out, or the run was cut."));
  } else {
    card.append(el("p", "state", it.kind === "no" ? "Answered no: nothing goes on." : it.kind === "edit" ? "Your text went on:" : "Approved: this went on."));
    if (it.kind === "edit") { const t = el("p", "txt"); t.textContent = it.answer; card.append(t); }
  }
  card.append(el("p", "state err"));
  if (shown) shown.el.replaceWith(card); else { const e = list.querySelector(".empty"); if (e) e.remove(); list.append(card); }
  cards.set(it.id, { el: card, state: label });
}
let gone = 0;
async function poll() {
  try {
    const r = await fetch("/api/items", { headers: H });
    const { items } = await r.json(); gone = 0;
    for (const it of items) draw(it);
    count();
  } catch (e) {
    if (++gone === 3) sub.textContent = TITLE + " · the run is over (or the engine stopped): this page no longer updates.";
  }
  setTimeout(poll, 1000);
}
poll();
</script></body></html>`;
//# sourceMappingURL=page-html.js.map