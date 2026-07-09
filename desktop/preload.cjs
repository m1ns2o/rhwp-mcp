const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('rhwpDesktop', {
  geminiGenerate: (payload) => ipcRenderer.invoke('rhwp:geminiGenerate', payload),
  mcpRequest: (method, params) => ipcRenderer.invoke('rhwp:mcpRequest', method, params),
  platform: process.platform,
});
