// Hstle per Windows: apre il client web del tuo server in una finestra Chromium.
// Serve soprattutto a poter condividere lo schermo CON l'audio del computer, cosa che Firefox non permette.
const { app, BrowserWindow, Menu, session, desktopCapturer, ipcMain, shell } = require('electron');
const fs = require('fs');
const path = require('path');

// Con un gioco a schermo intero la finestra di Hstle resta coperta: Chromium la considera "nascosta" e rallenta
// o ferma cattura e invio del video. Questi interruttori lo impediscono.
app.commandLine.appendSwitch('disable-renderer-backgrounding');
app.commandLine.appendSwitch('disable-background-timer-throttling');
app.commandLine.appendSwitch('disable-backgrounding-occluded-windows');
app.commandLine.appendSwitch('disable-features', 'CalculateNativeWinOcclusion');

/* Audio di una sola app (solo Windows): il pacchetto application-loopback lancia due piccoli programmi
   (ProcessList.exe e ApplicationLoopback.exe) che leggono l'audio del processo scelto. Se manca, resta
   solo l'opzione "tutto l'audio del computer". */
let loopback = null;
if (process.platform === 'win32') {
  try {
    loopback = require('application-loopback');
    loopback.setExecutablesRoot(path.join(__dirname.replace('app.asar', 'app.asar.unpacked'), 'node_modules', 'application-loopback', 'bin'));
  } catch (e) { console.warn('audio per app non disponibile:', e.message); loopback = null; }
}

const DEFAULT_URL = 'https://giuseppefausto.ddns.net:8444';
let win = null, picker = null, serverWin = null;
let pickerDone = null;
let pickerApps = new Map();
let pickerPids = new Map();   // id sorgente -> pid del programma che possiede la finestra
let capturePid = null;

const cfgFile = () => path.join(app.getPath('userData'), 'config.json');
function cleanUrl(s) {
  try {
    const u = new URL(String(s).trim());
    if (u.protocol !== 'http:' && u.protocol !== 'https:') return null;
    return u.origin + (u.pathname === '/' ? '' : u.pathname);
  } catch { return null; }
}
function serverUrl() {
  const arg = process.argv.find((a) => a.startsWith('--url='));
  const fromArg = arg && cleanUrl(arg.slice(6));
  if (fromArg) return fromArg;
  if (process.env.HSTLE_URL && cleanUrl(process.env.HSTLE_URL)) return cleanUrl(process.env.HSTLE_URL);
  try {
    const c = cleanUrl(JSON.parse(fs.readFileSync(cfgFile(), 'utf8')).url);
    if (c) return c;
  } catch {}
  return DEFAULT_URL;
}
function saveUrl(u) {
  fs.mkdirSync(path.dirname(cfgFile()), { recursive: true });
  fs.writeFileSync(cfgFile(), JSON.stringify({ url: u }));
}
const originOf = (u) => { try { return new URL(u).origin; } catch { return ''; } };
const isOurs = (u) => originOf(u) === originOf(serverUrl());

function errorPage(url, msg) {
  const html = `<!doctype html><meta charset="utf-8"><body style="margin:0;height:100vh;display:grid;place-items:center;background:#08050f;color:#ece8f8;font:15px system-ui">
  <div style="max-width:420px;text-align:center"><h2>Non riesco a collegarmi</h2><p style="color:#8d82aa">${msg}<br>${url.replace(/</g, '')}</p>
  <p><button onclick="location.href='${url.replace(/'/g, '')}'" style="background:#8b5cf6;color:#fff;border:0;border-radius:10px;padding:10px 18px;font:inherit;cursor:pointer">Riprova</button></p>
  <p style="color:#8d82aa">Per cambiare indirizzo: menu Hstle → Cambia server.</p></div>`;
  return 'data:text/html;charset=utf-8,' + encodeURIComponent(html);
}

