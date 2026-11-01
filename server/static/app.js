'use strict';
/* Hstle — client. Nessuna dipendenza: tutto quello che serve è qui. */
const $ = (id) => document.getElementById(id);
const TOKEN_KEY = 'hstle-token';
const CHAN_KEY = 'hstle-channel';
const ROLES = ['mic', 'cam', 'screen', 'sa'];  // le quattro "corsie" fisse di ogni connessione WebRTC (sa = audio dello schermo)

const lsGet = (k) => { try { return localStorage.getItem(k); } catch { return null; } };
const lsSet = (k, v) => { try { localStorage.setItem(k, v); } catch {} };
const lsDel = (k) => { try { localStorage.removeItem(k); } catch {} };

let token = lsGet(TOKEN_KEY);
let ws, retry, mode = 'login', me = null;

/* ---------- icone ---------- */
const ICON = {
  plus: 'M12 5v14M5 12h14',
  x: 'M18 6 6 18M6 6l12 12',
  menu: 'M4 6h16M4 12h16M4 18h16',
  send: 'M22 2 11 13M22 2l-7 20-4-9-9-4z',
  settings: 'M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1 14h6M9 8h6M17 16h6',
  logout: 'M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4M16 17l5-5-5-5M21 12H9',
  userplus: 'M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2M9 3a4 4 0 1 0 0 8 4 4 0 0 0 0-8zM19 8v6M22 11h-6',
  shield: 'M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z',
  users: 'M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2M9 3a4 4 0 1 0 0 8 4 4 0 0 0 0-8zM23 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75',
  mic: 'M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3zM19 10v2a7 7 0 0 1-14 0v-2M12 19v3',
  headphones: 'M3 18v-6a9 9 0 0 1 18 0v6M21 19a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3zM3 19a2 2 0 0 0 2 2h1a2 2 0 0 0 2-2v-3a2 2 0 0 0-2-2H3z',
  video: 'M23 7l-7 5 7 5V7zM3 5h11a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2z',
  monitor: 'M4 3h16a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2zM8 21h8M12 17v4',
  hash: 'M4 9h16M4 15h16M10 3 8 21M16 3l-2 18',
  volume: 'M11 5 6 9H2v6h4l5 4V5zM15.5 8.5a5 5 0 0 1 0 7M19 5a10 10 0 0 1 0 14',
  'volume-x': 'M11 5 6 9H2v6h4l5 4V5zM22 9l-6 6M16 9l6 6',
  maximize: 'M8 3H5a2 2 0 0 0-2 2v3M21 8V5a2 2 0 0 0-2-2h-3M3 16v3a2 2 0 0 0 2 2h3M16 21h3a2 2 0 0 0 2-2v-3',
  minimize: 'M8 3v3a2 2 0 0 1-2 2H3M21 8h-3a2 2 0 0 1-2-2V3M3 16h3a2 2 0 0 1 2 2v3M16 21v-3a2 2 0 0 1 2-2h3',
  pin: 'M12 17v5M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V17h14v-1.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V6h1a2 2 0 0 0 0-4H8a2 2 0 0 0 0 4h1z',
  pencil: 'M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5z',
  trash: 'M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6M10 11v6M14 11v6',
  upload: 'M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M17 8l-5-5-5 5M12 3v12',
  wave: 'M2 12h2M6 8v8M10 4v16M14 8v8M18 6v12M22 12h-2',
  message: 'M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z',
};
const SLASH = 'M3 3l18 18';
function ico(name) {
  const off = name.endsWith('-off');
  const base = off ? name.slice(0, -4) : name;
  const d = (ICON[base] || '') + (off ? SLASH : '');
  return `<svg class="i" viewBox="0 0 24 24" aria-hidden="true"><path d="${d}"/></svg>`;
}
const LOGO = `<svg viewBox="0 0 32 32" aria-hidden="true"><defs><linearGradient id="lg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#a78bfa"/><stop offset="1" stop-color="#5b34c4"/></linearGradient></defs><rect width="32" height="32" rx="9" fill="url(#lg)"/><path d="M10 8v16M10 17c0-3 2-5 5-5s5 2 5 5v7" fill="none" stroke="#fff" stroke-width="3" stroke-linecap="round"/></svg>`;
function hydrate(root = document) {
  root.querySelectorAll('[data-icon]').forEach((n) => { n.innerHTML = ico(n.dataset.icon); });
  root.querySelectorAll('[data-logo]').forEach((n) => { n.innerHTML = LOGO; });
}

/* ---------- utilità ---------- */
function el(tag, props, ...kids) {
  const e = document.createElement(tag);
  Object.assign(e, props || {});
  e.append(...kids);
  return e;
}
function iconBtn(name, title, onclick, cls = '') {
  const b = el('button', { type: 'button', className: 'ib ' + cls, title, onclick });
  b.setAttribute('aria-label', title);
  b.innerHTML = ico(name);
  return b;
}
function show(view) {
  $('auth').hidden = view !== 'auth';
  $('app').hidden = view !== 'app';
}
let toastTimer;
function toast(text) {
  const t = $('toast');
  t.textContent = text;
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { t.hidden = true; }, 4500);
}
function safeColor(c) { return /^#[0-9a-f]{6}$/i.test(c || '') ? c : ''; }
function fmtTime(ts) { return new Date(ts * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }); }
function sameDay(a, b) { return new Date(a * 1000).toDateString() === new Date(b * 1000).toDateString(); }
function fmtDay(ts) { return new Date(ts * 1000).toLocaleDateString([], { weekday: 'long', day: 'numeric', month: 'long' }); }

/** Finestrella di domanda o conferma al posto di prompt/confirm del browser. */
function ask({ title, text, input, value = '', ok = 'Conferma', danger = false, readonly = false, copy = false }) {
  return new Promise((resolve) => {
    const d = el('dialog', { className: 'small' });
    let result = input ? null : false;
    const field = input ? el('input', { value, maxLength: 40, autocomplete: 'off', readOnly: readonly }) : null;
    const okBtn = el('button', { className: 'btn' + (danger ? ' danger' : ''), textContent: ok });
    const cancel = el('button', { type: 'button', className: 'btn ghost', textContent: 'Annulla', onclick: () => d.close() });
    const form = el('form', { className: 'askbox', method: 'dialog' }, el('h2', { textContent: title }));
    if (text) form.append(el('p', { textContent: text }));
    if (field) form.append(field);
    form.append(el('div', { className: 'row' }, cancel, okBtn));
    form.onsubmit = (e) => {
      e.preventDefault();
      result = field ? field.value : true;
      if (copy && field) { try { navigator.clipboard.writeText(field.value); } catch {} }
      d.close();
    };
    d.append(form);
    d.onclose = () => { d.remove(); resolve(result); };
    d.onclick = (e) => { if (e.target === d) d.close(); };
    document.body.append(d);
    d.showModal();
    if (field) { field.focus(); field.select(); } else okBtn.focus();
  });
}

async function api(path, body, authed) {
  const headers = { 'Content-Type': 'application/json' };
  if (authed && token) headers.Authorization = 'Bearer ' + token;
  const r = await fetch(path, { method: 'POST', headers, body: body ? JSON.stringify(body) : undefined });
  let data = {};
  try { data = await r.json(); } catch {}
  if (!r.ok) throw new Error(data.error || 'errore ' + r.status);
  return data;
}
function sendWs(obj) {
  if (ws && ws.readyState === WebSocket.OPEN) { ws.send(JSON.stringify(obj)); return true; }
  return false;
}

/* ---------- stato della chat ---------- */
let channels = [];                       // [{id, name}]
let current = Number(lsGet(CHAN_KEY)) || null;
let roles = [];                          // dal più alto al più basso; 'everyone' (id 1) è l'ultimo
let members = new Map();                 // id -> {id, username, display_name, bio, accent, avatar, is_admin, roles, perms}
let online = new Set();                  // id degli utenti connessi
const store = new Map();                 // id canale -> Map(id messaggio -> messaggio)
const loaded = new Set();                // canali di cui abbiamo già la cronologia
const unread = new Set();                // canali con messaggi non letti
const deletedIds = new Set();
// messaggi privati
const dmStore = new Map();               // id utente -> Map(id -> messaggio)
const dmLoaded = new Set();
const dmDeleted = new Set();
let dmConvs = new Map();                 // id utente -> {user, last_id, last_ts, unread}
let tv = 'chan';                         // vista di testo: 'chan' o 'dm'
let dmPeer = null;
let inStage = false;                     // sto guardando la stanza vocale
let editing = null;                      // {dm, id, draft}

const P = { SEND: 1, VOICE: 2, MSGS: 4, CHANNELS: 8, ROLES: 16, INVITES: 32, ADMIN: 64 };   // come src/perms.rs
const PERMS = [
  [P.SEND, 'Scrivere messaggi', 'Scrivere e modificare i propri messaggi'],
  [P.VOICE, 'Canale vocale', 'Entrare nel canale vocale'],
  [P.MSGS, 'Gestire i messaggi', 'Eliminare i messaggi degli altri'],
  [P.CHANNELS, 'Gestire i canali', 'Creare, rinominare ed eliminare i canali'],
  [P.ROLES, 'Gestire i ruoli', 'Creare e assegnare ruoli, ma solo quelli più bassi del proprio'],
  [P.INVITES, 'Creare inviti', 'Generare i codici di invito'],
  [P.ADMIN, 'Amministratore', 'Ha tutti i permessi'],
];
const ACCENTS = ['#8b5cf6', '#ec4899', '#f43f5e', '#f97316', '#eab308', '#22c55e', '#06b6d4', '#3b82f6'];

function myPerms() {
  const m = me && members.get(me.id);
  if (m) return m.perms;
  return me ? (me.is_admin ? 127 : 3) : 0;
}
function can(p) { return (myPerms() & p) === p; }
function topPos(m) {
  if (m.is_admin) return Infinity;
  let top = 0;
  for (const r of roles) if (m.roles.includes(r.id)) top = Math.max(top, r.position);
  return top;
}
function outranks(pos) {
  const m = me && members.get(me.id);
  if (!m) return !!(me && me.is_admin);
  return topPos(m) > pos;
}
function roleColor(uid) {
  const m = members.get(uid);
  if (!m) return '';
  for (const r of roles) if (r.color && m.roles.includes(r.id)) return safeColor(r.color);
  return '';
}
function dname(uid, fallback) {
  const m = members.get(uid);
  return (m && (m.display_name || m.username)) || fallback || 'utente';
}
function accentOf(uid) {
  const m = members.get(uid);
  return (m && safeColor(m.accent)) || ACCENTS[Math.abs(uid || 0) % ACCENTS.length];
}

