// preload.cjs — Secure bridge between the Electron main process and the
// React renderer.
//
// With `contextIsolation: true`, the renderer cannot touch Node or Electron
// APIs directly. This preload script runs in an isolated context and exposes
// a small, explicit API on `window.electronAPI` via `contextBridge`. This is
// the only surface the renderer has to the outside world.

const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('electronAPI', {
  // Run a search; returns a promise resolving to the JSON from the Rust backend.
  search: (query) => ipcRenderer.invoke('search', query),
  // Activate a result (launch app / open file / copy). Returns after hiding the window.
  activate: (actionType, actionData) => ipcRenderer.invoke('activate', actionType, actionData),
  // Ask the main process to hide the window (Esc key).
  hide: () => ipcRenderer.send('hide'),
  // Ask the main process to resize the window to fit the results list.
  resize: (height) => ipcRenderer.invoke('resize', height),
  // Resolve freedesktop icon names to data URLs.
  getIcons: (iconNames) => ipcRenderer.invoke('get-icons', iconNames),
  // Register a callback for when the main process signals the window was shown.
  onShow: (callback) => ipcRenderer.on('spotlight-show', () => callback()),
});
