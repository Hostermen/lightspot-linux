const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('electronAPI', {
  search: (query) => ipcRenderer.invoke('search', query),
  activate: (actionType, actionData) => ipcRenderer.invoke('activate', actionType, actionData),
  hide: () => ipcRenderer.send('hide'),
  resize: (height) => ipcRenderer.invoke('resize', height),
  onShow: (callback) => ipcRenderer.on('spotlight-show', () => callback()),
});