/* ---------- avatar ---------- */
const avCache = new Map();   // 'id:versione' -> Promise<url|null>
function avUrl(id, v) {
  const k = id + ':' + v;
  if (!avCache.has(k)) {
    avCache.set(k, fetch(`/api/avatar/${id}?v=${v}`, { headers: { Authorization: 'Bearer ' + token } })
      .then((r) => (r.ok ? r.blob() : null))
      .then((b) => (b ? URL.createObjectURL(b) : null))
      .catch(() => null));
  }
  return avCache.get(k);
}
function paintAvatar(node, uid, name, ver, accent) {
  node.style.background = accent || accentOf(uid);
  node.textContent = (name || '?').trim().slice(0, 1);
  if (ver > 0) {
    avUrl(uid, ver).then((u) => {
      if (u) { node.style.backgroundImage = `url("${u}")`; node.textContent = ''; }
    });
  }
}
function avatar(uid, size = 36, opts = {}) {
  const m = members.get(uid);
  const n = el('span', { className: 'av' });
  n.style.setProperty('--s', size + 'px');
  n.dataset.uid = uid;
  paintAvatar(n, uid, dname(uid), m ? m.avatar : 0);
  if (opts.status) n.append(el('i', { className: 'st' + (online.has(uid) ? ' on' : '') }));
  if (opts.speaking && isSpeaking(uid)) n.classList.add('speaking');
  return n;
}

/* ================= accesso ================= */
function setMode(m) {
  mode = m;
  $('tabLogin').classList.toggle('on', m === 'login');
  $('tabRegister').classList.toggle('on', m === 'register');
  $('invRow').hidden = m !== 'register';
  $('authSubmit').textContent = m === 'login' ? 'Accedi' : 'Crea account';
  $('p').autocomplete = m === 'login' ? 'current-password' : 'new-password';
  $('authErr').textContent = '';
}
$('tabLogin').onclick = () => setMode('login');
$('tabRegister').onclick = () => setMode('register');
$('authForm').onsubmit = async (e) => {
  e.preventDefault();
  $('authErr').textContent = '';
  const body = { username: $('u').value, password: $('p').value };
  if (mode === 'register') body.invite = $('inv').value;
  try {
    const d = await api('/api/' + mode, body);
    token = d.token;
    lsSet(TOKEN_KEY, token);
    $('p').value = '';
    start();
  } catch (err) {
    $('authErr').textContent = err.message;
  }
};

/* ================= connessione ================= */
function setStatus(kind, text) {
  $('conn').className = 'conn ' + kind;
  $('conn').title = text;
  $('dockSub').textContent = kind === 'ok' ? (voice.active ? 'In chiamata' : 'Online') : text;
}

function handle(m) {
  switch (m.t) {
    case 'error':
      endSession();
      $('authErr').textContent = 'Sessione scaduta: accedi di nuovo.';
      break;
    case 'ready':
      me = m.user;
      if (Array.isArray(m.ice)) voice.ice = m.ice;
      setStatus('ok', 'Connesso');
      resetChat();
      renderAll();
      break;
    case 'channels': onChannels(m.channels); break;
    case 'directory': onDirectory(m); break;
    case 'presence': online = new Set(m.online); renderPresence(); break;
    case 'history':
      m.messages.forEach(putMsg);
      if (tv === 'chan' && m.channel === current) renderLog(true);
      break;
    case 'message': {
      const x = m.message;
      putMsg(x);
      if (tv === 'chan' && x.channel === current && !inStage) renderLog();
      else if (me && x.user.id !== me.id) { unread.add(x.channel); renderChannels(); }
      break;
    }
    case 'message_edited':
      putMsg(m.message);
      if (tv === 'chan' && m.message.channel === current) renderLog();
      break;
    case 'message_deleted':
      deletedIds.add(m.id);
      msgsOf(m.channel).delete(m.id);
      if (editing && !editing.dm && editing.id === m.id) editing = null;
      if (tv === 'chan' && m.channel === current) renderLog();
      break;
    case 'dm_summary': onDmSummary(m.conversations); break;
    case 'dm_history':
      m.messages.forEach(putDm);
      if (tv === 'dm' && m.with === dmPeer) renderLog(true);
      break;
    case 'dm_message': onDmMessage(m.message); break;
    case 'dm_edited':
      putDm(m.message);
      if (tv === 'dm' && dmPeerOf(m.message) === dmPeer) renderLog();
      break;
    case 'dm_deleted': {
      const peer = m.from === me.id ? m.to : m.from;
      dmDeleted.add(m.id);
      dmOf(peer).delete(m.id);
      if (editing && editing.dm && editing.id === m.id) editing = null;
      if (tv === 'dm' && peer === dmPeer) renderLog();
      break;
    }
    case 'notice':
      toast(m.error);
      renderRolesDialog();
      break;
    case 'voice_state': onVoiceState(m); break;
    case 'voice_channels': voice.vchannels = m.channels; renderVoiceList(); break;
    case 'voice_joined': onVoiceJoined(m); break;
    case 'voice_error': onVoiceError(m); break;
    case 'voice_kicked':
      note(m.reason || 'Sei stato disconnesso dal canale vocale.');
      leaveVoice(false);
      break;
    case 'signal': onSignal(m); break;
  }
}

function connect() {
  clearTimeout(retry);
  const proto = location.protocol === 'https:' ? 'wss' : 'ws';
  ws = new WebSocket(`${proto}://${location.host}/ws`);
  setStatus('', 'Connessione…');
  voice.lastV = -1;
  ws.onopen = () => ws.send(JSON.stringify({ t: 'auth', token }));
  ws.onmessage = (e) => handle(JSON.parse(e.data));
  ws.onclose = () => {
    if (voice.active || voice.joining) leaveVoice(false);
    voice.members.clear();
    online = new Set();
    renderAll();
    if (!token) return;
    setStatus('bad', 'Riconnessione…');
    retry = setTimeout(connect, 2000);
  };
}
function start() { show('app'); connect(); }
function endSession() {
  leaveVoice(true);
  token = null;
  me = null;
  closeAllDialogs();
  resetChat();
  lsDel(TOKEN_KEY);
  clearTimeout(retry);
  if (ws) ws.close();
  show('auth');
}
function closeAllDialogs() {
  document.querySelectorAll('dialog[open]').forEach((d) => d.close());
  closePop();
}

/* ================= messaggi di canale ================= */
function msgsOf(ch) {
  let s = store.get(ch);
  if (!s) store.set(ch, (s = new Map()));
  return s;
}
function putMsg(m) {
  if (deletedIds.has(m.id)) return;
  const s = msgsOf(m.channel);
  const old = s.get(m.id);
  if (!old || (m.edited || 0) >= (old.edited || 0)) s.set(m.id, m);
}
function dmOf(peer) {
  let s = dmStore.get(peer);
  if (!s) dmStore.set(peer, (s = new Map()));
  return s;
}
function dmPeerOf(m) { return m.from === me.id ? m.to : m.from; }
function putDm(m) {
  if (dmDeleted.has(m.id)) return;
  const s = dmOf(dmPeerOf(m));
  const old = s.get(m.id);
  if (!old || (m.edited || 0) >= (old.edited || 0)) s.set(m.id, m);
}

function viewMsgs() {
  if (tv === 'dm' && dmPeer !== null) {
    return [...dmOf(dmPeer).values()].sort((a, b) => a.id - b.id).slice(-300)
      .map((m) => ({ id: m.id, uid: m.from, text: m.text, ts: m.ts, edited: m.edited, dm: true }));
  }
  if (tv === 'chan' && current !== null) {
    return [...msgsOf(current).values()].sort((a, b) => a.id - b.id).slice(-300)
      .map((m) => ({ id: m.id, uid: m.user.id, text: m.text, ts: m.ts, edited: m.edited, dm: false, name: m.user.username }));
  }
  return [];
}
function findMsg(dm, id) {
  const sets = dm ? dmStore.values() : store.values();
  for (const s of sets) if (s.has(id)) return s.get(id);
  return null;
}

function msgRow(m, prev) {
  const cont = prev && prev.uid === m.uid && m.ts - prev.ts < 300 && sameDay(m.ts, prev.ts);
  const row = el('div', { className: 'msg ' + (cont ? 'cont' : 'first') });
  const gut = el('div', { className: 'gut' });
  const col = el('div', { className: 'col' });
  if (cont) {
    gut.textContent = fmtTime(m.ts);
  } else {
    const who = el('b', { textContent: dname(m.uid, m.name), onclick: (e) => showProfile(m.uid, e) });
    const c = roleColor(m.uid);
    if (c) who.style.color = c;
    gut.append(avatar(m.uid, 40));
    gut.firstChild.style.cursor = 'pointer';
    gut.firstChild.onclick = (e) => showProfile(m.uid, e);
    col.append(el('div', { className: 'hd' }, who, el('time', { textContent: fmtTime(m.ts), title: new Date(m.ts * 1000).toLocaleString() })));
  }
  row.append(gut, col);
  if (editing && editing.dm === m.dm && editing.id === m.id) { col.append(editorEl()); return row; }
  const tx = el('div', { className: 'tx', textContent: m.text });
  if (m.edited) tx.append(el('small', { className: 'edited', textContent: '(modificato)', title: new Date(m.edited * 1000).toLocaleString() }));
  col.append(tx);
  const mine = me && m.uid === me.id;
  const canEdit = mine && can(P.SEND);
  const canDel = mine || (!m.dm && can(P.MSGS));
  if (canEdit || canDel) {
    const acts = el('div', { className: 'acts' });
    if (canEdit) acts.append(iconBtn('pencil', 'Modifica', () => startEdit(m)));
    if (canDel) acts.append(iconBtn('trash', 'Elimina', () => deleteMsg(m), 'del'));
    row.append(acts);
  }
  return row;
}

