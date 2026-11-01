// Preload della finestra principale: dice alla pagina che è nell'app e le passa l'audio dell'app condivisa.
const { contextBridge, ipcRenderer } = require('electron');
let handler = null;
ipcRenderer.on('app-audio', (_e, chunk) => { if (handler) handler(chunk); });
contextBridge.exposeInMainWorld('hstleApp', {
  appAudioActive: () => ipcRenderer.invoke('app-audio:active'),
  stopAppAudio: () => ipcRenderer.invoke('app-audio:stop'),
  onAppAudio: (cb) => { handler = typeof cb === 'function' ? cb : null; },
});
