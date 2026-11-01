const { contextBridge, ipcRenderer } = require('electron');
contextBridge.exposeInMainWorld('hstleDesktop', {
  sources: () => ipcRenderer.invoke('picker:sources'),
  canShareAudio: () => ipcRenderer.invoke('picker:audio'),
  choose: (choice) => ipcRenderer.send('picker:choose', choice),
  getServer: () => ipcRenderer.invoke('server:get'),
  setServer: (v) => ipcRenderer.invoke('server:set', v),
});