function editorEl() {
  const input = el('input', { value: editing.draft, maxLength: 2000, autocomplete: 'off' });
  input.oninput = () => { editing.draft = input.value; };
  input.onkeydown = (e) => {
    if (e.key === 'Enter') { e.preventDefault(); saveEdit(); }
    else if (e.key === 'Escape') cancelEdit();
  };
  return el('div', { className: 'editor' }, input,
    el('button', { type: 'button', className: 'btn', textContent: 'Salva', onclick: saveEdit }),
    el('button', { type: 'button', className: 'btn ghost', textContent: 'Annulla', onclick: cancelEdit }));
}

function renderLog(toBottom, focusEditor) {
  const log = $('log');
  const stick = toBottom || log.scrollHeight - log.scrollTop - log.clientHeight < 80;
  const hadFocus = log.contains(document.activeElement);
  const list = viewMsgs();
  const kids = [];
  if (!list.length) {
    const who = tv === 'dm' ? dname(dmPeer) : '';
    const ch = channels.find((c) => c.id === current);
    kids.push(el('div', { className: 'chat-empty' },
      el('h3', { textContent: tv === 'dm' ? 'Conversazione con ' + who : ch ? 'Benvenuto in #' + ch.name : 'Nessun canale' }),
      el('div', { textContent: tv === 'dm' ? 'Solo voi due potete leggere questi messaggi.' : 'Ancora nessun messaggio. Scrivi il primo.' })));
  }
  list.forEach((m, i) => kids.push(msgRow(m, list[i - 1])));
  log.replaceChildren(...kids);
  if (stick) log.scrollTop = log.scrollHeight;
  if (editing && (focusEditor || hadFocus)) {
    const i = log.querySelector('.editor input');
    if (i) { i.focus(); i.setSelectionRange(i.value.length, i.value.length); }
  }
}

function startEdit(m) { editing = { dm: m.dm, id: m.id, draft: m.text }; renderLog(false, true); }
function cancelEdit() { editing = null; renderLog(); }
function saveEdit() {
  if (!editing) return;
  const text = editing.draft.trim();
  if (!text) { toast('Il messaggio non può essere vuoto: per toglierlo, eliminalo.'); return; }
  const m = findMsg(editing.dm, editing.id);
  if (m && text !== m.text) sendWs({ t: editing.dm ? 'dm_edit' : 'edit', id: editing.id, text });
  editing = null;
  renderLog();
}
async function deleteMsg(m) {
  if (await ask({ title: 'Eliminare il messaggio?', text: 'Non si può annullare.', ok: 'Elimina', danger: true })) {
    sendWs({ t: m.dm ? 'dm_delete' : 'delete', id: m.id });
  }
}

/* ---------- composer ---------- */
function renderComposer() {
  const ch = channels.find((c) => c.id === current);
  const target = tv === 'dm' ? dmPeer !== null : !!ch;
  const ok = can(P.SEND) && target;
  $('text').disabled = !ok;
  $('sendBtn').disabled = !ok;
  $('text').placeholder = !can(P.SEND) ? 'Non hai il permesso di scrivere'
    : tv === 'dm' && dmPeer !== null ? 'Scrivi a ' + dname(dmPeer)
    : ch ? 'Scrivi in #' + ch.name : 'Scrivi un messaggio';
}
function autoGrow() {
  const t = $('text');
  t.style.height = 'auto';
  t.style.height = Math.min(t.scrollHeight, 160) + 'px';
}
$('text').oninput = autoGrow;
$('text').onkeydown = (e) => {
  if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); $('form').requestSubmit(); }
};
$('form').onsubmit = (e) => {
  e.preventDefault();
  const text = $('text').value.trim();
  if (!text || !can(P.SEND)) return;
  const msg = tv === 'dm'
    ? (dmPeer !== null && { t: 'dm_send', to: dmPeer, text })
    : (current !== null && { t: 'send', channel: current, text });
  if (msg && sendWs(msg)) { $('text').value = ''; autoGrow(); }
};

/* ---------- vista: canali, DM, stanza vocale ---------- */
function closeNav() { document.body.classList.remove('nav-open'); }

function selectChannel(id) {
  tv = 'chan';
  inStage = false;
  current = id;
  lsSet(CHAN_KEY, String(id));
  unread.delete(id);
  editing = null;
  if (!loaded.has(id)) { loaded.add(id); sendWs({ t: 'load_history', channel: id }); }
  closeNav();
  renderSide();
  renderView();
  renderLog(true);
  if (matchMedia('(hover: hover)').matches) $('text').focus();
}

function openDm(peer) {
  if (!me || peer === me.id) return;
  tv = 'dm';
  inStage = false;
  dmPeer = peer;
  editing = null;
  if (!dmLoaded.has(peer)) { dmLoaded.add(peer); sendWs({ t: 'dm_history', with: peer }); }
  markRead(peer);
  closeNav();
  closePop();
  renderSide();
  renderView();
  renderLog(true);
  if (matchMedia('(hover: hover)').matches) $('text').focus();
}

function showStage() {
  if (!(voice.active || voice.joining)) return;
  inStage = true;
  closeNav();
  renderView();
  renderTiles();
}

function renderView() {
  const stage = inStage && (voice.active || voice.joining);
  if (!stage) inStage = false;
  $('stage').hidden = !stage;
  $('log').hidden = stage;
  $('form').hidden = stage;
  renderTitle();
  renderComposer();
  renderSide();
}

function renderTitle() {
  const t = $('title');
  t.replaceChildren();
  if (inStage && voice.channel !== null || inStage && voice.joining) {
    const ch = voice.vchannels.find((c) => c.id === voice.channel);
    const n = [...voice.members.values()].filter((x) => x.channel === voice.channel).length;
    t.innerHTML = ico('volume');
    t.append(ch ? ch.name : 'Voce', el('small', { textContent: n ? n + ' in chiamata' : '' }));
  } else if (tv === 'dm' && dmPeer !== null) {
    t.append(avatar(dmPeer, 26, { status: true }), dname(dmPeer));
  } else {
    const ch = channels.find((c) => c.id === current);
    t.innerHTML = ico('hash');
    t.append(ch ? ch.name : '');
  }
}

/* ---------- barra laterale ---------- */
function renderChannels() {
  const ul = $('chanList');
  ul.replaceChildren();
  for (const ch of channels) {
    const li = el('li');
    const b = el('button', { type: 'button', className: 'row-item', onclick: () => selectChannel(ch.id) });
    b.innerHTML = ico('hash');
    b.append(el('span', { className: 'nm', textContent: ch.name }));
    if (unread.has(ch.id)) b.append(el('i', { className: 'dotu' }));
    li.append(b);
    li.classList.toggle('on', tv === 'chan' && !inStage && ch.id === current);
    li.classList.toggle('unread', unread.has(ch.id));
    if (can(P.CHANNELS)) {
      li.append(el('span', { className: 'tools' },
        iconBtn('pencil', 'Rinomina', () => renameChannel(ch), 'sm'),
        iconBtn('trash', 'Elimina', () => deleteChannel(ch), 'sm')));
    }
    ul.append(li);
  }
  $('addChan').hidden = !can(P.CHANNELS);
  $('rolesBtn').hidden = !can(P.ROLES);
  $('inviteBtn').hidden = !can(P.INVITES);
}

async function newChannel(voiceCh) {
  const name = await ask({ title: voiceCh ? 'Nuovo canale vocale' : 'Nuovo canale', text: 'Lettere, numeri, - e _.', input: true, ok: 'Crea' });
  if (name && name.trim()) sendWs({ t: voiceCh ? 'voice_channel_create' : 'channel_create', name });
}
async function renameChannel(ch, voiceCh) {
  const name = await ask({ title: 'Rinomina il canale', input: true, value: ch.name, ok: 'Salva' });
  if (name && name.trim() && name.trim() !== ch.name) sendWs({ t: voiceCh ? 'voice_channel_rename' : 'channel_rename', id: ch.id, name });
}
async function deleteChannel(ch, voiceCh) {
  const text = voiceCh ? 'Chi è dentro verrà disconnesso.' : 'Anche tutti i suoi messaggi. Non si può annullare.';
  if (await ask({ title: 'Eliminare ' + (voiceCh ? '' : '#') + ch.name + '?', text, ok: 'Elimina', danger: true })) {
    sendWs({ t: voiceCh ? 'voice_channel_delete' : 'channel_delete', id: ch.id });
  }
}
$('addChan').onclick = () => newChannel(false);
$('addVChan').onclick = () => newChannel(true);

function renderVoiceList() {
  const ul = $('vchanList');
  ul.replaceChildren();
  $('addVChan').hidden = !can(P.CHANNELS);
  if (!voice.vchannels.length) ul.append(el('li', { className: 'empty', textContent: 'Nessun canale vocale' }));
  for (const ch of voice.vchannels) {
    const li = el('li');
    const enter = () => {
      if (!can(P.VOICE)) { toast('Non hai il permesso di entrare nei canali vocali.'); return; }
      if (voice.active && voice.channel === ch.id) showStage(); else joinVoice(ch.id);
    };
    const b = el('button', { type: 'button', className: 'row-item', onclick: enter,
      title: can(P.VOICE) ? 'Entra nel canale' : 'Non hai il permesso di entrare nei canali vocali' });
    b.innerHTML = ico('volume');
    b.append(el('span', { className: 'nm', textContent: ch.name }));
    li.append(b);
    li.classList.toggle('on', voice.active && voice.channel === ch.id);
    if (can(P.CHANNELS)) {
      li.append(el('span', { className: 'tools' },
        iconBtn('pencil', 'Rinomina', () => renameChannel(ch, true), 'sm'),
        iconBtn('trash', 'Elimina', () => deleteChannel(ch, true), 'sm')));
    }
    const mem = el('ul', { className: 'vmem' });
    for (const m of voice.members.values()) {
      if (m.channel !== ch.id) continue;
      const fl = el('span', { className: 'fl' });
      if (m.muted && !m.deaf) fl.insertAdjacentHTML('beforeend', `<span class="bad">${ico('mic-off')}</span>`);
      if (m.deaf) fl.insertAdjacentHTML('beforeend', `<span class="bad">${ico('headphones-off')}</span>`);
      if (m.cam) fl.insertAdjacentHTML('beforeend', ico('video'));
      if (m.screen) fl.insertAdjacentHTML('beforeend', ico('monitor'));
      const mb = el('button', { type: 'button', onclick: (e) => showProfile(m.id, e) },
        avatar(m.id, 22, { speaking: true }),
        el('span', { className: 'nm', textContent: dname(m.id, m.username) }), fl);
      mem.append(el('li', {}, mb));
    }
    if (mem.children.length) li.append(mem);
    ul.append(li);
  }
}