function createWindow() {
  win = new BrowserWindow({
    width: 1280, height: 800, minWidth: 380, minHeight: 500,
    backgroundColor: '#08050f', title: 'Hstle', autoHideMenuBar: true,
    icon: path.join(__dirname, 'icon.png'),
    webPreferences: { preload: path.join(__dirname, 'preload-app.js'), backgroundThrottling: false, contextIsolation: true, sandbox: true, nodeIntegration: false },
  });
  const url = serverUrl();
  win.loadURL(url);
  win.webContents.on('did-fail-load', (_e, code, desc, failedUrl, isMain) => {
    if (isMain && code !== -3) win.loadURL(errorPage(url, desc));
  });
  win.webContents.setWindowOpenHandler(({ url: u }) => {
    if (/^https?:/.test(u)) shell.openExternal(u);
    return { action: 'deny' };
  });
  win.webContents.on('will-navigate', (e, u) => {
    if (u.startsWith('data:') || isOurs(u)) return;
    e.preventDefault();
    if (/^https?:/.test(u)) shell.openExternal(u);
  });
  win.on('closed', () => { win = null; });
}

/* ---------- permessi: solo per il tuo server ---------- */
const OK_PERMS = new Set(['media', 'display-capture', 'fullscreen', 'clipboard-sanitized-write', 'notifications']);
function setupPermissions() {
  const ses = session.defaultSession;
  ses.setPermissionRequestHandler((wc, perm, cb, details) => cb(OK_PERMS.has(perm) && isOurs(details.requestingUrl || wc.getURL())));
  ses.setPermissionCheckHandler((wc, perm, origin) => OK_PERMS.has(perm) && isOurs(origin));

  // Condivisione schermo: finestra di scelta nostra, e audio del computer (solo Windows).
  ses.setDisplayMediaRequestHandler(async (request, callback) => {
    if (!isOurs(request.frame ? request.frame.url : '')) return callback({});
    const choice = await openPicker();
    if (!choice) return callback({});
    const sources = await desktopCapturer.getSources({ types: ['screen', 'window'] });
    const video = sources.find((s) => s.id === choice.id);
    if (!video) return callback({});
    const res = { video };
    stopAppCapture();
    if (choice.audio === 'system' && process.platform === 'win32') res.audio = 'loopback';
    else if (choice.audio === 'app') startAppCapture(choice.pid || pickerPids.get(choice.id));   // l'audio arriva alla pagina da un altro percorso
    callback(res);
  }, { useSystemPicker: false });
}

/* ---------- scelta di cosa condividere ---------- */
function openPicker() {
  return new Promise((resolve) => {
    if (picker) { picker.focus(); return resolve(null); }
    let settled = false;
    pickerDone = (v) => { if (!settled) { settled = true; resolve(v); } };
    picker = new BrowserWindow({
      width: 760, height: 560, parent: win || undefined, modal: !!win, title: 'Cosa vuoi condividere?',
      backgroundColor: '#130d20', autoHideMenuBar: true, minimizable: false,
      webPreferences: { preload: path.join(__dirname, 'preload.js'), contextIsolation: true, sandbox: true },
    });
    picker.setMenu(null);
    picker.loadFile(path.join(__dirname, 'picker.html'));
    picker.on('closed', () => { picker = null; if (pickerDone) pickerDone(null); pickerDone = null; });
  });
}
/** Trova il programma a cui appartiene una finestra (per catturare solo il suo audio). */
async function pidForWindow(source, wins) {
  const m = /^window:(\d+):/.exec(source.id);
  const hw = m ? Number(m[1]) : NaN;
  const hit = wins.find((x) => { const h = String(x.hwnd).trim(); return parseInt(h, 10) === hw || parseInt(h.replace(/^0x/i, ''), 16) === hw; })
    || wins.find((x) => x.title && x.title === source.name);
  return hit ? String(hit.processId) : null;
}
ipcMain.handle('picker:sources', async (e) => {
  if (!picker || e.sender !== picker.webContents) return [];
  const list = await desktopCapturer.getSources({ types: ['screen', 'window'], thumbnailSize: { width: 320, height: 180 }, fetchWindowIcons: false });
  let wins = [];
  if (loopback) { try { wins = await loopback.getActiveWindowProcessIds(); } catch {} }
  pickerPids = new Map();
  const out = [];
  for (const s of list) {
    const screen = s.id.startsWith('screen:');
    const pid = !screen && wins.length ? await pidForWindow(s, wins) : null;
    if (pid && Number(pid) !== process.pid) pickerPids.set(s.id, pid);
    out.push({ id: s.id, name: s.name, thumb: s.thumbnail.toDataURL(), screen, app: pickerPids.has(s.id) });
  }
  // programmi con una finestra aperta: per lo schermo intero si può scegliere l'audio di uno solo (es. il gioco)
  pickerApps = new Map();
  for (const w of wins) {
    const pid = String(w.processId);
    if (Number(pid) === process.pid || !w.title || pickerApps.has(pid)) continue;
    pickerApps.set(pid, String(w.title).slice(0, 80));
  }
  return { sources: out, apps: [...pickerApps].map(([pid, title]) => ({ pid, title })) };
});
ipcMain.handle('picker:audio', () => process.platform === 'win32');
ipcMain.on('picker:choose', (e, choice) => {
  if (!picker || e.sender !== picker.webContents) return;
  const done = pickerDone;
  pickerDone = null;
  if (done) done(choice && typeof choice.id === 'string' ? {
    id: choice.id,
    audio: ['app', 'system', 'none'].includes(choice.audio) ? choice.audio : 'none',
    pid: typeof choice.pid === 'string' && pickerApps.has(choice.pid) ? choice.pid : null,   // solo programmi dell'elenco mostrato
  } : null);
  picker.close();
});