function renderDms() {
  const ul = $('dmList');
  ul.replaceChildren();
  const ids = new Set(dmConvs.keys());
  if (tv === 'dm' && dmPeer !== null) ids.add(dmPeer);
  const list = [...ids].sort((a, b) => ((dmConvs.get(b) || {}).last_ts || 9e15) - ((dmConvs.get(a) || {}).last_ts || 9e15));
  if (!list.length) ul.append(el('li', { className: 'empty', textContent: 'Nessuna conversazione' }));
  for (const id of list) {
    const c = dmConvs.get(id);
    const b = el('button', { type: 'button', className: 'row-item', onclick: () => openDm(id) },
      avatar(id, 24, { status: true }), el('span', { className: 'nm', textContent: dname(id) }));
    if (c && c.unread > 0) b.append(el('span', { className: 'pill', textContent: String(c.unread) }));
    const li = el('li', {}, b);
    li.classList.toggle('on', tv === 'dm' && !inStage && id === dmPeer);
    li.classList.toggle('unread', !!(c && c.unread > 0));
    ul.append(li);
  }
}

function renderDock() {
  if (!me) return;
  const m = members.get(me.id);
  $('dockName').textContent = dname(me.id, me.username);
  const a = $('dockAv');
  a.className = 'av';
  a.style.setProperty('--s', '34px');
  a.style.backgroundImage = '';
  paintAvatar(a, me.id, dname(me.id, me.username), m ? m.avatar : 0);
  a.replaceChildren(...[...a.childNodes]);
  if (!a.querySelector('.st')) a.append(el('i', { className: 'st on' }));
}

function renderSide() {
  renderChannels();
  renderVoiceList();
  renderDms();
  renderDock();
  renderVoiceCard();
}

function renderVoiceCard() {
  const on = voice.active || voice.joining;
  $('vcard').hidden = !on;
  if (!on) return;
  const ch = voice.vchannels.find((c) => c.id === voice.channel);
  $('vcName').textContent = voice.joining && !voice.active ? 'Connessione…' : 'Voce connessa';
  $('vcSub').textContent = ch ? ch.name : '';
}
$('vcard').querySelector('.vc-info').onclick = showStage;

/* ---------- membri e presenza ---------- */
function renderMemberPane() {
  const box = $('memberList');
  box.replaceChildren();
  const all = [...members.values()].sort((a, b) => dname(a.id).localeCompare(dname(b.id)));
  const on = all.filter((m) => online.has(m.id));
  const off = all.filter((m) => !online.has(m.id));
  for (const [label, list] of [['Online', on], ['Offline', off]]) {
    if (!list.length) continue;
    box.append(el('div', { className: 'mgrp', textContent: `${label} — ${list.length}` }));
    for (const m of list) {
      const nm = el('span', { className: 'nm', textContent: dname(m.id) });
      if (m.display_name) nm.append(el('small', { textContent: m.username }));
      const c = roleColor(m.id);
      if (c) nm.style.color = c;
      const b = el('button', { type: 'button', className: 'mem' + (online.has(m.id) ? '' : ' offline'), onclick: (e) => showProfile(m.id, e) },
        avatar(m.id, 32, { status: true }), nm);
      box.append(b);
    }
  }
}
function renderPresence() { renderMemberPane(); renderDms(); renderTitle(); }

function toggleMembers() {
  const on = !$('app').classList.contains('members');
  $('app').classList.toggle('members', on);
  document.body.classList.toggle('mem-open', on);
  lsSet('hstle-members', on ? '1' : '0');
}
$('membersBtn').onclick = toggleMembers;
$('menuBtn').onclick = () => document.body.classList.toggle('nav-open');
$('scrim').onclick = () => {
  closeNav();
  if (document.body.classList.contains('mem-open')) toggleMembers();
};
{
  const stored = lsGet('hstle-members');
  const wide = window.innerWidth > 1100;
  $('app').classList.toggle('members', stored === null ? wide : stored === '1' && wide);
}

/* ---------- elenco canali, directory, DM dal server ---------- */
function onChannels(list) {
  channels = list;
  const ids = new Set(list.map((c) => c.id));
  for (const id of [...store.keys()]) if (!ids.has(id)) store.delete(id);
  for (const set of [loaded, unread]) for (const id of [...set]) if (!ids.has(id)) set.delete(id);
  const target = ids.has(current) ? current : (list.length ? list[0].id : null);
  if (target !== null && (target !== current || !loaded.has(target))) {
    if (tv === 'chan' && !inStage) selectChannel(target);
    else { current = target; if (!loaded.has(target)) { loaded.add(target); sendWs({ t: 'load_history', channel: target }); } renderSide(); }
  } else {
    renderSide();
    renderComposer();
    renderTitle();
  }
}

function onDirectory(d) {
  roles = d.roles;
  members = new Map(d.members.map((x) => [x.id, x]));
  if (me && members.has(me.id)) me.is_admin = members.get(me.id).is_admin;
  renderSide();
  renderMemberPane();
  renderTitle();
  renderComposer();
  renderLog();
  renderTiles();
  renderControls();
  renderRolesDialog();
  refreshSettingsPreview();
}

function onDmSummary(list) {
  dmConvs = new Map(list.map((c) => [c.user, c]));
  renderDms();
  const c = dmConvs.get(dmPeer);
  if (tv === 'dm' && !inStage && c && c.unread > 0) markRead(dmPeer);
}

function onDmMessage(m) {
  const peer = dmPeerOf(m);
  putDm(m);
  const incoming = m.from !== me.id;
  const viewing = tv === 'dm' && peer === dmPeer && !inStage;
  const c = dmConvs.get(peer) || { user: peer, last_id: 0, last_ts: 0, unread: 0 };
  c.last_id = m.id; c.last_ts = m.ts;
  dmConvs.set(peer, c);
  if (viewing) {
    renderLog();
    if (incoming) markRead(peer);
  } else if (incoming) {
    c.unread = (c.unread || 0) + 1;
    toast(dname(peer) + ': ' + (m.text.length > 80 ? m.text.slice(0, 80) + '…' : m.text));
  }
  renderDms();
}

function markRead(peer) {
  const c = dmConvs.get(peer);
  if (c) c.unread = 0;
  sendWs({ t: 'dm_read', with: peer });
  renderDms();
}

function resetChat() {
  channels = []; roles = []; members = new Map(); online = new Set();
  store.clear(); loaded.clear(); unread.clear(); deletedIds.clear();
  dmStore.clear(); dmLoaded.clear(); dmDeleted.clear(); dmConvs = new Map();
  editing = null;
  dmPeer = null;
  if (tv === 'dm') tv = 'chan';
  renderLog(true);
  renderSide();
}

/* ================= profilo: popover ================= */
function closePop() { $('pop').hidden = true; $('pop').replaceChildren(); }
document.addEventListener('pointerdown', (e) => {
  const p = $('pop');
  if (!p.hidden && !p.contains(e.target) && !e.target.closest('[data-keep-pop]')) closePop();
}, true);
document.addEventListener('keydown', (e) => { if (e.key === 'Escape') closePop(); });

function bannerFor(accent) {
  const b = el('div', { className: 'banner' });
  b.style.background = `linear-gradient(135deg, ${accent}, ${accent}55 70%, #0d0916)`;
  return b;
}

function showProfile(uid, ev, screenAudio) {
  const m = members.get(uid);
  if (!m) return;
  const pop = $('pop');
  pop.replaceChildren();
  const name = dname(uid);
  const body = el('div', { className: 'body' });
  const av = avatar(uid, 68, { status: true });
  body.append(av, el('h3', { textContent: name }), el('div', { className: 'un', textContent: '@' + m.username }));
  if (m.bio) body.append(el('div', { className: 'bio', textContent: m.bio }));
  const rs = roles.filter((r) => r.id !== 1 && m.roles.includes(r.id));
  if (m.is_admin || rs.length) {
    const chips = el('div', { className: 'chips' });
    if (m.is_admin) chips.append(el('span', { className: 'chip', textContent: 'Proprietario' }));
    for (const r of rs) {
      const d = el('span', { className: 'dot' });
      if (safeColor(r.color)) d.style.background = r.color;
      chips.append(el('span', { className: 'chip' }, d, r.name));
    }
    body.append(chips);
  }
  if (me && uid !== me.id) {
    if (voice.active && voice.members.has(uid)) body.append(volumeBlock(uid));
    body.append(el('button', { className: 'btn', onclick: () => openDm(uid) }, 'Scrivi un messaggio'));
  } else if (me) {
    body.append(el('button', { className: 'btn ghost', onclick: () => { closePop(); openSettings('profile'); } }, 'Modifica il profilo'));
  }
  if (screenAudio) {
    pop.append(el('div', { className: 'body', style: 'margin-top:0;padding-top:16px' }, volumeBlock('s' + uid, 'Audio dello schermo di ' + dname(uid))));
  } else pop.append(bannerFor(accentOf(uid)), body);
  pop.hidden = false;
  // posizione vicino al punto cliccato, senza uscire dallo schermo
  const r = (ev && ev.currentTarget && ev.currentTarget.getBoundingClientRect) ? ev.currentTarget.getBoundingClientRect() : { left: 20, top: 80, right: 20, bottom: 80 };
  const w = pop.offsetWidth, h = pop.offsetHeight;
  let x = r.right + 10;
  if (x + w > innerWidth - 10) x = Math.max(10, r.left - w - 10);
  if (x < 10) x = Math.min(10, innerWidth - w - 10);
  let y = Math.min(Math.max(10, r.top - 20), innerHeight - h - 10);
  pop.style.left = Math.max(10, x) + 'px';
  pop.style.top = Math.max(10, y) + 'px';
  if (ev && ev.stopPropagation) ev.stopPropagation();
}

/* ================= volume per utente ================= */
let vols = {};
try { vols = JSON.parse(lsGet('hstle-vol') || '{}') || {}; } catch { vols = {}; }
function volOf(uid) { const v = vols[uid] || {}; return { v: typeof v.v === 'number' ? v.v : 100, m: !!v.m }; }
function setVol(uid, patch) {
  vols[uid] = { ...volOf(uid), ...patch };
  lsSet('hstle-vol', JSON.stringify(vols));
  const p = voice.peers.get(typeof uid === 'string' ? Number(uid.slice(1)) : uid);   // 's<id>' = audio della condivisione
  if (p) applyPeerGain(p);
}
function applyPeerGain(peer) {
  const { v, m } = volOf(peer.id);
  const g = m ? 0 : v / 100;
  if (peer.gain) peer.gain.gain.value = g;
  else if (peer.audio) peer.audio.volume = Math.min(1, g);
  const sv = volOf('s' + peer.id);
  const sg = sv.m ? 0 : sv.v / 100;
  if (peer.sa) {
    if (peer.sa.gain) peer.sa.gain.gain.value = sg;
    else peer.sa.audio.volume = Math.min(1, sg);
  }
}
function volumeBlock(uid, label) {
  const { v, m } = volOf(uid);
  const out = el('span', { textContent: m ? 'Muto per te' : v + '%' });
  const range = el('input', { type: 'range', min: 0, max: 200, step: 5, value: v });
  range.setAttribute('aria-label', label || 'Volume di ' + dname(uid));
  const paint = () => range.style.setProperty('--p', (range.value / 200 * 100) + '%');
  paint();
  range.oninput = () => {
    paint();
    setVol(uid, { v: Number(range.value), m: false });
    out.textContent = range.value + '%';
    mbtn.classList.remove('on');
  };
  const mbtn = el('button', { type: 'button', className: 'btn ghost' + (m ? ' on' : ''), textContent: m ? 'Riattiva' : 'Muta per me' });
  mbtn.onclick = () => {
    const now = !volOf(uid).m;
    setVol(uid, { m: now });
    mbtn.textContent = now ? 'Riattiva' : 'Muta per me';
    out.textContent = now ? 'Muto per te' : volOf(uid).v + '%';
  };
  return el('div', { className: 'vol' },
    el('div', { className: 'hd' }, el('span', { textContent: label || 'Volume (solo per te)' }), out), range, mbtn);
}

/* ================= impostazioni ================= */
const sdlg = $('settingsDlg');
let draft = { name: '', bio: '', accent: ACCENTS[0] };
let ns = lsGet('hstle-ns') !== '0';       // riduzione del rumore: acceso di default

function openSettings(tab) {
  const m = me && members.get(me.id);
  draft = { name: (m && m.display_name) || '', bio: (m && m.bio) || '', accent: (m && safeColor(m.accent)) || accentOf(me.id) };
  $('pfName').value = draft.name;
  $('pfBio').value = draft.bio;
  $('pfErr').textContent = '';
  $('acctName').textContent = me ? me.username : '';
  $('nsToggle').checked = ns;
  renderNsNote();
  settingsTab(tab || 'profile');
  renderSwatches();
  refreshSettingsPreview();
  if (!sdlg.open) sdlg.showModal();
}
function settingsTab(t) {
  for (const [id, key] of [['sTabProfile', 'profile'], ['sTabAudio', 'audio'], ['sTabAccount', 'account']]) $(id).classList.toggle('on', t === key);
  $('sPaneProfile').hidden = t !== 'profile';
  $('sPaneAudio').hidden = t !== 'audio';
  $('sPaneAccount').hidden = t !== 'account';
}
$('sTabProfile').onclick = () => settingsTab('profile');
$('sTabAudio').onclick = () => settingsTab('audio');
$('sTabAccount').onclick = () => settingsTab('account');
$('settingsBtn').onclick = () => openSettings('audio');
$('dockMe').onclick = () => openSettings('profile');
$('setClose').onclick = () => sdlg.close();
sdlg.onclick = (e) => { if (e.target === sdlg) sdlg.close(); };

function renderSwatches() {
  const box = $('pfSwatches');
  box.replaceChildren();
  for (const c of ACCENTS) {
    const b = el('button', { type: 'button', className: 'sw' + (c.toLowerCase() === draft.accent.toLowerCase() ? ' on' : ''), title: c });
    b.style.background = c;
    b.setAttribute('aria-label', 'Colore ' + c);
    b.onclick = () => { draft.accent = c; renderSwatches(); refreshSettingsPreview(); };
    box.append(b);
  }
  const custom = el('input', { type: 'color', value: draft.accent, title: 'Colore personalizzato' });
  custom.setAttribute('aria-label', 'Colore personalizzato');
  custom.oninput = () => { draft.accent = custom.value; refreshSettingsPreview(); };
  custom.onchange = renderSwatches;
  box.append(custom);
}

function refreshSettingsPreview() {
  if (!sdlg.open || !me) return;
  const box = $('pfPreview');
  const m = members.get(me.id);
  const nm = draft.name.trim() || me.username;
  const av = el('span', { className: 'av' });
  av.style.setProperty('--s', '68px');
  paintAvatar(av, me.id, nm, m ? m.avatar : 0, draft.accent);
  const body = el('div', { className: 'body' }, av, el('h3', { textContent: nm }),
    el('div', { className: 'muted', textContent: '@' + me.username }));
  if (draft.bio.trim()) body.append(el('div', { className: 'bio', textContent: draft.bio }));
  box.replaceChildren(bannerFor(draft.accent), body);
  $('pfBioCount').textContent = draft.bio.length + '/190';
}
$('pfName').oninput = (e) => { draft.name = e.target.value; refreshSettingsPreview(); };
$('pfBio').oninput = (e) => { draft.bio = e.target.value; refreshSettingsPreview(); };
$('pfForm').onsubmit = (e) => {
  e.preventDefault();
  $('pfErr').textContent = '';
  if (sendWs({ t: 'profile_update', display_name: draft.name.trim(), bio: draft.bio.trim(), accent: draft.accent })) toast('Profilo salvato.');
  else $('pfErr').textContent = 'Non sei connesso al server.';
};

$('pfUpload').onclick = () => $('pfFile').click();
$('pfFile').onchange = async (e) => {
  const f = e.target.files[0];
  e.target.value = '';
  if (!f) return;
  try {
    const blob = await squareImage(f, 256);
    const r = await fetch('/api/avatar', { method: 'POST', headers: { Authorization: 'Bearer ' + token }, body: blob });
    if (!r.ok) { let d = {}; try { d = await r.json(); } catch {} throw new Error(d.error || 'errore ' + r.status); }
    toast('Immagine aggiornata.');
  } catch (err) {
    $('pfErr').textContent = err.message || 'Immagine non valida.';
  }
};
$('pfRemove').onclick = async () => {
  try { await api('/api/avatar/clear', null, true); toast('Immagine rimossa.'); } catch (err) { $('pfErr').textContent = err.message; }
};

/** Ritaglia al centro e riduce a size×size: l'immagine pesa poche decine di KB. */
async function squareImage(file, size) {
  const bmp = await createImageBitmap(file);
  const c = document.createElement('canvas');
  c.width = c.height = size;
  const g = c.getContext('2d');
  g.fillStyle = '#1a1229';
  g.fillRect(0, 0, size, size);
  const s = Math.min(bmp.width, bmp.height);
  g.drawImage(bmp, (bmp.width - s) / 2, (bmp.height - s) / 2, s, s, 0, 0, size, size);
  bmp.close && bmp.close();
  return new Promise((res, rej) => c.toBlob((b) => (b ? res(b) : rej(new Error('Immagine non valida.'))), 'image/jpeg', 0.88));
}

$('logoutBtn').onclick = async () => {
  try { await api('/api/logout', null, true); } catch {}
  endSession();
};
$('inviteBtn').onclick = async () => {
  try {
    const d = await api('/api/invites', null, true);
    await ask({ title: 'Codice invito', text: 'Manda questo codice alla persona: vale una volta sola.', input: true, value: d.code, readonly: true, ok: 'Copia', copy: true });
  } catch (err) { toast(err.message); }
};
$('newDm').onclick = () => { $('dmSearch').value = ''; renderPeople(); $('dmDlg').showModal(); $('dmSearch').focus(); };
$('dmClose').onclick = () => $('dmDlg').close();
$('dmDlg').onclick = (e) => { if (e.target === $('dmDlg')) $('dmDlg').close(); };
$('dmSearch').oninput = renderPeople;
function renderPeople() {
  const q = $('dmSearch').value.trim().toLowerCase();
  const ul = $('dmPeople');
  ul.replaceChildren();
  const list = [...members.values()].filter((m) => me && m.id !== me.id &&
    (dname(m.id).toLowerCase().includes(q) || m.username.toLowerCase().includes(q)));
  if (!list.length) ul.append(el('li', { className: 'empty', textContent: 'Nessuna persona trovata' }));
  for (const m of list) {
    const b = el('button', { type: 'button', className: 'mem', onclick: () => { $('dmDlg').close(); openDm(m.id); } },
      avatar(m.id, 32, { status: true }), el('span', { className: 'nm', textContent: dname(m.id) }));
    ul.append(el('li', {}, b));
  }
}

/* ================= finestra "Ruoli e membri" ================= */
const dlg = $('rolesDlg');
let rtab = 'roles';
let selRole = null;
let editorKey = null;

$('rolesBtn').onclick = () => { rtab = 'roles'; selRole = null; editorKey = null; dlg.showModal(); renderRolesDialog(); };
$('rolesClose').onclick = () => dlg.close();
dlg.onclick = (e) => { if (e.target === dlg) dlg.close(); };
$('rTabRoles').onclick = () => { rtab = 'roles'; renderRolesDialog(); };
$('rTabMembers').onclick = () => { rtab = 'members'; renderRolesDialog(); };
$('newRole').onclick = () => { selRole = 'new'; renderRolesDialog(); };