/* ---------- audio di una sola app: dal programma alla pagina ---------- */
function startAppCapture(pid) {
  if (!loopback || !pid) return false;
  try {
    loopback.startAudioCapture(pid, { onData: (chunk) => { if (win && !win.isDestroyed()) win.webContents.send('app-audio', chunk); } });
    capturePid = pid;
    return true;
  } catch (e) { console.warn('cattura audio:', e.message); capturePid = null; return false; }
}
function stopAppCapture() {
  if (loopback && capturePid) { try { loopback.stopAudioCapture(capturePid); } catch {} }
  capturePid = null;
}
const fromMainWindow = (e) => win && e.sender === win.webContents && isOurs(e.senderFrame ? e.senderFrame.url : '');
ipcMain.handle('app-audio:active', (e) => fromMainWindow(e) && !!capturePid);
ipcMain.handle('app-audio:stop', (e) => { if (fromMainWindow(e)) stopAppCapture(); });
app.on('before-quit', stopAppCapture);

/* ---------- cambiare server ---------- */
function openServerDialog() {
  if (serverWin) { serverWin.focus(); return; }
  serverWin = new BrowserWindow({
    width: 480, height: 260, parent: win || undefined, modal: !!win, title: 'Cambia server', resizable: false,
    backgroundColor: '#130d20', autoHideMenuBar: true, minimizable: false,
    webPreferences: { preload: path.join(__dirname, 'preload.js'), contextIsolation: true, sandbox: true },
  });
  serverWin.setMenu(null);
  serverWin.loadFile(path.join(__dirname, 'server.html'));
  serverWin.on('closed', () => { serverWin = null; });
}
ipcMain.handle('server:get', (e) => (serverWin && e.sender === serverWin.webContents ? serverUrl() : ''));
ipcMain.handle('server:set', (e, value) => {
  if (!serverWin || e.sender !== serverWin.webContents) return false;
  const u = cleanUrl(value);
  if (!u) return false;
  saveUrl(u);
  serverWin.close();
  if (win) win.loadURL(u);
  return true;
});

/* ---------- avvio ---------- */
if (!app.requestSingleInstanceLock()) app.quit();
else {
  app.on('second-instance', () => { if (win) { if (win.isMinimized()) win.restore(); win.focus(); } });
  app.whenReady().then(() => {
    setupPermissions();
    Menu.setApplicationMenu(Menu.buildFromTemplate([
      { label: 'Hstle', submenu: [
        { label: 'Cambia server…', click: openServerDialog },
        { label: 'Ricarica', accelerator: 'CmdOrCtrl+R', click: () => win && win.webContents.reload() },
        { label: 'Ricarica ignorando la cache', accelerator: 'CmdOrCtrl+Shift+R', click: () => win && win.webContents.reloadIgnoringCache() },
        { label: 'Schermo intero', accelerator: 'F11', click: () => win && win.setFullScreen(!win.isFullScreen()) },
        { label: 'Strumenti per sviluppatori', accelerator: 'CmdOrCtrl+Shift+I', click: () => win && win.webContents.toggleDevTools() },
        { type: 'separator' },
        { label: 'Esci', role: 'quit' },
      ] },
    ]));
    createWindow();
  });
  app.on('window-all-closed', () => app.quit());
}