function renderRolesDialog() {
  if (!dlg.open) return;
  if (!can(P.ROLES)) { dlg.close(); return; }
  if (selRole !== null && selRole !== 'new' && !roles.some((r) => r.id === selRole)) { selRole = null; editorKey = null; }
  $('rTabRoles').classList.toggle('on', rtab === 'roles');
  $('rTabMembers').classList.toggle('on', rtab === 'members');
  $('rolesPane').hidden = rtab !== 'roles';
  $('membersPane').hidden = rtab !== 'members';
  renderRoleList();
  renderRoleEditor();
  renderUserRoles();
}
function canManageRole(r) { return can(P.ROLES) && outranks(r.position); }

function renderRoleList() {
  const ul = $('roleList');
  ul.replaceChildren();
  for (const r of roles) {
    const dot = el('span', { className: 'dot' });
    if (safeColor(r.color)) dot.style.background = r.color;
    const li = el('li', { onclick: () => { selRole = r.id; renderRolesDialog(); } },
      dot, el('span', { className: 'rname', textContent: r.id === 1 ? '@everyone' : r.name }));
    li.classList.toggle('on', r.id === selRole);
    if (r.id !== 1 && canManageRole(r)) {
      const move = (up) => (e) => { e.stopPropagation(); sendWs({ t: 'role_move', id: r.id, up }); };
      li.append(iconBtn('x', 'Sposta in alto', move(true), 'sm'), iconBtn('x', 'Sposta in basso', move(false), 'sm'));
      const [u, d] = li.querySelectorAll('button');
      u.innerHTML = '▲'; d.innerHTML = '▼';
    }
    ul.append(li);
  }
  $('newRole').hidden = !can(P.ROLES);
}

function renderRoleEditor() {
  const key = String(selRole);
  if (editorKey === key) return;
  editorKey = key;
  const box = $('roleEditor');
  box.replaceChildren();
  const isNew = selRole === 'new';
  const r = isNew ? { id: 0, name: '', color: '', permissions: P.SEND | P.VOICE, position: 0 } : roles.find((x) => x.id === selRole);
  if (!r) { box.append(el('p', { className: 'muted', textContent: 'Scegli un ruolo per vederlo o modificarlo.' })); return; }
  const editable = isNew ? can(P.ROLES) : canManageRole(r);
  const fixed = r.id === 1;
  const name = el('input', { value: fixed ? '@everyone' : r.name, placeholder: 'Nome del ruolo', maxLength: 32, disabled: !editable || fixed });
  const none = el('input', { type: 'checkbox', checked: !safeColor(r.color), disabled: !editable });
  const color = el('input', { type: 'color', value: safeColor(r.color) || '#99aab5', disabled: !editable });
  if (!fixed) {
    box.append(el('div', { className: 'field' }, name),
      el('div', { className: 'field' }, color, el('label', { className: 'chip' }, none, 'Nessun colore')));
  } else {
    box.append(el('p', { className: 'muted', textContent: 'Vale per tutti i membri, anche senza altri ruoli.' }));
  }
  for (const [bit, label, hint] of PERMS) {
    const cb = el('input', { type: 'checkbox', checked: (r.permissions & bit) !== 0, disabled: !editable || (myPerms() & bit) === 0 });
    cb.dataset.bit = String(bit);
    box.append(el('label', { className: 'perm' }, cb, el('span', {}, label, el('small', { textContent: hint }))));
  }
  const row = el('div', { className: 'btnrow' });
  if (editable) {
    row.append(el('button', {
      type: 'button', textContent: isNew ? 'Crea ruolo' : 'Salva',
      onclick: () => {
        let bits = 0;
        for (const cb of box.querySelectorAll('input[data-bit]')) if (cb.checked) bits |= Number(cb.dataset.bit);
        const body = { name: name.value, color: none.checked ? '' : color.value, permissions: bits };
        if (isNew) { sendWs({ t: 'role_create', ...body }); selRole = null; editorKey = null; renderRolesDialog(); }
        else { sendWs({ t: 'role_edit', id: r.id, ...body }); editorKey = null; }
      },
    }));
    if (!isNew && !fixed) {
      row.append(el('button', {
        type: 'button', className: 'danger2', textContent: 'Elimina',
        onclick: async () => {
          if (!(await ask({ title: 'Eliminare il ruolo "' + r.name + '"?', text: 'Lo perderanno tutti i membri che lo hanno.', ok: 'Elimina', danger: true }))) return;
          sendWs({ t: 'role_delete', id: r.id });
          selRole = null; editorKey = null;
        },
      }));
    }
  } else {
    row.append(el('p', { className: 'muted', textContent: 'Non puoi modificare questo ruolo: è allo stesso livello del tuo o più in alto.' }));
  }
  box.append(row);
}

function renderUserRoles() {
  const box = $('membersList');
  box.replaceChildren();
  const assignable = roles.filter((r) => r.id !== 1);
  for (const mem of members.values()) {
    const title = dname(mem.id) + ' (@' + mem.username + ')' + (me && mem.id === me.id ? ' — tu' : '') + (mem.is_admin ? ' — proprietario' : '');
    const chips = el('div', { className: 'chips' });
    const mayEdit = can(P.ROLES) && !mem.is_admin && ((me && mem.id === me.id) || outranks(topPos(mem)));
    for (const r of assignable) {
      const cb = el('input', { type: 'checkbox', checked: mem.roles.includes(r.id), disabled: !mayEdit || !outranks(r.position) });
      cb.onchange = () => {
        const next = new Set(mem.roles);
        if (cb.checked) next.add(r.id); else next.delete(r.id);
        sendWs({ t: 'member_roles', user: mem.id, roles: [...next] });
      };
      const dot = el('span', { className: 'dot' });
      if (safeColor(r.color)) dot.style.background = r.color;
      chips.append(el('label', { className: 'chip' }, cb, dot, r.name));
    }
    if (!assignable.length) chips.append(el('span', { className: 'muted', textContent: 'Nessun ruolo creato.' }));
    box.append(el('div', { className: 'mrow' }, el('b', { textContent: title }), chips));
  }
}

/* ================= voce e video (WebRTC mesh) =================
   Ogni partecipante si collega direttamente a tutti gli altri. Il server inoltra
   solo la segnalazione (SDP e ICE). Chi entra per ultimo manda l'offerta a chi
   c'era già. Ogni connessione ha 3 corsie fisse: microfono, camera, schermo. */
const voice = {
  active: false, joining: false,
  channel: null, vchannels: [],
  joinV: 0, lastV: -1,
  members: new Map(),     // id -> {id, username, channel, muted, deaf, cam, screen}
  peers: new Map(),
  mic: null, cam: null, scr: null, scrAudio: null,
  camStream: null, scrStream: null,
  muted: false, deaf: false,     // scelte della persona: valgono anche prima di entrare
  ice: [{ urls: ['stun:stun.l.google.com:19302'] }],
};
const tiles = new Map();      // 'cam:ID' / 'scr:ID' -> elemento della griglia
const watchers = new Map();
let pinKey = null;
let actx = null, master = null;
let signalChain = Promise.resolve();

function note(text) {
  $('vnote').textContent = text || '';
  if (text && $('stage').hidden) toast(text);
}
function ensureAudioCtx() {
  try {
    if (!actx) {
      actx = new (window.AudioContext || window.webkitAudioContext)();
      master = actx.createGain();
      master.connect(actx.destination);
    }
    if (actx.state === 'suspended') actx.resume();
  } catch {}
}
['pointerdown', 'keydown'].forEach((ev) => document.addEventListener(ev, () => { if (actx && actx.state === 'suspended') actx.resume(); }, true));

const isMuted = () => voice.muted || voice.deaf;

/* ---------- pulsanti ---------- */
function renderControls() {
  const v = voice;
  const states = {
    mute: { icon: isMuted() ? 'mic-off' : 'mic', off: isMuted(), on: false, title: isMuted() ? 'Riattiva il microfono' : 'Disattiva il microfono' },
    deaf: { icon: v.deaf ? 'headphones-off' : 'headphones', off: v.deaf, on: false, title: v.deaf ? 'Riattiva l\'audio' : 'Disattiva l\'audio (cuffie)' },
    cam: { icon: v.cam ? 'video' : 'video-off', off: false, on: !!v.cam, title: v.cam ? 'Spegni la camera' : 'Accendi la camera' },
    screen: { icon: 'monitor', off: false, on: !!v.scr, title: v.scr ? 'Smetti di condividere lo schermo' : 'Condividi lo schermo' },
    ns: { icon: 'wave', off: false, on: ns, title: ns ? 'Riduzione del rumore: attiva' : 'Riduzione del rumore: spenta' },
  };
  document.querySelectorAll('[data-act]').forEach((b) => {
    const s = states[b.dataset.act];
    if (!s) return;
    b.innerHTML = ico(s.icon);
    b.title = s.title;
    b.setAttribute('aria-label', s.title);
    b.setAttribute('aria-pressed', String(s.on || s.off));
    b.classList.toggle('off', s.off);
    b.classList.toggle('on', s.on);
  });
  document.querySelectorAll('[data-act=cam],[data-act=screen]').forEach((b) => { b.disabled = !v.active; });
  document.querySelectorAll('[data-act=screen]').forEach((b) => { b.hidden = !(navigator.mediaDevices && navigator.mediaDevices.getDisplayMedia); });
  $('nsToggle').checked = ns;
}
document.addEventListener('click', (e) => {
  const b = e.target.closest('[data-act]');
  if (!b || b.disabled) return;
  ({ mute: toggleMute, deaf: toggleDeaf, cam: toggleCam, screen: toggleScreen, ns: () => setNs(!ns), leave: () => leaveVoice(true) })[b.dataset.act]();
});

/* ---------- muto e cuffie ---------- */
function applyAudioState() {
  if (voice.mic) voice.mic.enabled = !isMuted();
  if (master) master.gain.value = voice.deaf ? 0 : 1;
  for (const p of voice.peers.values()) { if (p.audio) p.audio.muted = voice.deaf || !!p.gain; if (p.sa) p.sa.audio.muted = voice.deaf || !!p.sa.gain; }   // senza WebAudio il volume passa dall'elemento
  if (voice.active) sendMeta();
  renderAll();
}
function toggleMute() {
  if (voice.deaf) { voice.deaf = false; voice.muted = false; }       // riattivare il microfono riattiva anche l'audio
  else voice.muted = !voice.muted;
  applyAudioState();
}
function toggleDeaf() {
  voice.deaf = !voice.deaf;
  applyAudioState();
}

/* ---------- riduzione del rumore ---------- */
function micConstraints() { return { echoCancellation: true, noiseSuppression: ns, autoGainControl: true }; }
function renderNsNote() {
  const sup = navigator.mediaDevices && navigator.mediaDevices.getSupportedConstraints && navigator.mediaDevices.getSupportedConstraints().noiseSuppression;
  $('nsNote').textContent = sup === false
    ? 'Questo browser non supporta la riduzione del rumore: l\'interruttore non avrà effetto.'
    : 'È il filtro integrato nel browser: non usa il server e non pesa sulla sua memoria.';
}
async function setNs(on) {
  ns = on;
  lsSet('hstle-ns', on ? '1' : '0');
  $('nsToggle').checked = on;
  renderControls();
  if (!voice.mic) return;
  const old = voice.mic;
  try {
    await old.applyConstraints(micConstraints());
    if (old.getSettings().noiseSuppression === on) return;
  } catch {}
  try {                                     // il browser non cambia al volo: riapro il microfono
    const s = await navigator.mediaDevices.getUserMedia({ audio: micConstraints() });
    const t = s.getAudioTracks()[0];
    t.enabled = !isMuted();
    voice.mic = t;
    old.stop();
    watchers.delete('me');
    if (voice.active) watchSpeaking('me', new MediaStream([t]));
    for (const p of voice.peers.values()) attachLocalTracks(p);
  } catch { note('Non riesco a cambiare la riduzione del rumore.'); }
}
$('nsToggle').onchange = (e) => setNs(e.target.checked);

/* ---------- griglia video ---------- */
function makeTile(key) {
  const t = el('div', { className: 'tile' });
  t.key = key;
  t.v = el('video', { autoplay: true, playsInline: true, muted: true });   // l'audio passa da un altro percorso
  t.ph = el('div', { className: 'ph' });
  t.label = el('div', { className: 'label' });
  t.tc = el('div', { className: 'tcontrols' });
  t.pinB = iconBtn('pin', 'Metti in primo piano', () => togglePin(key));
  t.fsB = iconBtn('maximize', 'Schermo intero', () => fullscreen(t));
  t.append(t.v, t.ph, t.label, t.tc);
  t.tc.append(t.pinB, t.fsB);
  t.ondblclick = () => fullscreen(t);
  return t;
}
function togglePin(key) { pinKey = pinKey === key ? null : key; renderTiles(); }
function fullscreen(t) {
  if (document.fullscreenElement) { document.exitFullscreen(); return; }
  const f = t.requestFullscreen ? t.requestFullscreen() : (t.v.webkitEnterFullscreen && t.v.webkitEnterFullscreen());
  if (f && f.catch) f.catch(() => toast('Lo schermo intero non è disponibile qui.'));
}
document.addEventListener('fullscreenchange', () => {
  for (const t of tiles.values()) t.fsB.innerHTML = ico(document.fullscreenElement === t ? 'minimize' : 'maximize');
});

function renderTiles() {
  const wanted = new Map();
  if (voice.active && me) {
    wanted.set('cam:' + me.id, { id: me.id, kind: 'cam', stream: voice.camStream, label: dname(me.id) + ' (tu)', video: !!voice.cam, muted: isMuted(), deaf: voice.deaf });
    if (voice.scr) wanted.set('scr:' + me.id, { id: me.id, kind: 'screen', stream: voice.scrStream, label: 'Il tuo schermo', video: true });
    for (const [id, peer] of voice.peers) {
      const m = voice.members.get(id);
      if (!m) continue;
      wanted.set('cam:' + id, { id, kind: 'cam', stream: peer.streams.cam, label: dname(id, m.username), video: m.cam, muted: m.muted, deaf: m.deaf });
      if (m.screen) wanted.set('scr:' + id, { id, kind: 'screen', stream: peer.streams.screen, label: 'Schermo di ' + dname(id, m.username), video: true });
    }
  }
  for (const [key, t] of tiles) {
    if (!wanted.has(key)) { t.v.srcObject = null; t.remove(); tiles.delete(key); if (pinKey === key) pinKey = null; }
  }
  for (const [key, w] of wanted) {
    let t = tiles.get(key);
    if (!t) {
      t = makeTile(key); tiles.set(key, t); $('grid').append(t);
      if (w.kind === 'screen' && pinKey === null) pinKey = key;      // una condivisione appena iniziata va in primo piano
    }
    if (t.v.srcObject !== w.stream) t.v.srcObject = w.stream || null;
    const hv = !!(w.video && w.stream);
    t.classList.toggle('hasvideo', hv);
    t.classList.toggle('screen', w.kind === 'screen');
    t.classList.toggle('speaking', w.kind === 'cam' && isSpeaking(w.id));
    t.classList.toggle('pinned', pinKey === key);
    t.pinB.title = pinKey === key ? 'Togli dal primo piano' : 'Metti in primo piano';
    t.pinB.classList.toggle('on', pinKey === key);
    t.label.replaceChildren();
    if (w.kind === 'cam' && (w.muted || w.deaf)) t.label.insertAdjacentHTML('beforeend', ico(w.deaf ? 'headphones-off' : 'mic-off'));
    t.label.append(el('span', { textContent: w.label }));
    if (w.kind === 'cam') {
      const a = avatar(w.id, pinKey ? 40 : 84);
      t.ph.replaceChildren(a);
    } else t.ph.replaceChildren();
    // volume: solo sulle persone degli altri
    t.vol && t.vol.remove();
    if (w.kind === 'screen' && me && w.id !== me.id && peerHasScreenAudio(w.id)) {
      t.vol = iconBtn(volOf('s' + w.id).m ? 'volume-x' : 'volume', 'Volume audio dello schermo', (e) => showProfile(w.id, e, true));
      t.vol.dataset.keepPop = '1';
      t.tc.prepend(t.vol);
    }
    if (w.kind === 'cam' && me && w.id !== me.id) {
      t.vol = iconBtn(volOf(w.id).m ? 'volume-x' : 'volume', 'Volume di ' + dname(w.id), (e) => { e.currentTarget.dataset.keepPop = '1'; showProfile(w.id, e); });
      t.vol.dataset.keepPop = '1';
      t.tc.prepend(t.vol);
    }
    if (hv && t.hasvideoPrev !== hv) t.v.play().catch(() => {});
    t.hasvideoPrev = hv;
  }
  $('grid').classList.toggle('has-pin', !!(pinKey && tiles.has(pinKey)));
}

function renderAll() { renderSide(); renderControls(); renderTiles(); renderView(); renderMemberPane(); }

/* ---------- chi sta parlando ---------- */
function isSpeaking(id) {
  const w = watchers.get(me && id === me.id ? 'me' : 'p' + id);
  return !!(w && w.speaking);
}
function applySpeaking() {
  for (const [key, t] of tiles) {
    if (key.startsWith('cam:')) t.classList.toggle('speaking', isSpeaking(Number(key.slice(4))));
  }
  document.querySelectorAll('.vmem .av').forEach((a) => a.classList.toggle('speaking', isSpeaking(Number(a.dataset.uid))));
}
function watchSpeaking(key, stream) {
  try {
    ensureAudioCtx();
    const an = actx.createAnalyser();
    an.fftSize = 512;
    actx.createMediaStreamSource(stream).connect(an);    // non va alle casse: serve solo a misurare
    watchers.set(key, { an, buf: new Uint8Array(an.fftSize), last: 0, speaking: false });
  } catch {}
}
setInterval(() => {
  if (!watchers.size) return;
  const now = performance.now();
  let changed = false;
  for (const [key, w] of watchers) {
    w.an.getByteTimeDomainData(w.buf);
    let sum = 0;
    for (const b of w.buf) { const x = (b - 128) / 128; sum += x * x; }
    if (Math.sqrt(sum / w.buf.length) > 0.03) w.last = now;
    const silenced = key === 'me' && isMuted();
    const speaking = !silenced && now - w.last < 300;
    if (speaking !== w.speaking) { w.speaking = speaking; changed = true; }
  }
  if (changed) applySpeaking();
}, 100);

/* ---------- messaggi dal server ---------- */
function onVoiceState(m) {
  if (m.v <= voice.lastV) return;
  voice.lastV = m.v;
  voice.members = new Map(m.members.map((x) => [x.id, x]));
  if (voice.active && m.v >= voice.joinV && me && !voice.members.has(me.id)) { leaveVoice(false); return; }
  for (const id of [...voice.peers.keys()]) {
    const pm = voice.members.get(id);
    if (!pm || pm.channel !== voice.channel) closePeer(id);
  }
  renderAll();
}
function onVoiceJoined(m) {
  voice.joining = false;
  voice.active = true;
  voice.joinV = m.v;
  voice.channel = m.channel;
  if (voice.mic) { voice.mic.enabled = !isMuted(); watchSpeaking('me', new MediaStream([voice.mic])); }
  for (const p of m.peers) createPeer(p.id, true);
  if (master) master.gain.value = voice.deaf ? 0 : 1;
  sendMeta();
  inStage = true;
  setStatus('ok', 'Connesso');
  renderAll();
}
function onVoiceError(m) {
  voice.joining = false;
  [voice.mic, voice.cam, voice.scr, voice.scrAudio].forEach((t) => t && t.stop());
  voice.mic = voice.cam = voice.scr = voice.scrAudio = voice.camStream = voice.scrStream = null;
  inStage = false;
  note(m.error || 'Impossibile entrare nel canale vocale.');
  renderAll();
}
function sendMeta() {
  sendWs({ t: 'voice_meta', muted: isMuted(), deaf: voice.deaf, cam: !!voice.cam, screen: !!voice.scr });
}

/* ---------- entrare e uscire ---------- */
async function joinVoice(channel) {
  if (voice.joining || !me || !can(P.VOICE)) return;
  if (voice.active) {
    if (voice.channel === channel) return;
    leaveVoice(true);
  }
  if (!navigator.mediaDevices || !window.RTCPeerConnection) {
    note('Il browser non permette la voce qui: serve una connessione HTTPS (o localhost).');
    return;
  }
  note('');
  voice.joining = true;
  voice.channel = channel;
  inStage = true;
  ensureAudioCtx();
  closeNav();
  renderAll();
  try {
    const s = await navigator.mediaDevices.getUserMedia({ audio: micConstraints() });
    voice.mic = s.getAudioTracks()[0];
    voice.mic.enabled = !isMuted();
  } catch {
    voice.mic = null;
    note('Microfono non disponibile: entri solo in ascolto.');
  }
  if (!sendWs({ t: 'voice_join', channel })) {
    voice.joining = false;
    if (voice.mic) voice.mic.stop();
    voice.mic = null;
    inStage = false;
    note('Non sei connesso al server.');
    renderAll();
  }
}
function leaveVoice(notify = true) {
  if (notify && (voice.active || voice.joining)) sendWs({ t: 'voice_leave' });
  for (const id of [...voice.peers.keys()]) closePeer(id);
  [voice.mic, voice.cam, voice.scr, voice.scrAudio].forEach((t) => t && t.stop());
  voice.mic = voice.cam = voice.scr = voice.scrAudio = voice.camStream = voice.scrStream = null;
  watchers.clear();
  voice.active = voice.joining = false;
  voice.channel = null;
  inStage = false;
  pinKey = null;
  if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
  $('vnote').textContent = '';
  renderAll();
}

/* ---------- camera e schermo ---------- */
function afterLocalChange() {
  for (const p of voice.peers.values()) attachLocalTracks(p);
  sendMeta();
  renderAll();
}
async function toggleCam() {
  if (!voice.active) return;
  if (voice.cam) {
    voice.cam.stop();
    voice.cam = voice.camStream = null;
  } else {
    try {
      const s = await navigator.mediaDevices.getUserMedia({ video: { width: { ideal: 640 }, height: { ideal: 360 }, frameRate: { ideal: 24 } } });
      const t = s.getVideoTracks()[0];
      t.onended = () => { if (voice.cam === t) toggleCam(); };
      voice.cam = t;
      voice.camStream = new MediaStream([t]);
    } catch { note('Impossibile accedere alla camera.'); return; }
  }
  afterLocalChange();
}
async function toggleScreen() {
  if (!voice.active) return;
  if (voice.scr) {
    voice.scr.stop();
    if (voice.scrAudio) voice.scrAudio.stop();
    voice.scr = voice.scrAudio = voice.scrStream = null;
  } else {
    try {
      const s = await navigator.mediaDevices.getDisplayMedia({
        video: { frameRate: { ideal: 20 } },
        // l'audio del computer: niente filtri (rovinerebbero musica e giochi) e, per le schede, non si risente in locale
        audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false, suppressLocalAudioPlayback: true },
        systemAudio: 'include',
      });
      const t = s.getVideoTracks()[0];
      const au = s.getAudioTracks()[0] || null;
      t.contentHint = 'detail';
      t.onended = () => { if (voice.scr === t) toggleScreen(); };
      voice.scr = t;
      voice.scrAudio = au;
      voice.scrStream = new MediaStream([t]);
      if (!au) note('Schermo condiviso senza audio: nella finestra di scelta spunta "Condividi anche l\'audio" (solo Chrome ed Edge, e non con una singola finestra).');
    } catch { return; }       // annullato dalla persona
  }
  afterLocalChange();
}

/* ---------- connessioni WebRTC ---------- */
function sdpOf(d) { return { type: d.type, sdp: d.sdp }; }
function sendSignal(to, data) { sendWs({ t: 'signal', to, data }); }

function attachLocalTracks(peer) {
  const want = [voice.mic, voice.cam, voice.scr, voice.scrAudio];
  peer.pc.getTransceivers().slice(0, 4).forEach((tr, i) => {
    const track = want[i] || null;
    if (tr.sender.track === track) return;
    tr.sender.replaceTrack(track).then(() => { if (track) limitBitrate(peer, i); }).catch(() => {});
  });
}
const MAX_BITRATE = [64000, 700000, 2500000, 96000];
async function limitBitrate(peer, i) {
  try {
    const sender = peer.pc.getTransceivers()[i].sender;
    const p = sender.getParameters();
    if (!p.encodings || !p.encodings.length) return;
    p.encodings[0].maxBitrate = MAX_BITRATE[i];
    await sender.setParameters(p);
  } catch {}
}

function createPeer(id, offerer) {
  const pc = new RTCPeerConnection({ iceServers: voice.ice });
  const peer = {
    id, pc, offerer,
    session: offerer ? Math.random().toString(36).slice(2) : null,
    pendingIce: [],
    streams: { mic: new MediaStream(), cam: new MediaStream(), screen: new MediaStream(), sa: new MediaStream() },
    audio: null, gain: null, sa: null,
  };
  voice.peers.set(id, peer);
  if (offerer) {
    pc.addTransceiver('audio', { direction: 'sendrecv' });
    pc.addTransceiver('video', { direction: 'sendrecv' });
    pc.addTransceiver('video', { direction: 'sendrecv' });
    pc.addTransceiver('audio', { direction: 'sendrecv' });   // corsia 3: audio dello schermo
    attachLocalTracks(peer);
  }
  pc.onicecandidate = (e) => { if (e.candidate) sendSignal(id, { candidate: e.candidate.toJSON() }); };
  pc.ontrack = (e) => {
    const role = ROLES[pc.getTransceivers().indexOf(e.transceiver)];
    if (!role) return;
    peer.streams[role].addTrack(e.track);
    if (role === 'mic') startRemoteAudio(peer);
    if (role === 'sa') startScreenAudio(peer);
    renderTiles();
  };
  pc.onnegotiationneeded = async () => {
    if (!peer.offerer) return;
    try { await makeOffer(peer); } catch (err) { console.warn('offerta', err); }
  };
  pc.onconnectionstatechange = () => {
    if (pc.connectionState === 'connected') ROLES.forEach((_, i) => limitBitrate(peer, i));
    if (pc.connectionState === 'failed' && peer.offerer) pc.restartIce();
  };
  return peer;
}

/** L'audio degli altri passa da un nodo di guadagno: da lì vengono il volume per persona (0–200%) e le cuffie. */
function startRemoteAudio(peer) {
  if (peer.audio) return;
  const a = new Audio();
  a.autoplay = true;
  a.srcObject = peer.streams.mic;
  peer.audio = a;
  try {
    ensureAudioCtx();
    const src = actx.createMediaStreamSource(peer.streams.mic);
    peer.gain = actx.createGain();
    src.connect(peer.gain);
    peer.gain.connect(master);
    a.muted = true;                       // l'elemento serve solo a far scorrere il flusso; si sente dal nodo
    peer.src = src;
  } catch {
    peer.gain = null;
    a.muted = voice.deaf;
  }
  applyPeerGain(peer);
  a.play().catch(() => {});
  watchSpeaking('p' + peer.id, peer.streams.mic);
}

/** Audio della condivisione schermo di un altro: stesso percorso, ma con un volume suo. */
function startScreenAudio(peer) {
  if (peer.sa) return;
  const a = new Audio();
  a.autoplay = true;
  a.srcObject = peer.streams.sa;
  peer.sa = { audio: a, gain: null, src: null };
  try {
    ensureAudioCtx();
    peer.sa.src = actx.createMediaStreamSource(peer.streams.sa);
    peer.sa.gain = actx.createGain();
    peer.sa.src.connect(peer.sa.gain);
    peer.sa.gain.connect(master);
    a.muted = true;
  } catch { a.muted = voice.deaf; }
  applyPeerGain(peer);
  a.play().catch(() => {});
  renderTiles();
}
function peerHasScreenAudio(id) {
  const p = voice.peers.get(id);
  return !!(p && p.streams.sa.getAudioTracks().length);
}

function closePeer(id) {
  const peer = voice.peers.get(id);
  if (!peer) return;
  const pc = peer.pc;
  pc.onicecandidate = pc.ontrack = pc.onnegotiationneeded = pc.onconnectionstatechange = null;
  pc.close();
  if (peer.audio) peer.audio.srcObject = null;
  if (peer.sa) peer.sa.audio.srcObject = null;
  try { peer.src && peer.src.disconnect(); peer.gain && peer.gain.disconnect(); peer.sa && peer.sa.src && peer.sa.src.disconnect(); peer.sa && peer.sa.gain && peer.sa.gain.disconnect(); } catch {}
  watchers.delete('p' + id);
  voice.peers.delete(id);
  renderTiles();
}
async function makeOffer(peer) {
  const pc = peer.pc;
  if (pc.signalingState !== 'stable') return;
  await pc.setLocalDescription(await pc.createOffer());
  sendSignal(peer.id, { sdp: sdpOf(pc.localDescription), session: peer.session });
}
async function flushIce(peer) {
  for (const c of peer.pendingIce.splice(0)) { try { await peer.pc.addIceCandidate(c); } catch {} }
}
function onSignal(m) {
  signalChain = signalChain.then(() => handleSignal(m.from, m.data)).catch((e) => console.warn('segnale', e));
}
async function handleSignal(from, data) {
  if (!voice.active) return;
  let peer = voice.peers.get(from);
  if (data.sdp && data.sdp.type === 'offer') {
    if (peer && peer.session !== data.session) { closePeer(from); peer = null; }
    if (!peer) { peer = createPeer(from, false); peer.session = data.session; }
    const pc = peer.pc;
    await pc.setRemoteDescription(data.sdp);
    pc.getTransceivers().slice(0, 4).forEach((t) => { t.direction = 'sendrecv'; });
    attachLocalTracks(peer);
    await pc.setLocalDescription(await pc.createAnswer());
    sendSignal(from, { sdp: sdpOf(pc.localDescription), session: peer.session });
    await flushIce(peer);
  } else if (data.sdp && data.sdp.type === 'answer') {
    if (!peer || data.session !== peer.session) return;
    await peer.pc.setRemoteDescription(data.sdp);
    await flushIce(peer);
  } else if (data.candidate && peer) {
    if (peer.pc.remoteDescription) { try { await peer.pc.addIceCandidate(data.candidate); } catch {} }
    else peer.pendingIce.push(data.candidate);
  }
}

/* ================= avvio ================= */
hydrate();
setMode('login');
renderNsNote();
if (!navigator.mediaDevices) note('Per usare microfono e camera serve HTTPS (o localhost).');
renderControls();
token ? start() : show('auth');
